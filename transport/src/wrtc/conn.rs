use std::{borrow::Cow, collections::HashMap, sync::{Weak, Mutex, Arc}, time::Duration, fmt::Debug};

use datachannel::RtcDataChannel;
use futures::future::join_all;
use log::{warn, debug};
use serde::Serialize;
use thiserror::Error;
use tokio::{sync::{oneshot, mpsc}, time::timeout};
use wdht_logic::{transport::{TransportError, TransportListener}, Id};

use super::{async_wrtc::{WrtcChannel, WrtcError, self, ChannelHandler}, protocol::{HandshakeRequest, HandshakeResponse, WrtcResponse, WrtcMessage, WrtcPayload, WrtcRequest}, Connections};


#[derive(Debug, Error)]
#[non_exhaustive]
pub enum PeerMessageError {
    #[error("Transport error: {0}")]
    TransportError(WrtcError),
    #[error("Wrong message format: {0}")]
    WrongFormat(serde_json::Error),
    #[error("Handshake error: {0}")]
    HandshakeError(Cow<'static, str>),
    #[error("Unknown answer id")]
    UnknownAnswerId,
    #[error("Internal error: {0}")]
    InternalError(Box<dyn std::error::Error>),
    #[error("Unknown internal error: {0}")]
    UnknownInternalError(&'static str)
}

impl From<WrtcError> for PeerMessageError {
    fn from(x: WrtcError) -> Self {
        PeerMessageError::TransportError(x)
    }
}

impl From<serde_json::Error> for PeerMessageError {
    fn from(x: serde_json::Error) -> Self {
        PeerMessageError::WrongFormat(x)
    }
}

fn encode_data<T: Serialize>(data: &T) -> Result<Vec<u8>, PeerMessageError> {
    serde_json::to_vec(data)
        .map_err(|x| PeerMessageError::InternalError(Box::new(x)))
}

pub async fn handshake_passive(conn: &mut WrtcChannel, id: Id) -> Result<Id, PeerMessageError> {
    let msg = conn.listener.recv().await
        .ok_or(WrtcError::ConnectionLost)??;
    let req = serde_json::from_slice::<HandshakeRequest>(&msg)?;

    let peer_id = req.my_id;

    let ans = HandshakeResponse::Ok {
        my_id: id,
    };
    let msg = encode_data(&ans)?;
    conn.data_channel.send(&msg)
        .map_err(|_| WrtcError::ConnectionLost)?;

    Ok(peer_id)
}

pub async fn handshake_active(conn: &mut WrtcChannel, id: Id) -> Result<Id, PeerMessageError> {
    let msg = encode_data(&HandshakeRequest {
        my_id: id,
    })?;
    conn.data_channel.send(&msg)
        .map_err(|_| WrtcError::ConnectionLost)?;

    let msg = conn.listener.recv().await
        .ok_or(WrtcError::ConnectionLost)??;
    let res = serde_json::from_slice::<HandshakeResponse>(&msg)?;

    match res {
        // The message has been sent from the other peer (so its their id)
        HandshakeResponse::Ok { my_id } => Ok(my_id),
        HandshakeResponse::Error { error } => Err(PeerMessageError::HandshakeError(error.into())),
    }
}

struct InnerWrtcConnection {
    next_id: u32,
    responses: HashMap<u32, oneshot::Sender<Result<WrtcResponse, TransportError>>>,
    _conn: async_wrtc::Connection,
    channel: Box<RtcDataChannel<ChannelHandler>>,
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
pub struct WrtcConnection {
    pub(crate) peer_id: Id,
    inner: Mutex<InnerWrtcConnection>,
    parent: Weak<Connections>,
}

impl WrtcConnection {
    pub fn new(peer_id: Id, channel: WrtcChannel, parent: Weak<Connections>) -> Arc<Self> {
        let WrtcChannel {
            peer_connection,
            data_channel,
            listener
        } = channel;
        let res = Arc::new(Self {
            peer_id,
            inner: Mutex::new(InnerWrtcConnection {
                next_id: 0,
                responses: HashMap::new(),
                _conn: peer_connection,
                channel: data_channel,
            }),
            parent,
        });

        tokio::spawn(connection_listen(listener, Arc::downgrade(&res)));
        res
    }

    pub async fn send_request(self: &Arc<Self>, mex: WrtcRequest) -> Result<WrtcResponse, TransportError> {
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
        parent.connection.lock().unwrap().remove(&self.peer_id);

        let mut inner = self.inner.lock().unwrap();
        for (_id, resp) in inner.responses.drain() {
            let _ = resp.send(Err(TransportError::ConnectionLost));
        }
    }
}

fn process_message(msg: &[u8], conn: Arc<WrtcConnection>) -> Result<(), PeerMessageError> {
    let msg: WrtcMessage = serde_json::from_slice(&msg)?;
    let req = match msg.payload {
        WrtcPayload::Req(x) => x,
        WrtcPayload::Res(x) => {
            let mut inner = conn.inner.lock().unwrap();
            let response = inner.responses.remove(&msg.id)
                .ok_or(PeerMessageError::UnknownAnswerId)?;
            // Ignore sending error
            let _ = response.send(Ok(x));
            return Ok(());
        },
    };

    let root = conn.parent.upgrade()
        .ok_or(PeerMessageError::UnknownInternalError("Shutting down"))?;

    match req {
        WrtcRequest::Req(x) => {
            let dht = match root.dht.upgrade() {
                Some(x) => x,
                None => return Ok(()),// Shutting down
            };
            let ans = dht.on_request(conn.peer_id, x);
            let mut inner = conn.inner.lock().unwrap();
            inner.send_response(msg.id, WrtcResponse::Ans(ans));
        },
        WrtcRequest::ForwardOffer(offers) => {
            let connections = root.connection.lock().unwrap();
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
            let weak_ptr = Arc::downgrade(&conn);
            tokio::spawn( async move {
                let results = fut.await;
                let connection = match weak_ptr.upgrade() {
                    Some(x) => x,
                    None => return,
                };
                connection.send_response(msg.id,WrtcResponse::ForwardAnswers(results));
            });
        },
        WrtcRequest::TryOffer(offer) => {
            let weak_ptr = Arc::downgrade(&conn);
            tokio::spawn(async move {
                let res = match root.create_passive(offer).await {
                    Ok(x) => WrtcResponse::OkAnswer(Ok(x)),
                    Err(x) => WrtcResponse::OkAnswer(Err(x.to_string())),
                };
                if let Some(x) = weak_ptr.upgrade() {
                    x.send_response(msg.id, res);
                }
            });
        },
    }

    Ok(())
}

async fn connection_listen(mut mex_rx: mpsc::Receiver<Result<Vec<u8>, WrtcError>>, conn: Weak<WrtcConnection>) {
    while let Some(msg) = mex_rx.recv().await {
        match (msg, conn.upgrade()) {
            (Ok(x), Some(conn)) => {
                if let Err(x) = process_message(&x, conn) {
                    warn!("Error while processing message: {}", x);
                    return;
                }
            }
            (Err(WrtcError::ConnectionLost), _) | (_, None) => {
                debug!("Connection lost");
                return;
            }
            (Err(x), _) => {
                warn!("Peer message error: {}", x);
                return;
            }
        }
    }
}
