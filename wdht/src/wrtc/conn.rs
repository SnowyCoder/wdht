use std::{collections::HashMap, fmt::Debug, sync::Mutex, time::Duration};

use futures::future::join_all;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, span, warn, Instrument, Level};
use wdht_logic::{
    transport::{TransportError, TransportListener},
    Id,
};
use wdht_wrtc::{WrtcChannel, WrtcDataChannel, WrtcError, RawConnection, WrtcEvent, RawChannel};
use wdht_wasync::{sleep, spawn, Orc, Weak};

use crate::events::{TransportEvent, ChannelOpenEvent, DisconnectReason};

use super::{
    protocol::{
        WrtcMessage, WrtcPayload, WrtcRequest, WrtcResponse,
    },
    Connections, WrtcTransportError,
};

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum PeerMessageError {
    #[error("Transport error: {0}")]
    TransportError(WrtcTransportError),
    #[error("Wrong message format: {0}")]
    WrongFormat(serde_json::Error),
    #[error("Unknown answer id")]
    UnknownAnswerId,
    #[error("Unknown internal error: {0}")]
    UnknownInternalError(&'static str),
}

impl From<WrtcError> for PeerMessageError {
    fn from(x: WrtcError) -> Self {
        PeerMessageError::TransportError(x.into())
    }
}

impl From<WrtcTransportError> for PeerMessageError {
    fn from(x: WrtcTransportError) -> Self {
        PeerMessageError::TransportError(x)
    }
}

impl From<serde_json::Error> for PeerMessageError {
    fn from(x: serde_json::Error) -> Self {
        PeerMessageError::WrongFormat(x)
    }
}

struct InnerWrtcConnection {
    next_id: u32,
    responses: HashMap<u32, oneshot::Sender<Result<WrtcResponse, TransportError>>>,
    channel: WrtcDataChannel,
    /// True when the connection is also used in routing tables (so we can't drop the connection)
    dont_cleanup: bool,
    /// If true the peer won't be issuing other requests but will still answer requests
    other_half_closed: bool,
    this_half_closed: bool,
}

impl InnerWrtcConnection {
    fn wrap_message(&mut self, mex: WrtcRequest) -> WrtcMessage {
        let req_id = self.next_id;
        self.next_id = req_id.wrapping_add(1);
        WrtcMessage {
            id: req_id,
            payload: WrtcPayload::Req(mex),
        }
    }

    fn send_raw(&mut self, mex: WrtcRequest) -> Result<(), WrtcError> {
        let message = self.wrap_message(mex);
        let data = serde_json::to_vec(&message).expect("Failed to serialize");

        self.channel
            .send(&data)
            .map_err(|_| WrtcError::DataChannelError("Failed to send message".into()))
    }

    pub fn send_request(
        &mut self,
        mex: WrtcRequest,
    ) -> oneshot::Receiver<Result<WrtcResponse, TransportError>> {
        let message = self.wrap_message(mex);
        debug!("Send: {:?}", message);

        let (send, recv) = oneshot::channel();
        self.responses.insert(message.id, send);

        let data = serde_json::to_vec(&message).expect("Failed to serialize");
        if let Err(_err) = self.channel.send(&data) {
            self.responses
                .remove(&message.id)
                .map(|x| x.send(Err("Failed to send message".into())));
        }

        recv
    }

    pub fn send_response(&mut self, id: u32, res: WrtcResponse) -> Result<(), ()> {
        let message = WrtcMessage {
            id,
            payload: WrtcPayload::Res(res),
        };

        debug!("Send: {:?}", message);
        let data = serde_json::to_vec(&message).expect("Failed to serialize");
        match self.channel.send(&data) {
            Err(x) => {
                warn!("Failed to send message: {}", x);
                Err(())
            }
            Ok(_) => Ok(()),
        }
    }
}
pub struct WrtcConnection {
    pub(crate) peer_id: Id,
    inner: Mutex<InnerWrtcConnection>,
    parent: Weak<Connections>,
}

impl WrtcConnection {
    pub fn new(peer_id: Id, channel: WrtcChannel, parent: Weak<Connections>) -> Orc<Self> {
        let kad_id = parent.upgrade().unwrap().dht.upgrade().unwrap().id();
        let WrtcChannel { sender, listener } = channel;
        let res = Orc::new(Self {
            peer_id,
            inner: Mutex::new(InnerWrtcConnection {
                next_id: 0,
                responses: HashMap::new(),
                channel: sender,
                dont_cleanup: false,
                other_half_closed: false,
                this_half_closed: false,
            }),
            parent,
        });

        spawn(
            connection_listen(listener, Orc::downgrade(&res)).instrument(
                span!(parent: None, Level::INFO, "kad_listener_wrtc", %kad_id, peer_id=%peer_id),
            ),
        );
        res
    }

    pub async fn send_request(
        self: Orc<Self>,
        mex: WrtcRequest,
    ) -> Result<WrtcResponse, TransportError> {
        let reply = self.inner.lock().unwrap().send_request(mex);

        let weak = Orc::downgrade(&self);
        drop(self);

        tokio::select! {
            _ = sleep(Duration::from_secs(10 * 60)) => {
                // Timeout expired, connection is not alive
                let this = match weak.upgrade() {
                    Some(x) => x,
                    None => return Err(TransportError::ConnectionLost),
                };
                this.shutdown(DisconnectReason::TimeoutExpired);
                Err(TransportError::ConnectionLost)
            }
            x = reply => {
                match x {
                    Ok(x) => x,
                    Err(_) => Err(TransportError::ConnectionLost),
                }
            }
        }
    }

    fn send_response(&self, id: u32, res: WrtcResponse) {
        if self.inner.lock().unwrap().send_response(id, res).is_err() {
            self.shutdown(DisconnectReason::SendFail);
        }
    }

    fn shutdown(&self, reason: DisconnectReason) {
        debug!("Shutting down connection: {reason}");
        let parent = match self.parent.upgrade() {
            Some(x) => x,
            None => return,
        };
        parent.connections.lock().unwrap().remove(&self.peer_id);
        parent.on_disconnect(
            self.peer_id,
            reason,
            true,
            self.inner.lock().unwrap().this_half_closed,
        );

        self.shutdown_local();
    }

    pub(crate) fn shutdown_local(&self) {
        let mut inner = self.inner.lock().unwrap();
        for (_id, resp) in inner.responses.drain() {
            let _ = resp.send(Err(TransportError::ConnectionLost));
        }
    }

    fn send_half_close(&self) -> Result<(), WrtcError> {
        self.inner.lock().unwrap().send_raw(WrtcRequest::HalfClose)
    }

    /// Called when the last usable contact is lost, will try to close (or half-close) the connection
    pub fn on_contact_lost(self: &Orc<Self>) {
        let other_half_closed = {
            let mut inner = self.inner.lock().unwrap();
            if inner.dont_cleanup {
                return; // Can't close this half, it's used in the routing table
            }
            if !inner.other_half_closed {
                // Don't set this half closed, we're closing the connection instantly
                inner.this_half_closed = true;
            }
            inner.other_half_closed
        };
        if other_half_closed {
            self.shutdown(DisconnectReason::HalfCloseBoth);
        } else {
            if let Some(x) = self.parent.upgrade() {
                x.on_half_closed(self.peer_id);
            }
            if let Err(x) = self.send_half_close() {
                warn!("Failed to send half-close: {}", x);
            }
        }
    }

    pub fn set_dont_cleanup(self: &Orc<Self>, dont_cleanup: bool) {
        self.inner.lock().unwrap().dont_cleanup = dont_cleanup;
    }

    pub fn raw_connection(&self) -> RawConnection {
        self.inner.lock().unwrap().channel.raw_connection()
    }
}

fn process_message(msg: &[u8], conn: Orc<WrtcConnection>) -> Result<(), PeerMessageError> {
    let msg: WrtcMessage = serde_json::from_slice(msg)?;
    debug!("Received message: {:?}", msg);
    let req = match msg.payload {
        WrtcPayload::Req(x) => x,
        WrtcPayload::Res(x) => {
            let mut inner = conn.inner.lock().unwrap();
            let response = inner
                .responses
                .remove(&msg.id)
                .ok_or(PeerMessageError::UnknownAnswerId)?;
            // Ignore sending error
            let _ = response.send(Ok(x));
            return Ok(());
        }
    };

    let root = conn
        .parent
        .upgrade()
        .ok_or(PeerMessageError::UnknownInternalError("Shutting down"))?;

    match req {
        WrtcRequest::Req(x) => {
            let dht = match root.dht.upgrade() {
                Some(x) => x,
                None => return Ok(()), // Shutting down
            };
            let ans = dht.on_request(conn.peer_id, x);
            conn.send_response(msg.id, WrtcResponse::Ans(ans));
        }
        WrtcRequest::ForwardOffer(offers) => {
            let connections = root.connections.lock().unwrap();
            let fut = join_all(offers.into_iter().map(|(id, offer)| {
                let oconn = connections.get(&id).cloned();
                let peer_id = conn.peer_id;
                async move {
                    match oconn {
                        Some(x) => {
                            match x.send_request(WrtcRequest::TryOffer(peer_id, offer)).await {
                                Ok(WrtcResponse::OkAnswer(x)) => x,
                                Ok(_) => Err("peer_error".into()),
                                Err(_) => Err("not_found".into()),
                            }
                        }
                        None => Err("not_found".into()),
                    }
                }
            }));
            let weak_ptr = Orc::downgrade(&conn);
            spawn(async move {
                let results = fut.await;
                let connection = match weak_ptr.upgrade() {
                    Some(x) => x,
                    None => return,
                };
                connection.send_response(msg.id, WrtcResponse::ForwardAnswers(results));
            });
        }
        WrtcRequest::TryOffer(id, offer) => {
            if root.connections.lock().unwrap().contains_key(&id) {
                conn.send_response(
                    msg.id,
                    WrtcResponse::OkAnswer(Err("already_connected".into())),
                );
                return Ok(());
            }

            let weak_ptr = Orc::downgrade(&conn);
            spawn(async move {
                let res = match root.create_passive(id, offer).await {
                    Ok((desc, _)) => WrtcResponse::OkAnswer(Ok(desc)),
                    Err(x) => WrtcResponse::OkAnswer(Err(x.to_string())),
                };
                if let Some(x) = weak_ptr.upgrade() {
                    x.send_response(msg.id, res);
                }
            });
        }
        WrtcRequest::HalfClose => {
            let mut inner = conn.inner.lock().unwrap();
            inner.other_half_closed = true;
            if inner.this_half_closed {
                drop(inner);
                conn.shutdown(DisconnectReason::HalfCloseBoth);
            }
        }
    }

    Ok(())
}

async fn process_channel(channel: RawChannel, conn: Orc<WrtcConnection>) -> Result<(), PeerMessageError> {
    let root = conn
        .parent
        .upgrade()
        .ok_or(PeerMessageError::UnknownInternalError("Shutting down"))?;

    let connection = conn.inner.lock().unwrap().channel.raw_connection().clone();
    let _ = root.events_tx.broadcast(TransportEvent::ChannelOpen(ChannelOpenEvent {
        id: conn.peer_id,
        connection,
        channel,
    })).await;
    Ok(())
}

async fn connection_listen(
    mut mex_rx: mpsc::Receiver<Result<WrtcEvent, WrtcError>>,
    conn: Weak<WrtcConnection>,
) {
    // TODO: add proper shutdown reason
    while let Some(msg) = mex_rx.recv().await {
        match (msg, conn.upgrade()) {
            (Ok(WrtcEvent::OpenChannel(x)), Some(conn)) => {
                if let Err(x) = process_channel(x, conn).await {
                    warn!("Error while processing open channel: {}", x);
                    break;
                }
            }
            (Ok(WrtcEvent::Data(x)), Some(conn)) => {
                if let Err(x) = process_message(&x, conn) {
                    warn!("Error while processing message: {}", x);
                    break;
                }
            }
            (Err(WrtcError::ConnectionLost), _) | (_, None) => {
                debug!("Connection lost");
                break;
            }
            (Err(x), _) => {
                warn!("Peer message error: {}", x);
                break;
            }
        }
    }
    if let Some(x) = conn.upgrade() {
        x.shutdown(DisconnectReason::ConnectionLost);
    }
}
