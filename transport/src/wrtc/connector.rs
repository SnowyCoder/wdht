use std::{
    collections::{hash_map::Entry, HashMap},
    iter,
    sync::Mutex,
};

use async_broadcast as broadcast;
use futures::future::join_all;
use tokio::sync::oneshot;
use tracing::{error, event, Level};
use wdht_logic::{transport::TransportError, Id};
use wdht_wrtc::SessionDescription;

use super::{
    conn::WrtcConnection,
    protocol::{WrtcRequest, WrtcResponse},
    wasync::Orc,
    Connections, WrtcContact, WrtcTransportError,
};

pub type ContactResult = Result<WrtcContact, TransportError>;

#[derive(Clone)]
pub struct CreatingConnectionSender {
    peer_id: Option<Id>,
    id: usize,
    message_sent: bool,
    channel: broadcast::Sender<ContactResult>,
    owner: Option<Orc<WrtcConnector>>,
}

impl CreatingConnectionSender {
    pub fn send(mut self, result: ContactResult) -> bool {
        self.send0(result)
        // drop!
    }

    pub fn is_last(&self) -> bool {
        match (self.owner.as_ref(), self.peer_id.as_ref()) {
            (Some(parent), Some(id)) => parent
                .inner
                .lock()
                .unwrap()
                .connecting
                .get(id)
                .map_or(false, |x| x.1 == self.id),
            _ => true,
        }
    }

    fn send0(&mut self, result: ContactResult) -> bool {
        self.message_sent = true;
        if let (Some(owner), Some(id)) = (self.owner.take(), self.peer_id.take()) {
            // Only send message if this is the last sender created!
            let mut inner = owner.inner.lock().unwrap();
            if inner.connecting.get(&id).map_or(false, |x| x.1 == self.id) {
                let _ = self.channel.try_broadcast(result);
                inner.connecting.remove(&id);
                true
            } else {
                false
            }
        } else {
            let _ = self.channel.try_broadcast(result);
            false
        }
    }
}

impl Drop for CreatingConnectionSender {
    fn drop(&mut self) {
        if !self.message_sent {
            self.send0(Err("creator dropped".into()));
        }
    }
}

type WrtcChannelCreationData = oneshot::Sender<Result<SessionDescription, WrtcTransportError>>;

#[derive(Default)]
struct WrtcConnectorInner {
    connecting: HashMap<Id, (broadcast::Receiver<ContactResult>, usize)>,
    sender_id: usize,
}

impl WrtcConnectorInner {
    pub fn create_active(
        &mut self,
        parent: &Orc<WrtcConnector>,
        id: Id,
    ) -> (
        Option<CreatingConnectionSender>,
        broadcast::Receiver<ContactResult>,
    ) {
        if let Some(x) = self.connecting.get(&id) {
            return (None, x.clone().0);
        }
        self.create_passive(parent, id)
    }

    pub fn create_passive(
        &mut self,
        parent: &Orc<WrtcConnector>,
        id: Id,
    ) -> (
        Option<CreatingConnectionSender>,
        broadcast::Receiver<ContactResult>,
    ) {
        let entry = self.connecting.entry(id);
        match entry {
            Entry::Occupied(mut entry) => {
                // Rule: only the one with the lowest connection can open!
                // We already have an active connection present, and we're creating a passive connection
                // Only use the passive connection if other_id < self_id
                let sender = if id < parent.dht_id {
                    // Use this connection
                    let sender_id = self.sender_id;
                    self.sender_id += 1;

                    entry.get_mut().1 = sender_id;
                    let sender = CreatingConnectionSender {
                        peer_id: Some(id),
                        id: sender_id,
                        message_sent: false,
                        channel: entry.get().0.new_sender(),
                        owner: Some(parent.clone()),
                    };
                    Some(sender)
                } else {
                    event!(Level::INFO, kad_id=%parent.dht_id, %id, "Dropping passive connection to prevent conflict");
                    None
                };
                (sender, entry.get().0.new_receiver())
            }
            Entry::Vacant(entry) => {
                let sender_id = self.sender_id;
                self.sender_id += 1;

                let (sender, receiver) = broadcast::broadcast(1);
                let sender = CreatingConnectionSender {
                    peer_id: Some(id),
                    id: sender_id,
                    message_sent: false,
                    channel: sender,
                    owner: Some(parent.clone()),
                };
                entry.insert((receiver.clone(), sender_id));
                (Some(sender), receiver)
            }
        }
    }
}

pub struct WrtcConnector {
    dht_id: Id,
    inner: Mutex<WrtcConnectorInner>,
}

impl WrtcConnector {
    pub fn new(id: Id) -> Self {
        WrtcConnector {
            dht_id: id,
            inner: Default::default(),
        }
    }

    async fn connect_to(
        &self,
        conn: &Orc<Connections>,
        ids: Vec<(Id, CreatingConnectionSender)>,
        referrer: Orc<WrtcConnection>,
    ) {
        // function to call before error-returning, it will kill all intermediary data
        let shutdown = |err: WrtcTransportError, data: Vec<WrtcChannelCreationData>| {
            for x in data {
                let _ = x.send(Err(err.clone()));
            }
        };

        // Create N active connections
        let offers = join_all(ids.into_iter().map(|(id, connector)| async move {
            conn.clone()
                .create_active_with_connector(connector)
                .await
                .map(|(desc, sender)| (id, desc, sender))
        }))
        .await;

        // map each new offer to an id, this will return
        // Vec<(id, offer)>, Vec<(id, answer_receiver, connection_receiver, connection_sender)>
        // TODO: what if the offer creation call fails?
        let (offers, middle_data): (Vec<_>, Vec<_>) = offers
            .into_iter()
            .filter_map(|x| match x {
                Err(e) => {
                    error!("Failed to create offer: {}", e);
                    None
                }
                Ok(x) => Some(x),
            })
            .map(|(id, descriptor, answer_sender)| ((id, descriptor), answer_sender))
            .unzip();
        let res = referrer
            .send_request(WrtcRequest::ForwardOffer(offers))
            .await;
        let res = match res {
            Ok(x) => x,
            Err(x) => {
                shutdown(WrtcTransportError::Transport(x), middle_data);
                return;
            }
        };

        let res = match res {
            WrtcResponse::ForwardAnswers(x) => x,
            _ => {
                shutdown(WrtcTransportError::InvalidMessage, middle_data);
                return;
            }
        };

        // Map back the received answers and wait for the channel creation to complete
        // Zip our temporary data with the returned answers
        middle_data
            .into_iter()
            .zip(res.into_iter().map(Some).chain(iter::repeat_with(|| None)))
            .for_each(|(answer_sender, ans)| {
                let ans = match ans {
                    Some(Ok(x)) => x,
                    Some(Err(x)) => {
                        let _ =
                            answer_sender
                                .send(Err(format!("Client responded with error: {}", x).into()));
                        return;
                    }
                    None => {
                        let _ = answer_sender
                            .send(Err("Client returned no forwarded response for id".into()));
                        return;
                    }
                };
                // Send the answer back to the receiver and return the contact future
                let _ = answer_sender.send(Ok(ans));
            });
    }

    pub async fn connect_all(
        self: &Orc<Self>,
        conn: &Orc<Connections>,
        referrer: Orc<WrtcConnection>,
        ids: Vec<Id>,
    ) -> Vec<ContactResult> {
        let mut to_send = Vec::<(Id, CreatingConnectionSender)>::new();
        let contacts = {
            let mut inner = self.inner.lock().unwrap();

            join_all(ids.iter().map(|id| {
                let (sender, mut recv) = inner.create_active(self, *id);
                if let Some(sender) = sender {
                    to_send.push((*id, sender));
                }

                async move { recv.recv().await.expect("Error receiving contact") }
            }))
        };

        if !to_send.is_empty() {
            self.connect_to(conn, to_send, referrer).await;
        }

        contacts.await
    }

    pub fn is_connecting(&self, id: Id) -> bool {
        self.inner.lock().unwrap().connecting.contains_key(&id)
    }

    pub fn create_unknown(
        self: &Orc<Self>,
    ) -> (CreatingConnectionSender, broadcast::Receiver<ContactResult>) {
        let (sender, receiver) = broadcast::broadcast(1);
        (
            CreatingConnectionSender {
                peer_id: None,
                id: 0,
                message_sent: false,
                channel: sender,
                owner: None,
            },
            receiver,
        )
    }

    pub fn create_active(
        self: &Orc<Self>,
        id: Id,
    ) -> (
        Option<CreatingConnectionSender>,
        broadcast::Receiver<ContactResult>,
    ) {
        let mut conns = self.inner.lock().unwrap();
        conns.create_active(self, id)
    }

    pub fn create_passive(
        self: &Orc<Self>,
        id: Id,
    ) -> (
        Option<CreatingConnectionSender>,
        broadcast::Receiver<ContactResult>,
    ) {
        let mut conns = self.inner.lock().unwrap();
        conns.create_passive(self, id)
    }
}
