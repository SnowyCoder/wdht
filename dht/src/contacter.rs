use std::future::Future;

use thiserror::Error;

use crate::id::Id;

// Should use some sort of interior mutability and Refcounting
// You must be able to send a Transport copy between boundaries! (Send)
pub trait Transport : Clone + Send {
    // some protocol have implicit keepalive pings,
    // let them handle it properly
    fn ping(&self, id: &Id);

    // TODO:
    // we could also use some actix-like traits to restrict down the possible answers?
    // other option: use some lower-level API (to gain performance)
    type Fut: Future<Output=Result<Response, TransportError>>;
    fn send(&self, id: &Id, msg: Request) -> Self::Fut;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestId(pub u32);

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