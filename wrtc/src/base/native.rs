use std::sync::{Arc, Mutex, Weak};

use datachannel::{
    ConnectionState, DataChannelHandler, DataChannelInit, GatheringState, IceCandidate,
    PeerConnectionHandler, RtcConfig as InnerConfig, RtcDataChannel, RtcPeerConnection, SdpType,
    SignalingState,
};
use tokio::sync::{oneshot, mpsc};
use tracing::{debug, error, info};

use super::common::ChannelHandler;
use crate::{
    error::WrtcError, ConnectionRole, SessionDescription as WrappedSessionDescription, WrtcChannel,
    WrtcDataChannel as WrappedWrtcDataChannel,
};

use datachannel::SessionDescription as RawSessionDescription;

pub type SessionDescription = Box<RawSessionDescription>;
pub type RawConnection = ();// Not available on native!
pub type RawChannel = ();// Not available on native!

type Connection = Arc<Mutex<Box<RtcPeerConnection<ConnectionHandler>>>>;
pub struct WrtcDataChannel {
    // Only kept to keep the original connection from being deallocated
    _peer_connection: Connection,
    data_channel: Box<RtcDataChannel<ChannelHandler>>,
}

impl WrtcDataChannel {
    pub fn send(&mut self, msg: &[u8]) -> Result<(), WrtcError> {
        self.data_channel
            .send(msg)
            .map_err(|_| WrtcError::DataChannelError("runtime error".into()))
    }

    pub fn raw_connection(&self) -> RawConnection {
        ()
    }
}

#[derive(Clone, Debug)]
pub struct RtcConfig(InnerConfig);

impl RtcConfig {
    pub fn new<S: AsRef<str>>(ice_servers: &[S]) -> Self {
        let mut conf = InnerConfig::new(ice_servers);
        conf.disable_auto_negotiation = true;
        RtcConfig(conf)
    }
}

pub async fn create_channel<E>(
    config: &RtcConfig,
    role: ConnectionRole<E>,
    answer: oneshot::Sender<WrappedSessionDescription>,
) -> Result<WrtcChannel, E>
where
    E: From<WrtcError>,
{
    let (inbound_tx, inbound_rx) = mpsc::channel(16);
    let (conn, state_rx) = create_connection(config, answer);

    let (ready, chan) = ChannelHandler::new(inbound_tx);
    let dc_init = DataChannelInit::default()
        .negotiated()
        .manual_stream()
        .stream(0)
        .protocol("wrtc_json");

    let dc = conn
        .lock()
        .unwrap()
        .create_data_channel_ex("wdht", chan, &dc_init)
        .expect("Invalid args provided");

    match role {
        ConnectionRole::Active(answer_rx) => {
            conn.lock()
                .unwrap()
                .set_local_description(SdpType::Offer)
                .expect("Error setting local description");
            let answer = answer_rx.await.map_err(|_| WrtcError::SignalingFailed)??;
            conn.lock().unwrap().set_remote_description(&answer.0)
        }
        ConnectionRole::Passive(offer) => {
            let mut conn = conn.lock().unwrap();
            conn.set_remote_description(&offer.0)
                .and_then(|_| conn.set_local_description(SdpType::Answer))
        }
    }
    .map_err(|_| WrtcError::InvalidDescription)?;

    // Wait for the connection to open
    if !state_rx.await.map_err(|_| WrtcError::SignalingFailed)? {
        return Err(WrtcError::SignalingFailed.into());
    }
    // Wait for the datachannel to be ready
    ready.await.map_err(|_| WrtcError::SignalingFailed)??;

    debug!("Datachannel open");

    // Return the newly created channel (along with the PeerConnection so it does not close)
    Ok(WrtcChannel {
        sender: WrappedWrtcDataChannel(WrtcDataChannel {
            _peer_connection: conn,
            data_channel: dc,
        }),
        listener: inbound_rx,
    })
}

// Private implementation

fn create_connection(
    config: &RtcConfig,
    signal_tx: oneshot::Sender<WrappedSessionDescription>,
) -> (Connection, oneshot::Receiver<bool>) {
    let (state_tx, state_rx) = oneshot::channel();
    let conn = Arc::new_cyclic(|parent| {
        Mutex::new(
            RtcPeerConnection::new(
                &config.0,
                ConnectionHandler {
                    signal_tx: Some(signal_tx),
                    ready_tx: Some(state_tx),
                    parent: parent.clone(),
                },
            )
            .expect("Failed to create RtcPeerConnection"),
        )
    });
    (conn, state_rx)
}

impl DataChannelHandler for ChannelHandler {
    fn on_open(&mut self) {
        // Signal open
        self.open();
    }

    fn on_closed(&mut self) {
        self.closed();
    }

    fn on_error(&mut self, err: &str) {
        self.error(err.to_string());
    }

    fn on_message(&mut self, msg: &[u8]) {
        self.message(msg.to_vec());
    }

    // TODO: implement back-pressure
    fn on_buffered_amount_low(&mut self) {}

    fn on_available(&mut self) {}
}

struct ConnectionHandler {
    signal_tx: Option<oneshot::Sender<WrappedSessionDescription>>,
    ready_tx: Option<oneshot::Sender<bool>>,
    parent: Weak<Mutex<Box<RtcPeerConnection<ConnectionHandler>>>>,
}

impl PeerConnectionHandler for ConnectionHandler {
    type DCH = ChannelHandler;

    fn data_channel_handler(&mut self) -> Self::DCH {
        let (tx, _rx) = mpsc::channel(0);
        let (_, chan) = ChannelHandler::new(tx);
        chan
    }

    fn on_description(&mut self, _sess_desc: RawSessionDescription) {
        // We can't use this to send the description back since we need to wait
        // until ICE candidates are all gathered (this method gets called
        // instantly since it implements the trickle ICE protocol).
    }

    fn on_candidate(&mut self, _cand: IceCandidate) {}

    fn on_connection_state_change(&mut self, state: ConnectionState) {
        debug!("Connection state change: {:?}", state);
        use ConnectionState::*;
        let is_successful = match state {
            New | Connecting => return,
            Connected => true,
            Disconnected | Failed | Closed => false,
        };
        self.ready_tx.take().map(|x| x.send(is_successful));
    }

    fn on_gathering_state_change(&mut self, state: GatheringState) {
        if let GatheringState::Complete = state {
            let signal_listener = match self.signal_tx.take() {
                Some(x) => x,
                None => return, // Double listen (or we simply ignore the result)
            };

            let par = match self.parent.upgrade() {
                Some(x) => x,
                None => return, // Connection closed
            };
            let sess_desc = match par.lock().unwrap().local_description() {
                Some(x) => x,
                None => {
                    error!("Gathering complete but no local description provided");
                    return;
                }
            };

            // Ignore if signal is not needed
            let _ = signal_listener.send(WrappedSessionDescription(Box::new(sess_desc)));
        }
    }

    fn on_data_channel(&mut self, _data_channel: Box<RtcDataChannel<Self::DCH>>) {
        info!("Peer tried to open data channel");
        // Data channel not supported on native connections (yet)
    }

    fn on_signaling_state_change(&mut self, _state: SignalingState) {}
}
