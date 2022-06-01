mod error;
mod base;

pub use error::WrtcError;
use serde::{Serialize, Deserialize};
use tokio::sync::{oneshot, mpsc};

// Re-export things to make them prettier and consistent with base changes.

#[derive(Serialize, Deserialize, Debug)]
#[serde(transparent)]
pub struct SessionDescription(base::SessionDescription);

pub enum ConnectionRole<E: From<WrtcError>> {
    // Active: sends offer and awaits an answer
    Active(oneshot::Receiver<Result<SessionDescription, E>>),
    // Passive: receives an offer, sends an answer back
    Passive(SessionDescription),
}

pub struct WrtcChannel {
    pub sender: WrtcDataChannel,
    pub listener: mpsc::Receiver<Result<Vec<u8>, WrtcError>>,
}

// If this is dropped the connection is also closed
pub struct WrtcDataChannel(base::WrtcDataChannel);

impl WrtcDataChannel {
    pub fn send(&mut self, msg: &[u8]) -> Result<(), WrtcError> {
        self.0.send(msg)
    }
}

#[derive(Clone, Debug)]
pub struct RtcConfig(base::RtcConfig);

impl RtcConfig {
    pub fn new<S: AsRef<str>>(ice_servers: &[S]) -> Self {
        RtcConfig(base::RtcConfig::new(ice_servers))
    }
}

pub async fn create_channel<E>(config: &RtcConfig, role: ConnectionRole<E>, answer: oneshot::Sender<SessionDescription>) ->
        Result<WrtcChannel, E>
    where E: From<WrtcError> {
    base::create_channel(&config.0, role, answer).await
}


#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
