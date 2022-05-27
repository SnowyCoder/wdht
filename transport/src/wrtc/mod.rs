use std::{sync::{Mutex, Arc, Weak, atomic::{AtomicU64, Ordering}}, collections::{HashMap, VecDeque}, num::NonZeroU64};

use datachannel::{RtcConfig, SessionDescription};
use tracing::{info, error, warn, debug};
use once_cell::sync::Lazy;
use tokio::sync::oneshot;
use wdht_logic::{KademliaDht, Id, config::SystemConfig, transport::TransportListener};

use self::{conn::{WrtcConnection, PeerMessageError}, async_wrtc::{WrtcError, ConnectionRole, WrtcChannel}, connector::WrtcConnector};

mod conn;
mod connector;
mod protocol;
pub mod async_wrtc;
mod sender;

pub use sender::{WrtcSender, WrtcContact};


pub struct Connections {
    pub dht: Weak<KademliaDht<WrtcSender>>,
    pub self_id: Id,// Same ase dht.upggrade().unwrap().id
    max_connections: Option<NonZeroU64>,
    connection_count: AtomicU64,
    // TODO: use some locking hashmap?
    pub connections: Mutex<HashMap<Id, Arc<WrtcConnection>>>,
    half_closed_connections: Mutex<VecDeque<Id>>,
    half_closed_count: AtomicU64,
    pub connector: WrtcConnector,
}

static RTC_CONFIG: Lazy<RtcConfig> = Lazy::new(|| {
    // TODO: have more STUN servers.
    let mut config = RtcConfig::new(&["stun1.l.google.com:19302",]);
    config.disable_auto_negotiation = true;
    config
});

impl Connections {
    pub fn create(config: SystemConfig, id: Id) -> Arc<KademliaDht<WrtcSender>> {
        Arc::new_cyclic(|weak_dht| {
            let connections = Arc::new(Connections {
                dht: weak_dht.clone(),
                max_connections: config.routing.max_connections,
                self_id: id,
                connections: Mutex::new(HashMap::new()),
                connection_count: AtomicU64::new(0),
                half_closed_count: AtomicU64::new(0),
                half_closed_connections: Mutex::new(VecDeque::new()),
                connector: Default::default(),
            });
            let sender = WrtcSender(connections);

            KademliaDht::new(config, id, sender)
        })
    }

    fn after_handshake(
        self: Arc<Self>,
        channel: WrtcChannel,
        res: Result<Id, PeerMessageError>,
        conn_tx: Option<oneshot::Sender<WrtcContact>>
    ) {
        let id = match res {
            Ok(x) => x,
            Err(x) => {
                warn!("Handshake error {}", x);
                self.connection_count.fetch_sub(1, Ordering::SeqCst);
                return;
            }
        };
        debug!("{} connected", id);
        let connection = conn::WrtcConnection::new(id, channel, Arc::downgrade(&self));

        self.connections.lock().unwrap().insert(id, connection.clone());
        self.dht.upgrade()
            .map(|x| {
                // Inform the connection that it's used in the routing table
                connection.set_dont_cleanup(x.on_connect(id));
            });
        let connection = WrtcContact::Other(connection);
        if let Some(listener) = conn_tx {
            let _ = listener.send(connection);
        }
    }

    fn alloc_connection(self: &Arc<Self>) -> bool {
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
        role: ConnectionRole,
        answer_tx: oneshot::Sender<SessionDescription>,
        is_active: bool,
        conn_tx: Option<oneshot::Sender<WrtcContact>>
    ) {
        let channel = async_wrtc::create_channel(&RTC_CONFIG, role, answer_tx).await;

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
                debug!("Error opening connection {}", x);
                return;
            },
        };
    }

    pub async fn create_passive(self: Arc<Self>, offer: SessionDescription) -> Result<SessionDescription, WrtcError> {
        if !self.alloc_connection() {
            info!("Cannot create passive connection: connection limit reached");
            return Err(WrtcError::ConnectionLimitReached);
        }

        let (answer_tx, answer_rx) = oneshot::channel();
        let this = Arc::downgrade(&self);
        drop(self);

        let role = ConnectionRole::Passive(offer);
        tokio::spawn(
            Self::create_channel_and_register(this.clone(), role, answer_tx, false, None)
        );

        debug!("Waiting for passive answer...");
        answer_rx.await.map_err(|_| {
            this.upgrade().map(|x| x.connection_count.fetch_sub(1, Ordering::SeqCst));
            WrtcError::SignalingFailed
        })
    }

    pub async fn create_active(self: Arc<Self>) -> Result<(SessionDescription, oneshot::Sender<SessionDescription>, oneshot::Receiver<WrtcContact>), WrtcError> {
        if !self.alloc_connection() {
            return Err(WrtcError::ConnectionLimitReached);
        }

        let (answer_tx, answer_rx) = oneshot::channel();
        let (offer_tx, offer_rx) = oneshot::channel();
        let (conn_tx, conn_rx) = oneshot::channel();

        let this = Arc::downgrade(&self);
        drop(self);

        let role = ConnectionRole::Active(answer_rx);
        tokio::spawn(Self::create_channel_and_register(this.clone(), role, offer_tx, true, Some(conn_tx)));

        let offer = offer_rx.await.map_err(|_| {
            this.upgrade().map(|x| x.connection_count.fetch_sub(1, Ordering::SeqCst));
            WrtcError::SignalingFailed
        })?;
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
