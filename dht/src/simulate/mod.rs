use std::{sync::{Arc, Mutex}, collections::{HashMap, HashSet}, fmt::{Write, Display, self}};

use futures::Future;
use log::{debug, trace};
use tokio::sync::{mpsc, oneshot, broadcast, Barrier};

use crate::{contacter::{Transport, Response, Request, TransportError}, Id, KademliaDht, config::SystemConfig};


#[derive(Debug)]
struct TransportResponse {
    payload: Response,
    // When sending Ids also send their location
    contacts: Vec<mpsc::Sender<TransportMessage>>,
}

#[derive(Debug)]
enum TransportMessage {
    Hello {
        id: Id,
        mex: mpsc::Sender<TransportMessage>,
    },
    Request {
        id: Id,
        msg: Request,
        res: oneshot::Sender<TransportResponse>,
    },
    ConnectTo {
        // Sent from transport to the actor when a new node is contacted
        ids: Vec<(Id, mpsc::Sender<TransportMessage>)>,
        res: oneshot::Sender<()>,
    },
    // Used in testing
    Barrier(Arc<Barrier>),
}

// TODO: you can't keep all contacts,
//      We should have a maximum pool of contacts (N = max contact number)
//      some contacts should be available for the routing table
//      (R = max routing table contact n., with R < N)
//      the others can be used for querying, (FILO queue?)
//      There should also be some kind of reference counting for connections
//      When a connection isn't used anymore it can be dropped
// But when is a connection not used anymore?
//      A connection can be used for: routing or searching
//      A routing connection is never deallocated unless it's lost
//      Just needs to manage searching connections? (how??)

pub struct Actor {
    dht: Arc<KademliaDht<SimulatedTransport>>,
    mailbox: mpsc::Receiver<TransportMessage>,
    shutdown: broadcast::Receiver<()>,
}

impl Actor {
    pub fn spawn(shutdown: broadcast::Receiver<()>, id: Id, config: SystemConfig) -> Arc<KademliaDht<SimulatedTransport>> {
        let (tx, rx) = mpsc::channel(128);
        let transport = SimulatedTransport {
            id: id.clone(),
            data: Arc::new(Mutex::new(TransportData {
                contacts: HashMap::new(),
            })),
            actor: tx,
        };
        let dht = Arc::new(KademliaDht::new(config, id, transport));
        let actor = Self {
            dht: dht.clone(),
            mailbox: rx,
            shutdown,
        };
        tokio::spawn(actor.run());
        dht
    }

    async fn run(mut self) {
        loop {
            let mail = tokio::select! {
                x = self.mailbox.recv() => x,
                _ = self.shutdown.recv() => break,
            };
            //eprintln!("{:?} MAIL -> {:?}", self.dht.id(), mail);
            let mail: TransportMessage = match mail {
                Some(x) => x,
                None => break,
            };
            use TransportMessage::*;
            match mail {
                Hello { id, mex } => {
                    self.dht.on_connect(&id);
                    self.dht.transport.data.lock().unwrap().contacts.insert(id, mex);
                }
                Request { id, msg, res: wait } => {
                    let res = self.dht.on_request(&id, msg);
                    let contacts = match &res {
                        Response::FoundNodes(ids) => {
                            // We're sending node ids, also send contact data!
                            // (in a WebRTC-like implementation this would be a tad more complex)
                            let routes = self.dht.transport.data.lock().unwrap();
                            ids.iter()
                                .map(|x| routes.contacts.get(x).unwrap().clone())
                                .collect()
                        }
                        _ => Vec::new(),
                    };
                    let res = TransportResponse {
                        payload: res,
                        contacts,
                    };
                    wait.send(res).expect("Cannot send response back");
                }
                ConnectTo { ids, res } => {
                    for (id, mailbox) in ids.iter() {
                        {
                            let mut transport = self.dht.transport.data.lock().unwrap();
                            if transport.contacts.contains_key(id) {
                                // Prevent double-join (possible when two connections discover)
                                // the same address concurrently.
                                continue;
                            }
                            transport.contacts
                                .insert(id.clone(), mailbox.clone());
                        }

                        self.dht.on_connect(id);
                        mailbox
                            .send(TransportMessage::Hello {
                                id: self.dht.id().clone(),
                                // Own mailbox
                                mex: self.dht.transport.actor.clone(),
                            })
                            .await
                            .expect("Failed to send hello");
                    }
                    res.send(()).unwrap();
                }
                Barrier(b) => {
                    b.wait().await;
                }
            }
        }
    }
}

struct TransportData {
    contacts: HashMap<Id, mpsc::Sender<TransportMessage>>,
}

#[derive(Clone)]
pub struct SimulatedTransport {
    id: Id,
    data: Arc<Mutex<TransportData>>,
    actor: mpsc::Sender<TransportMessage>,
}

impl SimulatedTransport {
    async fn send_req(self, id: Id, msg: Request) -> Result<Response, TransportError> {
        trace!("send_req({:?} to {:?}, {:?}", self.id, id, msg);
        let sender = {
            let data = self.data.lock().unwrap();
            data.contacts.get(&id)
                .expect("Cannot find contact")
                .clone()
        };
        let (tx, rx) = oneshot::channel();

        sender.send(TransportMessage::Request {
            id: self.id.clone(),
            msg: msg.clone(),
            res: tx,
        }).await.expect("Failed to send request");
        let TransportResponse {
            payload,
            contacts
        } = rx.await.expect("Error receiving response");


        debug!("{:?} -> {:?} = {:?}? {:?}", self.id, id, msg, payload);

        if let Response::FoundNodes(found) = &payload  {
            let ids: Vec<_> = {
                let data = self.data.lock().unwrap();
                found.iter()
                    .zip(contacts.into_iter())
                    .filter(|(id, _)| !data.contacts.contains_key(id))
                    .map(|(id, mail)| (id.clone(), mail))
                    .collect()
            };
            if ids.len() > 0 {
                let (tx, rx) = oneshot::channel();
                self.actor.send(TransportMessage::ConnectTo {
                    ids,
                    res: tx,
                }).await.unwrap();
                rx.await.unwrap();
            }
        }
        Ok(payload)
    }

    pub async fn connect_to(&self, ids: Vec<(&Id, &KademliaDht<SimulatedTransport>)>) {
        let (tx, rx) = oneshot::channel();

        let ids = ids.iter()
            .map(|(id, dht)| ((*id).clone(), dht.transport().actor.clone()))
            .collect();
        self.actor.send(TransportMessage::ConnectTo {
            ids,
            res: tx,
        }).await.unwrap();
        rx.await.unwrap();
    }

    pub async fn barrier_sync(&self, barrier: Arc<Barrier>) {
        self.actor.send(TransportMessage::Barrier(barrier)).await.unwrap();
    }
}

impl Transport for SimulatedTransport {
    fn ping(&self, _id: &Id) {
        // Yes, we don't simulate node failure yet
    }

    type Fut = impl Future<Output=Result<Response, TransportError>>;
    fn send(&self, id: &Id, msg: Request) -> Self::Fut {
        let s = self.clone();
        s.send_req(id.clone(), msg)
    }
}

pub trait IntoDot {
    fn to_dot_string(self) -> String;
}

impl<'a, T> IntoDot for T where T: Iterator<Item = &'a SimulatedTransport> {
    fn to_dot_string(mut self) -> String {
        let mut res = String::new();
        let mut visited = HashSet::new();
        res.push_str("graph {\n");

        while let Some(t) = self.next() {
            let id = &t.id;
            visited.insert(id.clone());
            let data = t.data.lock().unwrap();
            for (c, _) in data.contacts.iter() {
                if visited.contains(c) {
                    continue;
                }
                write!(&mut res, "\"{}\" -- \"{}\";\n", id.as_short_hex(), c.as_short_hex()).unwrap();
            }
        }
        res.push_str("}\n");

        res
    }
}


#[cfg(test)]
mod tests {
    use log::info;

    use crate::search::BasicSearchOptions;

    use super::*;

    fn init() {
        let _ = env_logger::builder()
            .is_test(true)
            .try_init();
    }

    #[tokio::test]
    async fn simulate_simple() {
        init();

        let (killswitch, shutdown) = broadcast::channel(1);

        let config: SystemConfig = Default::default();

        // Create 2 DHTs (a and b)
        let aid = Id::from_hex("aa");
        let a = Actor::spawn(killswitch.subscribe(), aid.clone(), config.clone());

        let bid = Id::from_hex("ba");

        let b = Actor::spawn(shutdown, bid.clone(), config);

        // Connect b to a (and vice-versa)
        b.transport().connect_to(vec![(&aid, &a)]).await;

        // Barrier: allow processing of joins
        let barr = Arc::new(Barrier::new(3));
        a.transport().barrier_sync(barr.clone()).await;
        b.transport().barrier_sync(barr.clone()).await;
        barr.wait().await;

        // Ask a to find b
        // The network only has a and b, so there aren't many possible nodes
        // a will ask b for any other nodes, but there won't be any, so the search
        // will terminate with [b]
        let res = a.query_nodes(bid.clone(), BasicSearchOptions {
            parallelism: 1,
        }).await;
        assert_eq!(res, vec![bid.clone()]);

        // Shutdown everything
        killswitch.send(()).unwrap();
    }



    #[tokio::test]
    async fn simulate_10() {
        init();

        let (killswitch, _shutdown) = broadcast::channel(1);

        let config: SystemConfig = Default::default();
        let search_options = BasicSearchOptions {
            parallelism: 2,
        };

        let ids = [
            "aaaaaaaa",
            "aaaabbbb",
            "aaaa0000",
            "aaaa4444",
            "4444aaaa",
            "44441234",
            "cafebabe",
            "89abcdef",
            "12345678",
            "31415fab",
        ].into_iter()
            .map(|x| Id::from_hex(x))
            .collect::<Vec<_>>();

        let dhts = ids.iter()
            .cloned()
            .map(|id| Actor::spawn(killswitch.subscribe(), id, config.clone()))
            .collect::<Vec<_>>();

        // "aaaaaaaa" is the rendevouz DHT (a.k.a. bootstrap dht)
        for i in 1..ids.len() {
            info!("----- NODE {:?} ----", ids[i]);
            dhts[i].transport().connect_to(vec![(&ids[0], &dhts[0])]).await;
            // Bootstrap
            dhts[i].query_nodes(ids[i].clone(), search_options.clone()).await;
        }

        // Uncomment to write dot graph file (for visualization)
        /*File::create("sim10.dot").unwrap().write_all(
            dhts.iter()
                .map(|x| x.transport())
                .to_dot_string()
                .as_bytes()
        ).unwrap();*/

        killswitch.send(()).unwrap();
    }
}
