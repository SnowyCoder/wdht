use std::{future::Future, fmt::Debug, borrow::Cow};

#[cfg(feature = "serde")]
use serde::{Serialize, Deserialize};
use thiserror::Error;

use crate::id::Id;

/// An interface to deal with Transport-held contacts
///
/// This needs to be used for connection recycling.
/// When someone uses a connection outside of DHT's routing
/// table they need to say to the Transport when that connection
/// is still needed, the Transport might do some refcounting using this
/// trait.
///
/// Simpler transport (ex. simulated ones) might not need any additional
/// tracking data at all, they might then use Id as a Contact directly.
///
/// When using connections outside of DHT routing tables functions should
/// hold on to the Transport's Contact, if only the Id is kept and the
/// Contact is lost a [`TransportError::ContactLost`] might be thrown when
/// trying to contact said id.
pub trait Contact : Clone + Debug {
    fn id(&self) -> &Id;
}

impl Contact for Id {
    fn id(&self) -> &Id {
        self
    }
}

/// Object able to send messages to an id
// Should use some sort of interior mutability and Refcounting
// You must be able to send a Transport copy between boundaries! (Send)
pub trait TransportSender : Clone + Send {
    /// Tries to ping an Id to check connection liveliness.
    ///
    /// This is not implemented as a normal message since we
    /// need to support multiple protocols, some of which
    /// might already have connection liveliness support.
    ///
    /// If a ping fails the connection is broken and the host
    /// will be disconnected.
    fn ping(&self, id: &Id);

    /// Future returned when sending a message to another peer
    type Fut: Future<Output=Result<RawResponse<Self::Contact>, TransportError>> + Send;

    /// Sends a message to a peer and waits for the response
    ///
    /// id needs to have a contact in the transport level,
    /// if it's registered in the DHT routing table then it's live,
    /// otherwise a Self::Contact needs to be kept in scope,
    /// if not (or if trying to call an unknown Id)
    /// a [`TransportError::ConnectionLost`] error might be thrown
    fn send(&self, id: &Id, msg: Request) -> Self::Fut;

    /// Wraps an Id in a Contact
    ///
    /// The passed Id must be used in the DHT's routing table,
    /// otherwise the wrapping might fail with a panic.
    ///
    /// Unwrapping a contact, dropping it and trying to rewrap it
    /// is a perfect recipe for a panic since the connection will
    /// probably be dropped at the transport level.
    fn wrap_contact(&self, id: Id) -> Self::Contact;

    /// The type of the smart pointer used by this transport
    type Contact: Contact;
}

pub trait TransportListener {
    /// Returns true only if the id is used in the routing protocol
    fn on_connect(&self, id: Id) -> bool;

    fn on_disconnect(&self, id: Id);

    fn on_request(&self, sender: Id, request: Request) -> Response;
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Request {
    FindNodes(Id),
    FindData(Id),
    // id, seconds, data
    Insert(Id, u32, Vec<u8>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum RawResponse<T> {
    FoundNodes(Vec<T>),
    FoundData(Vec<u8>),
    Done,// Generic response (ex: response to Insert)
    Error,// Generic bad response (should never be thrown with a correct client)
}

pub type Response = RawResponse<Id>;

#[derive(Clone, Debug, Error)]
#[non_exhaustive]
pub enum TransportError {
    #[error("Client connection lost")]
    ConnectionLost,

    #[error("Cannot find client address")]
    ContactLost,

    #[error("Unknown transport error {0}")]
    UnknownError(Cow<'static, str>),
}

impl From<&'static str> for TransportError {
    fn from(x: &'static str) -> Self {
        TransportError::UnknownError(Cow::Borrowed(x))
    }
}

impl From<String> for TransportError {
    fn from(x: String) -> Self {
        TransportError::UnknownError(Cow::Owned(x))
    }
}
