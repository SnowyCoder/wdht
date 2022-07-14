mod base;
mod error;

pub use error::{Result, WrtcError};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};

pub use base::RawConnection;
pub use base::RawChannel;

// Re-export things to make them prettier and consistent with base changes.

#[derive(Serialize, Deserialize, Debug)]
#[serde(transparent)]
pub struct SessionDescription(base::SessionDescription);

pub enum ConnectionRole<E: From<WrtcError>> {
    // Active: sends offer and awaits an answer
    Active(oneshot::Receiver<core::result::Result<SessionDescription, E>>),
    // Passive: receives an offer, sends an answer back
    Passive(SessionDescription),
}

pub enum WrtcEvent {
    Data(Vec<u8>),
    OpenChannel(RawChannel),
}

impl WrtcEvent {
    pub fn data(self) -> Option<Vec<u8>> {
        match self {
            Self::Data(x) => Some(x),
            _ => None,
        }
    }

    pub fn open_channel(self) -> Option<RawChannel> {
        match self {
            Self::OpenChannel(x) => Some(x),
            _ => None,
        }
    }
}

pub struct WrtcChannel {
    pub sender: WrtcDataChannel,
    pub listener: mpsc::Receiver<Result<WrtcEvent>>,
}

// If this is dropped the connection is also closed
pub struct WrtcDataChannel(base::WrtcDataChannel);

impl WrtcDataChannel {
    pub fn send(&mut self, msg: &[u8]) -> Result<()> {
        self.0.send(msg)
    }

    // Use with caution! Not supported in native (for now)
    pub fn raw_connection(&self) -> RawConnection {
        self.0.raw_connection()
    }

    pub fn local_certificate_fingerprint(&self) -> Result<Vec<u8>> {
        self.0.local_certificate_fingerprint()
    }

    pub fn remote_certificate_fingerprint(&self) -> Result<Vec<u8>> {
        self.0.remote_certificate_fingerprint()
    }
}

#[derive(Clone, Debug)]
pub struct RtcConfig(base::RtcConfig);

impl RtcConfig {
    pub fn new<S: AsRef<str>>(ice_servers: &[S]) -> Self {
        RtcConfig(base::RtcConfig::new(ice_servers))
    }
}

pub async fn create_channel<E>(
    config: &RtcConfig,
    role: ConnectionRole<E>,
    answer: oneshot::Sender<SessionDescription>
) -> core::result::Result<WrtcChannel, E>
where
    E: From<WrtcError>
{
    base::create_channel(&config.0, role, answer).await
}
