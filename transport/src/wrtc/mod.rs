use std::{sync::{Mutex, Arc, Weak}, collections::HashMap};

use datachannel::{RtcConfig, SessionDescription};
use log::info;
use once_cell::sync::Lazy;
use tokio::sync::oneshot;
use wdht_logic::{KademliaDht, Id, config::SystemConfig};

use self::{conn::{WrtcConnection, PeerMessageError}, async_wrtc::{WrtcError, ConnectionRole, WrtcChannel}};

mod conn;
mod protocol;
pub mod async_wrtc;
mod sender;

pub use sender::{WrtcSender, WrtcContact};


pub struct Connections {
    pub dht: Weak<KademliaDht<WrtcSender>>,
    pub self_id: Id,// Same ase dht.upggrade().unwrap().id
    // TODO: use some locking hashmap?
    pub connection: Mutex<HashMap<Id, Arc<WrtcConnection>>>,
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
                self_id: id,
                connection: Mutex::new(HashMap::new()),
            });
            let sender = WrtcSender(connections);

            KademliaDht::new(config, id, sender)
        })
    }

    fn after_handshake(
        self: Arc<Self>,
        channel: WrtcChannel,
        res: Result<Id, PeerMessageError>,
        conn_tx: Option<oneshot::Sender<Arc<WrtcConnection>>>
    ) {
        let id = match res {
            Ok(x) => x,
            Err(x) => {
                log::warn!("Handshake error {}", x);
                return;
            }
        };
        let connection = conn::WrtcConnection::new(id, channel, Arc::downgrade(&self));

        self.connection.lock().unwrap().insert(id, connection.clone());
        if let Some(listener) = conn_tx {
            let _ = listener.send(connection);
        }
    }

    pub async fn create_passive(self: Arc<Self>, offer: SessionDescription) -> Result<SessionDescription, WrtcError> {
        let (answer_tx, answer_rx) = oneshot::channel();

        tokio::spawn(async move {
            let role = ConnectionRole::Passive(offer);

            match async_wrtc::create_channel(&RTC_CONFIG, role, answer_tx).await {
                Ok(mut channel) => {
                    let res = conn::handshake_passive(&mut channel, self.self_id).await;
                    self.after_handshake(channel, res, None);
                },
                Err(x) => {
                    log::debug!("Error opening connection {}", x);
                    return;
                },
            };
        });
        info!("Waiting for passive answer...");
        answer_rx.await.map_err(|_| WrtcError::SignalingFailed)
    }

    pub async fn create_active(self: Arc<Self>) -> Result<(SessionDescription, oneshot::Sender<SessionDescription>, oneshot::Receiver<Arc<WrtcConnection>>), WrtcError> {
        let (answer_tx, answer_rx) = oneshot::channel();
        let (offer_tx, offer_rx) = oneshot::channel();
        let (conn_tx, conn_rx) = oneshot::channel();

        let weak_ptr = Arc::downgrade(&self);
        tokio::spawn(async move {
            let role = ConnectionRole::Active(answer_rx);

            match async_wrtc::create_channel(&RTC_CONFIG, role, offer_tx).await {
                Ok(mut channel) => {
                    let res = conn::handshake_active(&mut channel, self.self_id).await;
                    weak_ptr.upgrade().map(|x| x.after_handshake(channel, res, Some(conn_tx)));
                },
                Err(x) => {
                    log::debug!("Error opening connection {}", x);
                    return;
                },
            };
        });
        let offer = offer_rx.await.map_err(|_| WrtcError::SignalingFailed)?;
        Ok((offer, answer_tx, conn_rx))
    }
}
