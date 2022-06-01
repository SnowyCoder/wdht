use tokio::sync::{oneshot, mpsc::{self, error::TrySendError}};
use tracing::debug;

use crate::WrtcError;


pub struct ChannelHandler {
    ready_tx: Option<oneshot::Sender<Result<(), WrtcError>>>,
    inbound_tx: mpsc::Sender<Result<Vec<u8>, WrtcError>>,
}

impl ChannelHandler {
    pub fn new() -> (
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

    pub fn open(&mut self) {
        // Signal open
        let _ = self.ready_tx.take()
            .expect("Channel already open")
            .send(Ok(()));
    }

    pub fn closed(&mut self) {
        debug!("Datachannel closed");
        if let Err(TrySendError::Full(x)) = self.inbound_tx.try_send(Err(WrtcError::ConnectionLost)) {
            let itx = self.inbound_tx.clone();
            tokio::spawn(async move { itx.send(x).await });
        }
    }

    pub fn error(&mut self, err: String) {
        debug!("DataChannel on_error {}", err);
        let _ = self
            .ready_tx
            .take()
            .map(|x| x.send(Err(WrtcError::DataChannelError(err.clone().into()))));
        let _ = self
            .inbound_tx
            .blocking_send(Err(WrtcError::DataChannelError(err.into())));
    }

    pub fn message(&mut self, msg: Vec<u8>) {
        let _ = self.inbound_tx.blocking_send(Ok(msg));
    }
}
