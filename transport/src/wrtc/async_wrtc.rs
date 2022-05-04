use std::sync::{Arc, Mutex, Weak};

pub use datachannel::{ConnectionState, IceCandidate, RtcConfig, SessionDescription};
use datachannel::{DataChannelHandler, PeerConnectionHandler, RtcDataChannel, RtcPeerConnection, DataChannelInit, SdpType, GatheringState, SignalingState};
use thiserror::Error;
use tokio::sync::{oneshot, mpsc};
use log::{debug, error};

#[derive(Error, Debug)]
pub enum WrtcError {
    #[error("DataChannel error {0}")]
    DataChannelError(String),
    #[error("Connection lost")]
    ConnectionLost,
    #[error("Singaling failed")]
    SignalingFailed,
    #[error("Invalid session description")]
    InvalidDescription,
}

pub enum ConnectionRole {
    // Active: sends offer and awaits an answer
    Active(oneshot::Receiver<SessionDescription>),
    // Passive: receives an offer, sends an answer back
    Passive(SessionDescription),
}

pub struct WrtcChannel {
    pub peer_connection: Connection,
    pub data_channel: Box<RtcDataChannel<ChannelHandler>>,
    pub listener: mpsc::Receiver<Result<Vec<u8>, WrtcError>>,
}

pub async fn create_channel(config: &RtcConfig, role: ConnectionRole, answer: oneshot::Sender<SessionDescription>) -> Result<WrtcChannel, WrtcError> {
    let (conn, state_rx) = create_connection(config, Some(answer));

    let (ready, rx_inbound, chan) = ChannelHandler::new();
    let dc_init = DataChannelInit::default()
        .negotiated()
        .protocol("wrtc_json");

    let dc = conn.lock().unwrap()
        .create_data_channel_ex("wdht", chan, &dc_init)
        .expect("Invalid args provided");
    {

        match role {
            ConnectionRole::Active(answer_rx) => {
                conn.lock().unwrap().set_local_description(SdpType::Offer)
                    .expect("Error setting local description");
                let answer = answer_rx.await.map_err(|_| WrtcError::SignalingFailed)?;
                conn.lock().unwrap().set_remote_description(&answer)
            },
            ConnectionRole::Passive(offer) => {
                let mut conn = conn.lock().unwrap();
                conn.set_remote_description(&offer)
                    .and_then(|_| conn.set_local_description(SdpType::Answer))
            },
        }.map_err(|_| WrtcError::InvalidDescription)?;
    }

    // Wait for the connection to open
    if !state_rx.await.map_err(|_| WrtcError::SignalingFailed)? {
        return Err(WrtcError::SignalingFailed);
    }
    // Wait for the datachannel to be ready
    ready.await
        .map_err(|_| WrtcError::SignalingFailed)??;

    // Return the newly created channel (along with the PeerConnection so it does not close)
    Ok(WrtcChannel {
        peer_connection: conn,
        data_channel: dc,
        listener: rx_inbound,
    })
}

// Private implementation

pub type Connection = Arc<Mutex<Box<RtcPeerConnection<ConnectionHandler>>>>;

fn create_connection(
    config: &RtcConfig,
    signal_tx: Option<oneshot::Sender<SessionDescription>>,
) -> (Connection, oneshot::Receiver<bool>) {
    let (state_tx, state_rx) = oneshot::channel();
    let conn = Arc::new_cyclic(|parent| Mutex::new(RtcPeerConnection::new(
        config,
        ConnectionHandler {
            signal_tx,
            ready_tx: Some(state_tx),
            parent: parent.clone(),
        },
    ).expect("Failed to create RtcPeerConnection")));
    (conn, state_rx)
}

pub struct ChannelHandler {
    ready_tx: Option<oneshot::Sender<Result<(), WrtcError>>>,
    inbound_tx: mpsc::Sender<Result<Vec<u8>, WrtcError>>,
}

impl ChannelHandler {
    fn new() -> (
        oneshot::Receiver<Result<(), WrtcError>>,
        mpsc::Receiver<Result<Vec<u8>, WrtcError>>,
        Self,
    ) {
        let (ready_tx, ready_rx) = oneshot::channel();
        let (inbound_tx, inbound_rx) = mpsc::channel(16);
        (
            ready_rx,
            inbound_rx,
            Self {
                ready_tx: Some(ready_tx),
                inbound_tx,
            },
        )
    }
}

impl DataChannelHandler for ChannelHandler {
    fn on_open(&mut self) {
        // Signal open
        let _ = self.ready_tx.take()
            .expect("Channel already open")
            .send(Ok(()));
    }

    fn on_closed(&mut self) {
        let _ = self.inbound_tx.send(Err(WrtcError::ConnectionLost));
    }

    fn on_error(&mut self, err: &str) {
        debug!("DataChannel on_error {}", err);
        let _ = self
            .ready_tx
            .take()
            .map(|x| x.send(Err(WrtcError::DataChannelError(err.to_string()))));
        let _ = self
            .inbound_tx
            .try_send(Err(WrtcError::DataChannelError(err.to_string())));
    }

    fn on_message(&mut self, msg: &[u8]) {
        let _ = self.inbound_tx.try_send(Ok(msg.to_vec()));
    }

    // TODO: implement back-pressure
    fn on_buffered_amount_low(&mut self) {
    }

    fn on_available(&mut self) {
    }
}

pub struct ConnectionHandler {
    signal_tx: Option<oneshot::Sender<SessionDescription>>,
    ready_tx: Option<oneshot::Sender<bool>>,
    parent: Weak<Mutex<Box<RtcPeerConnection<ConnectionHandler>>>>,
}

impl PeerConnectionHandler for ConnectionHandler {
    type DCH = ChannelHandler;

    fn data_channel_handler(&mut self) -> Self::DCH {
        let (_, _, chan) = ChannelHandler::new();
        chan
    }

    fn on_description(&mut self, _sess_desc: SessionDescription) {
        // We can't use this to send the description back since we need to wait
        // until ICE candidates are all gathered (this method gets called
        // instantly since it implements the trickle ICE protocol).
    }

    fn on_candidate(&mut self, _cand: IceCandidate) {
    }

    fn on_connection_state_change(&mut self, state: ConnectionState) {
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
                None => return,// Double listen (or we simply ignore the result)
            };

            let par = match self.parent.upgrade() {
                Some(x) => x,
                None => return,// Connection closed
            };
            let sess_desc = match par.lock().unwrap().local_description() {
                Some(x) => x,
                None => {
                    error!("Gathering complete but no local description provided");
                    return;
                }
            };

            // Ignore if signal is not needed
            let _ = signal_listener.send(sess_desc);
        }
    }

    fn on_data_channel(&mut self, _data_channel: Box<RtcDataChannel<Self::DCH>>) {
    }

    fn on_signaling_state_change(&mut self, _state: SignalingState) {
    }
}
