use std::{sync::{Mutex, atomic::{AtomicU64, Ordering}}, collections::{HashMap, VecDeque}, num::NonZeroU64};

use tracing::{info, error, warn, debug, event, Level};
use once_cell::sync::Lazy;
use tokio::sync::oneshot;
use async_broadcast as broadcast;
use wdht_logic::{KademliaDht, Id, config::SystemConfig, transport::{TransportListener, TransportError}};
use wdht_wrtc::{RtcConfig, WrtcChannel, ConnectionRole, SessionDescription, create_channel, WrtcError};

use self::{conn::{WrtcConnection, PeerMessageError}, connector::{WrtcConnector, CreatingConnectionSender, ContactResult}, wasync::{Orc, Weak, spawn}};

use super::wasync;
mod conn;
mod connector;
mod error;
mod protocol;
mod sender;

pub use sender::{WrtcSender, WrtcContact};
pub use error::WrtcTransportError;


pub struct Connections {
    pub dht: Weak<KademliaDht<WrtcSender>>,
    pub self_id: Id,// Same ase dht.upggrade().unwrap().id
    max_connections: Option<NonZeroU64>,
    connection_count: AtomicU64,
    // TODO: use some locking hashmap?
    pub connections: Mutex<HashMap<Id, Orc<WrtcConnection>>>,
    half_closed_connections: Mutex<VecDeque<Id>>,
    half_closed_count: AtomicU64,
    pub connector: Orc<WrtcConnector>,
}

static RTC_CONFIG: Lazy<RtcConfig> = Lazy::new(|| {
    // TODO: have more STUN servers.
    RtcConfig::new(&["stun:stun1.l.google.com:19302",])
});

impl Connections {
    pub fn create(config: SystemConfig, id: Id) -> Orc<KademliaDht<WrtcSender>> {
        Orc::new_cyclic(|weak_dht| {
            let connections = Orc::new(Connections {
                dht: weak_dht.clone(),
                max_connections: config.routing.max_connections,
                self_id: id,
                connections: Mutex::new(HashMap::new()),
                connection_count: AtomicU64::new(0),
                half_closed_count: AtomicU64::new(0),
                half_closed_connections: Mutex::new(VecDeque::new()),
                connector: Orc::new(WrtcConnector::new(id)),
            });
            let sender = WrtcSender(connections);

            KademliaDht::new(config, id, sender)
        })
    }

    fn after_handshake(
        self: Orc<Self>,
        channel: WrtcChannel,
        res: Result<Id, PeerMessageError>,
        conn_tx: CreatingConnectionSender
    ) {
        let id = match res {
            Ok(x) => x,
            Err(x) => {
                warn!("Handshake error {}", x);
                conn_tx.send(Err(TransportError::HandshakeError));
                self.connection_count.fetch_sub(1, Ordering::SeqCst);
                return;
            }
        };
        if !conn_tx.is_last() {
            conn_tx.send(Err("Already connecting".into()));
            self.connection_count.fetch_sub(1, Ordering::SeqCst);
            return;
        }
        debug!("{} connected", id);
        let connection = conn::WrtcConnection::new(id, channel, Orc::downgrade(&self));

        {
            let mut conns = self.connections.lock().unwrap();
            if conns.contains_key(&id) {
                event!(Level::ERROR, kad_id=%self.self_id, "Same id connection conflict!");
                return;
            }
            conns.insert(id, connection.clone());
        }
        self.dht.upgrade()
            .map(|x| {
                // Inform the connection that it's used in the routing table
                connection.set_dont_cleanup(x.on_connect(id));
            });
        let connection = WrtcContact::Other(connection);
        conn_tx.send(Ok(connection));
    }

    fn alloc_connection(self: &Orc<Self>) -> bool {
        let limit = match self.max_connections {
            Some(x) => x,
            None => {
                self.connection_count.fetch_add(1, Ordering::SeqCst);
                return true;
            }
        }.get();

        let r = self.connection_count.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |x| {
            if x < limit {
                Some(x + 1)
            } else {
                None
            }
        });

        if let Ok(_) = r {
            return true;// The connection permit is ours, wohoo!
        }
        // Connections are full, let's try to get an half-connection that
        // we can close.
        let r = self.half_closed_count.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |x| {
            if x > 0 {
                Some(x - 1)
            } else {
                None
            }
        });
        if let Err(_) = r {
            // We didn't get any permit even from the half-closed connections
            // In italian i might say "questa connessione non s'ha da fare"
            return false;
        }
        // We got an half-close connection that we can reuse
        let id = match self.half_closed_connections.lock().unwrap().pop_front() {
            Some(x) => x,
            None => {
                error!("Half-closed connections and atomic counter out of sync");
                return false;
            }
        };
        let conn = match self.connections.lock().unwrap().remove(&id) {
            Some(x) => x,
            None => {
                warn!("Half-closed connection was not present in connections!");
                return true;
            }
        };
        // Do not update the connection count, and don't even update the half-closed queue
        // (we already took care of that)
        self.on_disconnect(id, false, false);
        conn.shutdown_local();
        true
    }

    async fn create_channel_and_register(
        this: Weak<Self>,
        role: ConnectionRole<WrtcTransportError>,
        answer_tx: oneshot::Sender<SessionDescription>,
        is_active: bool,
        conn_tx: CreatingConnectionSender
    ) {
        let channel = create_channel(&RTC_CONFIG, role, answer_tx).await;

        let this = match this.upgrade() {
            Some(x) => x,
            None => return,
        };

        match channel {
            Ok(mut channel) => {
                let res = if is_active {
                    conn::handshake_active(&mut channel, this.self_id).await
                } else {
                    conn::handshake_passive(&mut channel, this.self_id).await
                };
                this.after_handshake(channel, res, conn_tx);
            },
            Err(x) => {
                this.connection_count.fetch_sub(1, Ordering::SeqCst);
                conn_tx.send(Err(format!("{}", x).into()));
                debug!("Error opening connection {}", x);
            },
        };
    }

    pub async fn create_passive(self: Orc<Self>, id: Id, offer: SessionDescription) -> Result<(SessionDescription, broadcast::Receiver<ContactResult>), WrtcTransportError> {
        let (conn_tx, conn_rx) = self.connector.create_passive(id);
        let conn_tx = match conn_tx {
            Some(x) => x,
            None => return Err(WrtcTransportError::AlreadyConnecting),
        };
        if !self.alloc_connection() {
            info!("Cannot create passive connection: connection limit reached");
            return Err(WrtcTransportError::ConnectionLimitReached);
        }

        let (answer_tx, answer_rx) = oneshot::channel();
        let this = Orc::downgrade(&self);
        drop(self);

        let role = ConnectionRole::Passive(offer);
        spawn(
            Self::create_channel_and_register(this.clone(), role, answer_tx, false, conn_tx)
        );

        debug!("Waiting for passive answer...");
        answer_rx.await
            .map(|x| (x, conn_rx))
            .map_err(|_| {
                this.upgrade().map(|x| x.connection_count.fetch_sub(1, Ordering::SeqCst));
                WrtcError::SignalingFailed.into()
            })
    }

    pub async fn create_active_with_connector(self: Orc<Self>, sender: CreatingConnectionSender) -> Result<(SessionDescription, oneshot::Sender<Result<SessionDescription, WrtcTransportError>>), WrtcTransportError> {
        if !self.alloc_connection() {
            return Err(WrtcTransportError::ConnectionLimitReached);
        }

        let (answer_tx, answer_rx) = oneshot::channel();
        let (offer_tx, offer_rx) = oneshot::channel();

        let this = Orc::downgrade(&self);
        drop(self);

        let role = ConnectionRole::Active(answer_rx);
        spawn(Self::create_channel_and_register(this.clone(), role, offer_tx, true, sender));

        let offer = offer_rx.await.map_err(|_| {
            this.upgrade().map(|x| x.connection_count.fetch_sub(1, Ordering::SeqCst));
            WrtcError::SignalingFailed
        })?;
        Ok((offer, answer_tx))
    }

    pub async fn create_active(self: Orc<Self>, id: Option<Id>) -> Result<(SessionDescription, oneshot::Sender<Result<SessionDescription, WrtcTransportError>>, broadcast::Receiver<ContactResult>), WrtcTransportError> {
        let (conn_tx, conn_rx) = match id {
            Some(id) => match self.connector.create_active(id) {
                (Some(sender), chan) => (sender, chan),
                (None, _) => return Err(WrtcTransportError::AlreadyConnecting),
            },
            None => self.connector.create_unknown(),
        };

        let (offer, answer_tx) = self.create_active_with_connector(conn_tx).await?;
        Ok((offer, answer_tx, conn_rx))
    }

    fn on_disconnect(&self, peer_id: Id, update_conn_count: bool, was_half_closed: bool) {
        info!("{} disconnected (half_closed: {})", peer_id, was_half_closed);
        self.connections.lock().unwrap().remove(&peer_id);
        if update_conn_count {
            self.connection_count.fetch_sub(1, Ordering::SeqCst);
        }

        if was_half_closed {
            self.half_closed_count.fetch_sub(1, Ordering::SeqCst);
            let mut half_closed_vec = self.half_closed_connections.lock().unwrap();
            if let Some(index) = half_closed_vec.iter().position(|x| *x == peer_id) {
                half_closed_vec.remove(index);
            } else {
                // Caller said that the closed connection was half_closed but it's not in the vec
                // so it's a liar!!
                warn!("webrtc connections, on_disconnect called but was_half_closed lied!");
            }
        }

        self.dht.upgrade().map(|dht| dht.on_disconnect(peer_id));
    }

    pub(crate) fn on_half_closed(&self, conn: Id) {
        info!("{} half_closed", conn);
        self.half_closed_connections.lock().unwrap().push_back(conn);
        self.half_closed_count.fetch_add(1, Ordering::SeqCst);
    }
}
