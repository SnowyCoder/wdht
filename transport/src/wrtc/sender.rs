use core::future::Future;
use std::{sync::Arc, fmt::{Debug, Formatter}};

use futures::future::join_all;
use log::{error, warn};
use wdht_logic::{transport::{TransportSender, Request, RawResponse, TransportError, Contact}, Id};

use super::{Connections, protocol::{WrtcRequest, WrtcResponse}, conn::WrtcConnection};


async fn resolve_nodes(contact: Arc<WrtcConnection>, conn: Arc<Connections>, ids: Vec<Id>) -> Result<Vec<WrtcContact>, TransportError> {
    // Collect old_contacts (contacts already known)
    // and contacts to query
    // old_contacts is a Vec<Option<WrtcContact>>, None elements are placeholders for
    // Contacts that will be queried
    let mut to_query = Vec::new();
    let old_contacts = {
        let connections = conn.connection.lock().unwrap();
        ids.into_iter().map(|id| {
            let x = connections.get(&id);
            if let None = x {
                to_query.push(id)
            }
            x.cloned().map(|x| WrtcContact(x))
        }).collect::<Vec<_>>()
    };

    if to_query.is_empty() {
        // No new node needs to be queried, we know them all!
        return Ok(
            old_contacts.into_iter()
                .map(|x| x.unwrap())
                .collect()
        );
    }

    // Create N active connections
    let offers = join_all(
        (0..to_query.len())
                .map(|_| conn.clone().create_active())
    ).await;

    // map each new offer to an id, this will return
    // Vec<(id, offer)>, Vec<(answer_receiver, connection_receiver)>
    let offers: (Vec<_>, Vec<_>) =
        to_query.iter()
        .zip(
            offers.into_iter()
                .filter_map(|x| {
                    match x {
                        Err(e) => {
                            error!("Failed to create offer: {}", e);
                            None
                        }
                        Ok(x) => Some(x),
                    }
                })
        )
        .map(|(id, x)| ((*id, x.0), (x.1, x.2)))
        .unzip();
    let res = contact.send_request(WrtcRequest::ForwardOffer(offers.0)).await?;

    let res = match res {
        WrtcResponse::ForwardAnswers(x) => x,
        _ => return Err(TransportError::UnknownError("Invalid answer received".into()))
    };

    // Map back the received answers and wait for the channel creation to complete
    let new_contacts: Vec<WrtcContact> = join_all(
        // Zip our Vec<*answer_receiver, contact_receiver)> with the returned answers
        offers.1.into_iter()
            .zip(res)
            .filter_map(|(recv, ans)| {
                let ans = match ans {
                    Ok(x ) => x,
                    Err(x) => {
                        warn!("Client responded with error: {}", x);
                        return None;
                    }
                };
                // Send the answer back to the receiver and return the contact future
                let _ = recv.0.send(ans);
                Some(recv.1)
            })
        ).await
        .into_iter()
        .filter_map(|x| match x {
            Ok(x) => Some(x),
            Err(e) => {
                warn!("Signaling error {}", e);
                None
            }
        })
        .map(|x| WrtcContact(x))
        .collect();

    // Piece back together old contacts and new contacts
    let mut new_contacts = new_contacts.into_iter();

    let res = old_contacts.into_iter()
        .filter_map(|x| {
            match x {
                Some(x) => Some(x),
                None => new_contacts.next(),
            }
        })
        .collect::<Vec<_>>();

    Ok(res)
}

async fn translate_response(contact: Arc<WrtcConnection>, conn: Arc<Connections>, res: RawResponse<Id>) -> Result<RawResponse<WrtcContact>, TransportError> {
    use RawResponse::*;
    Ok(match res {
        FoundNodes(nodes) => FoundNodes(resolve_nodes(contact, conn, nodes).await?),
        FoundData(x) => FoundData(x),
        Done => Done,
        Error => Error,
    })
}


#[derive(Clone)]
pub struct WrtcSender(pub(crate) Arc<Connections>);

impl TransportSender for WrtcSender {
    fn ping(&self, _id: &Id) {
        // WebRTC automatically manages disconnections
    }

    type Fut = impl Future<Output=Result<RawResponse<Self::Contact>, TransportError>>;

    fn send(&self, id: &Id, msg: Request) -> Self::Fut {
        let root = self.0.clone();
        let id = *id;
        async move {
            let contact = root.connection.lock().unwrap().get(&id)
                .ok_or(TransportError::ContactLost)?
                .clone();

            let res = contact.send_request(WrtcRequest::Req(msg)).await;

            match res {
                Ok(WrtcResponse::Ans(x)) => translate_response(contact, root, x).await,
                Ok(_) => Err(TransportError::UnknownError("Invalid response".into())),
                Err(x) => Err(TransportError::UnknownError(x.to_string())),
            }
        }
    }

    fn wrap_contact(&self, id: Id) -> Self::Contact {
        WrtcContact(self.0.connection.lock().unwrap().get(&id)
            .expect("Cannot find contact")
            .clone())
    }

    type Contact = WrtcContact;
}


#[derive(Clone)]
pub struct WrtcContact(Arc<WrtcConnection>);

impl Contact for WrtcContact {
    fn id(&self) -> &Id {
        &self.0.peer_id
    }
}

impl Debug for WrtcContact {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("WrtcContact").field(self.id()).finish()
    }
}
