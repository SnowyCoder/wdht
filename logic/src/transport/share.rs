use core::fmt;
use std::sync::Arc;

use crate::Id;

use super::Contact;

pub enum RequestPacket {
    FindNodes(Id),
    FindValue(Id),
    // A is trying to connect to X nodes trough B
    // then A asks B to forward each offer to them.
    WrtcForward(Vec<(Id, Vec<u8>)>),
    // Hello, client Id is trying to connect to you with offer Vec<u8>
    WrtcTryOffer(Id, Vec<u8>),
}


#[derive(Clone)]
pub enum SharedContact {
    Permanent(Id),
    Temporary(Arc<dyn SmartContactPointer>),
}

impl fmt::Debug for SharedContact {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("SharedContact")
            .field(self.id())
            .finish()
    }
}

impl Contact for SharedContact {
    fn id(&self) -> &Id {
        match self {
            SharedContact::Permanent(x) => x,
            SharedContact::Temporary(x) => x.id(),
        }
    }
}

pub trait SmartContactPointer {
    fn id(&self) -> &Id;
}

pub struct SharedTransportSender {

}
