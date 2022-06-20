use core::future::Future;
use std::fmt::{Debug, Formatter};
use tracing::warn;
use wdht_logic::{
    transport::{Contact, RawResponse, Request, TransportError, TransportSender},
    Id,
};
use wdht_wrtc::RawConnection;

use super::{
    conn::WrtcConnection,
    protocol::{WrtcRequest, WrtcResponse},
    wasync::Orc,
    Connections,
};

async fn resolve_nodes(
    referrer: Orc<WrtcConnection>,
    conn: &Orc<Connections>,
    ids: Vec<Id>,
) -> Result<Vec<WrtcContact>, TransportError> {
    // Collect old_contacts (contacts already known)
    // and contacts to query
    // old_contacts is a Vec<Option<WrtcContact>>, None elements are placeholders for
    // Contacts that will be queried
    let mut to_query = Vec::new();
    let old_contacts = {
        let connections = conn.connections.lock().unwrap();
        ids.into_iter()
            .map(|id| {
                let x = connections.get(&id);
                if x.is_none() {
                    to_query.push(id)
                }
                x.cloned().map(WrtcContact::Other)
            })
            .collect::<Vec<_>>()
    };

    if to_query.is_empty() {
        // No new node needs to be queried, we know them all!
        return Ok(old_contacts.into_iter().map(|x| x.unwrap()).collect());
    }

    let new_contacts = conn.connector.connect_all(conn, referrer, to_query).await;

    // Piece back together old contacts and new contacts
    let mut new_contacts = new_contacts.into_iter();

    let res = old_contacts
        .into_iter()
        .filter_map(|x| match x {
            Some(x) => Some(x),
            None => new_contacts.next().and_then(|x| match x {
                Ok(x) => Some(x),
                Err(e) => {
                    warn!("Error connecting: {}", e);
                    None
                }
            }),
        })
        .collect::<Vec<_>>();

    Ok(res)
}

async fn translate_response(
    contact: Orc<WrtcConnection>,
    conn: Orc<Connections>,
    res: RawResponse<Id>,
) -> Result<RawResponse<WrtcContact>, TransportError> {
    use RawResponse::*;
    Ok(match res {
        FoundNodes(nodes) => FoundNodes(resolve_nodes(contact, &conn, nodes).await?),
        FoundData(x) => FoundData(x),
        Done => Done,
        Error => Error,
    })
}

#[derive(Clone)]
pub struct WrtcSender(pub(crate) Orc<Connections>);

impl TransportSender for WrtcSender {
    fn ping(&self, _id: Id) {
        // WebRTC automatically manages disconnections
    }

    type Fut = impl Future<Output = Result<RawResponse<Self::Contact>, TransportError>>;

    fn send(&self, id: Id, msg: Request) -> Self::Fut {
        let root = self.0.clone();
        async move {
            let contact = root
                .connections
                .lock()
                .unwrap()
                .get(&id)
                .ok_or(TransportError::ContactLost)?
                .clone();

            let res = contact.clone().send_request(WrtcRequest::Req(msg)).await;

            match res {
                Ok(WrtcResponse::Ans(x)) => translate_response(contact, root, x).await,
                Ok(_) => Err(TransportError::UnknownError("Invalid response".into())),
                Err(x) => Err(x),
            }
        }
    }

    fn wrap_contact(&self, id: Id) -> Self::Contact {
        if self.0.dht.upgrade().expect("Shutting down").id() == id {
            return WrtcContact::SelfId(id);
        }

        WrtcContact::Other(
            self.0
                .connections
                .lock()
                .unwrap()
                .get(&id)
                .expect("Cannot find contact")
                .clone(),
        )
    }

    type Contact = WrtcContact;
}

#[derive(Clone)]
pub enum WrtcContact {
    SelfId(Id),
    Other(Orc<WrtcConnection>),
}

impl WrtcContact {
    pub fn raw_connection(&self) -> Option<RawConnection> {
        match self {
            WrtcContact::Other(x) => Some(x.raw_connection()),
            _ => None,
        }
    }
}

impl Drop for WrtcContact {
    fn drop(&mut self) {
        let parent = match self {
            WrtcContact::SelfId(_) => return,
            WrtcContact::Other(x) => x,
        };

        // this + connection's
        if Orc::strong_count(parent) != 2 {
            return;
        }

        parent.on_contact_lost();
    }
}

impl Contact for WrtcContact {
    fn id(&self) -> Id {
        match self {
            WrtcContact::SelfId(x) => *x,
            WrtcContact::Other(x) => x.peer_id,
        }
    }
}

impl Debug for WrtcContact {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("WrtcContact").field(&self.id()).finish()
    }
}
