use tokio::sync::{
    mpsc,
    oneshot,
};
use tracing::debug;
use wdht_wasync::SenderExt;

use crate::{Result, WrtcError, WrtcEvent};

pub struct ChannelHandler {
    ready_tx: Option<oneshot::Sender<Result<()>>>,
    inbound_tx: mpsc::Sender<Result<WrtcEvent>>,
}

impl ChannelHandler {
    #![allow(clippy::type_complexity)]
    pub fn new(
        inbound_tx: mpsc::Sender<Result<WrtcEvent>>
    ) -> (
        oneshot::Receiver<Result<()>>,
        Self,
    ) {
        let (ready_tx, ready_rx) = oneshot::channel();
        (
            ready_rx,
            Self {
                ready_tx: Some(ready_tx),
                inbound_tx,
            },
        )
    }

    pub fn open(&mut self) {
        // Signal open
        let _ = self
            .ready_tx
            .take()
            .expect("Channel already open")
            .send(Ok(()));
    }

    pub fn closed(&mut self) {
        debug!("Datachannel closed");
        let _ = self.inbound_tx.maybe_spawn_send(Err(WrtcError::ConnectionLost));
    }

    pub fn error(&mut self, err: String) {
        debug!("DataChannel on_error {}", err);
        let _ = self
            .ready_tx
            .take()
            .map(|x| x.send(Err(WrtcError::DataChannelError(err.clone().into()))));
        let _ = self
            .inbound_tx
            .maybe_spawn_send(Err(WrtcError::DataChannelError(err.into())));
    }

    pub fn message(&mut self, msg: Vec<u8>) {
        let _ = self.inbound_tx.maybe_spawn_send(Ok(WrtcEvent::Data(msg)));
    }
}
