use core::fmt;

use async_broadcast::RecvError;
use wdht_logic::Id;
use wdht_wrtc::{RawConnection, RawChannel};

use crate::wrtc::WrtcContact;

#[derive(Clone, Debug)]
pub struct ChannelOpenEvent {
    pub id: Id,
    pub connection: RawConnection,
    pub channel: RawChannel,
}


#[derive(Clone, Debug)]
pub enum TransportEvent {
    Connect(WrtcContact),
    Disconnect(Id, DisconnectReason),
    ChannelOpen(ChannelOpenEvent),
    Shutdown,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum DisconnectReason {
    ConnectionLost,
    HalfCloseReplace,// Connection was half closen and we needed space to open new connections
    HalfCloseBoth,
    BadBehavior,
    TimeoutExpired,
    SendFail,
    ProtocolVersionMismatch,// TODO: implement some kind of protocol version matching
    ShuttingDown,// DHT is shutting down
}

impl fmt::Display for DisconnectReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use DisconnectReason::*;
        let str = match self {
            ConnectionLost => "connection lost",
            HalfCloseReplace => "half closen replaced",
            HalfCloseBoth => "both half closen",
            BadBehavior => "bad behavior",
            TimeoutExpired => "timeout expired",
            SendFail => "message sending failed",
            ProtocolVersionMismatch => "protocol version mismatch",
            ShuttingDown => "DHT is shutting down",
        };
        f.write_str(str)
    }
}

// TODO: find (if possible) a way to expose this to any async_broadcast::Receiver<TransportEvent> as an extension trait
pub async fn wait_for_event(listener: &mut async_broadcast::Receiver<TransportEvent>, mut predicate: impl FnMut(Result<TransportEvent, RecvError>) -> bool) {
    loop {
        let ev = listener.recv().await;
        let is_closed = matches!(ev, Err(RecvError::Closed));

        if predicate(ev) {
            break;
        }
        if is_closed {
            panic!("Event source closed!");
        }
    }
}

pub async fn wait_for_shutdown(listener: &mut async_broadcast::Receiver<TransportEvent>) {
    wait_for_event(listener, |ev| match ev {
        Ok(TransportEvent::Shutdown) |
        Err(RecvError::Closed) => true,
        _ => false,
    }).await;
}
