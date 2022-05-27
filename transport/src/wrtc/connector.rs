use std::{collections::HashMap, sync::{Arc, Mutex}, iter};

use datachannel::SessionDescription;
use futures::future::join_all;
use log::{error, warn};
use tokio::sync::{broadcast, oneshot};
use wdht_logic::{Id, transport::TransportError};

use super::{WrtcContact, conn::WrtcConnection, Connections, protocol::{WrtcRequest, WrtcResponse}};

pub type ContactResult = Result<WrtcContact, TransportError>;

struct WrtcChannelCreationData {
    id: Id,
    answer_sender: oneshot::Sender<SessionDescription>,
    contact_recv: oneshot::Receiver<WrtcContact>,
    contact_sender: broadcast::Sender<ContactResult>,
}

#[derive(Default)]
pub struct WrtcConnector {
    // TODO: use a more efficient broadcast channel (ex. crossbeam-channel?)
    connecting: Mutex<HashMap<Id, broadcast::Sender<ContactResult>>>,
}

impl WrtcConnector {
    async fn connect_to(&self, conn: &Arc<Connections>, ids: Vec<(Id, broadcast::Sender<ContactResult>)>, referrer: Arc<WrtcConnection>) {
        // function to call before error-returning, it will kill all intermediary data
        let shutdown = |err: TransportError, data: Vec<WrtcChannelCreationData>| {
            let mut conns = self.connecting.lock().unwrap();
            for x in data {
                conns.remove(&x.id);
                let _ = x.contact_sender.send(Err(err.clone()));
            }
        };

        // Create N active connections
        let offers = join_all(
            (0..ids.len())
                    .map(|_| conn.clone().create_active())
        ).await;

        // map each new offer to an id, this will return
        // Vec<(id, offer)>, Vec<(id, answer_receiver, connection_receiver, connection_sender)>
        // TODO: what if the offer creation call fails?
        let (offers, middle_data): (Vec<_>, Vec<_>) =
            ids.into_iter()
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
            .map(|((id, contact_sender), (descriptor, answer_sender, contact_recv))| (
                (id, descriptor),
                WrtcChannelCreationData {
                    id,
                    answer_sender,
                    contact_recv,
                    contact_sender,
                }
            ))
            .unzip();
        let res = referrer.send_request(WrtcRequest::ForwardOffer(offers)).await;
        let res = match res {
            Ok(x) => x,
            Err(x) => {
                shutdown(x, middle_data);
                return
            }
        };

        let res = match res {
            WrtcResponse::ForwardAnswers(x) => x,
            _ => {
                shutdown("Invalid answer received".into(), middle_data);
                return
            }
        };

        // Map back the received answers and wait for the channel creation to complete
        join_all(
        // Zip our temporary data with the returned answers
        middle_data.into_iter()
            .zip(
                res.into_iter()
                    .map(|x| Some(x))
                    .chain(iter::repeat_with(|| None))
            )
            .filter_map(|(data, ans)| {
                let ans = match ans {
                    Some(Ok(x )) => x,
                    Some(Err(x)) => {
                        self.connecting.lock().unwrap().remove(&data.id);
                        let _ = data.contact_sender.send(Err(format!("Client responded with error: {}", x).into()));
                        return None;
                    }
                    None => {
                        self.connecting.lock().unwrap().remove(&data.id);
                        let _ = data.contact_sender.send(Err("Client returned no forwarded response for id".into()));
                        return None;
                    }
                };
                // Send the answer back to the receiver and return the contact future
                let _ = data.answer_sender.send(ans);
                Some(async move {
                    let recv = match data.contact_recv.await {
                        Ok(x) => Ok(x),
                        Err(_) => Err("Connection failed, sender dropped".into()),
                    };
                    self.connecting.lock().unwrap().remove(&data.id);
                    if data.contact_sender.send(recv).is_err() {
                        warn!("Failed to send back connection answer");
                    }
                })
            })
        ).await;
    }

    pub async fn connect_all(&self, conn: &Arc<Connections>, referrer: Arc<WrtcConnection>, ids: Vec<Id>) -> Vec<ContactResult> {
        let mut to_send = Vec::<(Id, broadcast::Sender<ContactResult>)>::new();
        let contacts = {
            let mut connecting = self.connecting.lock().unwrap();

            join_all(ids.iter()
                .map(|x| {
                    let sender = connecting.entry(*x).or_insert_with(|| {
                        let (sender, _receiver) = broadcast::channel(1);
                        to_send.push((*x, sender.clone()));
                        sender
                    });
                    let mut recv = sender.subscribe();
                    async move {
                        recv.recv().await.expect("Error receiving contact")
                    }
                }))
        };

        if !to_send.is_empty() {
            self.connect_to(conn, to_send, referrer).await;
        }

        contacts.await
    }
}
