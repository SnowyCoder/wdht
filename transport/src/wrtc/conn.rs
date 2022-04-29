use std::{collections::{HashMap, HashSet}, sync::{Mutex, Weak, Arc}, time::Duration, error::Error, future};

use datachannel::{RtcPeerConnection, RtcDataChannel, DataChannelHandler, PeerConnectionHandler, RtcConfig};
use futures::future::join_all;
use thiserror::Error;
use tokio::{sync::oneshot, time::timeout};
use wdht_logic::{transport::{RawResponse, Request, TransportError, TransportListener, TransportSender}, Id, KademliaDht};

use crate::util::ArcKey;

use super::{protocol::{WrtcResponse, WrtcRequest, HandshakeRequest, HandshakeResponse, WrtcMessage, WrtcPayload}, Connections};


#[derive(Clone)]
pub struct MyTransportSender;

impl TransportSender for MyTransportSender {
    fn ping(&self, _id: &Id) {
    }

    type Fut = future::Ready<Result<RawResponse<Id>, TransportError>>;

    fn send(&self, id: &Id, msg: Request) -> Self::Fut {
        todo!()
    }

    fn wrap_contact(&self, id: Id) -> Self::Contact {
        todo!()
    }

    type Contact = Id;
}



struct InnerWrtcConnection {
    is_master: bool,
    peer_id: Option<Id>,
    next_id: u32,
    responses: HashMap<u32, oneshot::Sender<Result<WrtcResponse, TransportError>>>,
    connection: Box<RtcPeerConnection<Handler>>,
    channel: Box<RtcDataChannel<Handler>>,
}
pub struct WrtcConnection {
    inner: Mutex<InnerWrtcConnection>,
    parent: Weak<Connections>,
}


#[derive(Clone)]
struct Handler(Weak<WrtcConnection>);

impl PeerConnectionHandler for Handler {
    type DCH = Handler;

    fn data_channel_handler(&mut self) -> Self::DCH {
        self.clone()
    }
}

#[derive(Debug, Error)]

#[non_exhaustive]
enum PeerMessageError {
    #[error("Wrong message format: {0}")]
    WrongFormat(serde_json::Error),
    #[error("Handshake error: {0}")]
    HandshakeError(String),
    #[error("Unknown answer id")]
    UnknownAnswerId,
    #[error("Internal error: {0}")]
    InternalError(Box<dyn Error>),
    #[error("Unknown internal error: {0}")]
    UnknownInternalError(&'static str)
}

impl Handler {
    fn handle_request(sender_id: Id, conns: &Arc<Connections>, conn: &mut InnerWrtcConnection,
            reply_to: u32, req: WrtcRequest, backref: &Arc<WrtcConnection>) {

        match req {
            WrtcRequest::Req(x) => {
                let ans = conns.dht.on_request(sender_id, x);
                conn.send_response(reply_to, WrtcResponse::Ans(ans));
            },
            WrtcRequest::ForwardOffer(offers) => {
                let connections = conns.connections_by_id.lock().unwrap();
                let fut = join_all(offers.into_iter()
                        .map(|(id, offer)| {
                    let conn = connections.get(&id).cloned();
                    async {
                        match conn {
                            Some(x) => {
                                match x.send_request(WrtcRequest::TryOffer(offer)).await {
                                    Ok(WrtcResponse::OkAnswer(x)) => x,
                                    Ok(_) => Err("peer_error".into()),
                                    Err(_) => Err("not_found".into()),
                                }
                            },
                            None => Err("not_found".into()),
                        }
                    }
                }));
                let weak_ptr = Arc::downgrade(backref);
                tokio::spawn( async move {
                    let results = fut.await;
                    let connection = match weak_ptr.upgrade() {
                        Some(x) => x,
                        None => return,
                    };
                    connection.send_response(reply_to,WrtcResponse::ForwardAnswers(results));
                });
            },
            WrtcRequest::TryOffer(offer) => {
                conns.
            },
        }
    }

    fn handle_message(&mut self, msg: &[u8]) -> Result<(), PeerMessageError> {
        let conn = match self.0.upgrade() {
            Some(x) => x,
            None => return Ok(()),// Connection is closed
        };
        let mut inner = conn.inner.lock().unwrap();
        let parent = conn.parent.upgrade()
                .ok_or(PeerMessageError::UnknownInternalError("Shutting down"))?;

        match &inner.peer_id {
            None => {
                // Not initialized yet,
                // this must be the handshake
                if inner.is_master {
                    let req = serde_json::from_slice::<HandshakeRequest>(msg)
                        .map_err(|e| PeerMessageError::WrongFormat(e))?;

                    inner.peer_id = Some(req.my_id);
                    let ans = HandshakeResponse::Ok {
                        my_id: conn.parent.upgrade()
                                .ok_or(PeerMessageError::UnknownInternalError("Shutting down"))?
                                .dht.id(),
                    };
                    let data = serde_json::to_vec(&ans)
                            .map_err(|e| PeerMessageError::InternalError(Box::new(e)))?;
                    // Ignore sending error
                    let _ = inner.channel.send(&data);
                } else {
                    let ans = serde_json::from_slice::<HandshakeResponse>(msg)
                        .map_err(|e| PeerMessageError::WrongFormat(e))?;

                    inner.peer_id = match ans {
                        HandshakeResponse::Ok { my_id } => Some(my_id),
                        HandshakeResponse::Error { error } => Err(PeerMessageError::HandshakeError(error))?,
                    };
                }
            }
            Some(peer_id) => {
                let message = serde_json::from_slice::<WrtcMessage>(msg)
                        .map_err(PeerMessageError::WrongFormat)?;

                match message.payload {
                    WrtcPayload::Req(x) => {
                        Self::handle_request(*peer_id, &parent, &mut inner, message.id, x, &conn);
                    },
                    WrtcPayload::Res(x) => {
                        let response = inner.responses.remove(&message.id)
                            .ok_or(PeerMessageError::UnknownAnswerId)?;
                        // Ignore sending error
                        let _ = response.send(Ok(x));
                    },
                }
            }
        }
        Ok(())
    }
}

impl DataChannelHandler for Handler {
    fn on_error(&mut self, err: &str) {
        if let Some(x) = self.0.upgrade() {
            log::info!("Error with wrtc channel {:?}: {}", x.inner.lock().unwrap().peer_id, err);
            x.shutdown()
        }
    }

    fn on_closed(&mut self) {
        if let Some(x) = self.0.upgrade() {
            log::info!("Wrtc channel closed: {:?}", x.inner.lock().unwrap().peer_id);
            x.shutdown()
        }
    }

    fn on_message(&mut self, msg: &[u8]) {
        if let Err(e) = self.handle_message(msg) {
            log::warn!("Peer error while handling message: {}", e);
            if let Some(x) = self.0.upgrade() {
                x.shutdown();
            }
        }
    }
}

impl InnerWrtcConnection {
    pub fn send_request(&mut self, mex: WrtcRequest) -> oneshot::Receiver<Result<WrtcResponse, TransportError>> {
        let req_id = self.next_id;
        self.next_id += 1;
        let message = WrtcMessage {
            id: req_id,
            payload: WrtcPayload::Req(mex),
        };

        let (send, recv) = oneshot::channel();
        self.responses.insert(req_id, send);

        // TODO: better error management
        let data = serde_json::to_vec(&message).expect("Failed to serialize");
        self.channel.send(&data).expect("Failed to send message to channel");

        recv
    }

    pub fn send_response(&mut self, id: u32, res: WrtcResponse) {
        let message = WrtcMessage {
            id,
            payload: WrtcPayload::Res(res),
        };

        // TODO: better error management
        let data = serde_json::to_vec(&message).expect("Failed to serialize");
        self.channel.send(&data).expect("Failed to send message to channel");
    }
}

impl WrtcConnection {
    pub fn create(config: &RtcConfig) -> Self {

        RtcPeerConnection::new(config, Handler);


    }

    pub async fn send_request(self: Arc<Self>, mex: WrtcRequest) -> Result<WrtcResponse, TransportError> {
        let reply = self.inner.lock().unwrap().send_request(mex);

        let weak = Arc::downgrade(&self);
        drop(self);

        let duration = Duration::from_secs(10);
        match timeout(duration, reply).await {
            Ok(Err(_)) => {
                return Err(TransportError::ConnectionLost);
            },
            Ok(Ok(x)) => {
                x
            },
            Err(_) => {
                // Timeout expired, connection is not alive
                let this = match weak.upgrade() {
                    Some(x) => x,
                    None => return Err(TransportError::ConnectionLost),
                };
                this.shutdown();
                // TODO: Destroy connection
                Err(TransportError::ConnectionLost)
            },
        }
    }

    fn send_response(&self, id: u32, res: WrtcResponse) {
        self.inner.lock().unwrap().send_response(id, res)
    }

    fn shutdown(self: Arc<Self>) {
        let parent = match self.parent.upgrade() {
            Some(x) => x,
            None => return,
        };
        let mut inner = self.inner.lock().unwrap();
        if let Some(id) = inner.peer_id {
            parent.connections_by_id.lock().unwrap().remove(&id);
        }

        parent.connections.lock().unwrap().remove(&ArcKey(self.clone()));
        for (_id, resp) in inner.responses.drain() {
            let _ = resp.send(Err(TransportError::ConnectionLost));
        }
    }
}
