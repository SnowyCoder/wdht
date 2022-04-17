use std::{future::Future, sync::Arc, ops::Deref};

use thiserror::Error;

use crate::id::Id;

/// Object able to send messages to an id
// Should use some sort of interior mutability and Refcounting
// You must be able to send a Transport copy between boundaries! (Send)
pub trait TransportSender : Clone + Send {
    // some protocol have implicit keepalive pings,
    // let them handle it properly
    fn ping(&self, id: &Id);

    // TODO:
    // we could also use some actix-like traits to restrict down the possible answers?
    // other option: use some lower-level API (to gain performance)
    type Fut: Future<Output=Result<Response, TransportError>> + Send;
    fn send(&self, id: &Id, msg: Request) -> Self::Fut;
}

pub trait TransportListener {
    /// Returns true only if the id is used in the routing protocol
    fn on_connect(&self, id: &Id) -> bool;

    fn on_disconnect(&self, id: &Id);

    fn on_request(&self, sender: &Id, request: Request) -> Response;
}

impl<T: TransportListener> TransportListener for Arc<T> {
    fn on_connect(&self, id: &Id) -> bool {
        self.deref().on_connect(id)
    }

    fn on_disconnect(&self, id: &Id) {
        self.deref().on_disconnect(id)
    }

    fn on_request(&self, sender: &Id, request: Request) -> Response {
        self.deref().on_request(sender, request)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    FindNodes(Id),
    FindData(Id),
    // id, seconds, data
    Insert(Id, u32, Vec<u8>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Response {
    FoundNodes(Vec<Id>),
    FoundData(Vec<u8>),
    Done,// Generic response (ex: response to Insert)
    Error,// Generic bad response (should never be thrown with a correct client)
}

#[derive(Clone, Debug, Error)]
#[non_exhaustive]
pub enum TransportError {
    #[error("Client connection lost")]
    ConnectionLost,
}
