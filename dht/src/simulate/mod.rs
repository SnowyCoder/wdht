use core::fmt;
use std::{sync::{Arc, Mutex, Weak}, collections::{HashMap, HashSet, hash_map::Entry}, fmt::Write};

use futures::Future;
use log::{debug, trace};
use tokio::sync::{mpsc, oneshot, broadcast, Barrier};

use crate::{transport::{TransportSender, Response, Request, TransportError, TransportListener, Contact, RawResponse}, Id, KademliaDht, config::SystemConfig};


#[derive(Debug)]
struct SimulatedResponse {
    payload: Response,
    // When sending Ids also send their location
    contacts: Vec<mpsc::Sender<TransportMessage>>,
}

#[derive(Clone, Debug)]
pub struct IntrospectionData {
    pub connection_count: usize,
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
        res: oneshot::Sender<SimulatedResponse>,
    },
    ConnectTo {
        // Sent from transport to the actor when a new node is contacted
        ids: Vec<(Id, mpsc::Sender<TransportMessage>)>,
        res: oneshot::Sender<Vec<SearchContact>>,
    },
    // Used in testing
    Barrier(Arc<Barrier>),
}

#[derive(Clone, Debug)]
pub enum SearchContact {
    Routed(Id),
    Owned(Arc<Inner>),
}

pub struct Inner {
    id: Id,
    parent: Arc<Mutex<TransportData>>,
}

impl fmt::Debug for Inner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Inner")
            .field("id", &self.id)
            .finish()
    }
}

impl Drop for SearchContact {
    fn drop(&mut self) {
        if let SearchContact::Owned(arc) = self {
            if Arc::strong_count(&arc) == 1 {
                trace!("Dropping contact: {:?}", arc.id);
                let mut data = arc.parent.lock().unwrap();
                data.on_contact_drop(&arc.id);
            }
        }
    }
}

impl Contact for SearchContact {
    fn id(&self) -> &Id {
        match self {
            SearchContact::Routed(x) => x,
            SearchContact::Owned(x) => &x.id,
        }
    }
}

pub struct AsyncSimulatedTransport;

impl AsyncSimulatedTransport {
    pub fn create(id: Id, shutdown: broadcast::Receiver<()>) -> (Sender, Receiver) {
        // Mailbox
        let (tx, rx) = mpsc::channel(128);

        let sender = Sender {
            id: id.clone(),
            data: Arc::new(Mutex::new(TransportData {
                contacts: HashMap::new(),
            })),
            receiver: tx,
        };
        let receiver = Receiver {
            sender: sender.clone(),
            mailbox: rx,
            shutdown,
        };
        (sender, receiver)
    }

    pub fn spawn(config: SystemConfig, id: Id, shutdown: broadcast::Receiver<()>) -> Arc<KademliaDht<Sender>> {
        let (sender, receiver) = Self::create(id.clone(), shutdown);
        let kad = Arc::new(KademliaDht::new(config, id, sender));
        tokio::spawn(receiver.run(kad.clone()));
        kad
    }
}

pub struct Receiver {
    sender: Sender,
    mailbox: mpsc::Receiver<TransportMessage>,
    shutdown: broadcast::Receiver<()>,
}

impl Receiver {
    async fn run<L: TransportListener, R: AsRef<L>>(mut self, listener: R) {
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
                    if listener.as_ref().on_connect(&id) {
                        self.sender.data.lock().unwrap().contacts.insert(id, (mex, ContactLifetime::Routing));
                    }
                }
                Request { id, msg, res: wait } => {
                    let res = listener.as_ref().on_request(&id, msg);
                    let contacts = match &res {
                        Response::FoundNodes(ids) => {
                            // We're sending node ids, also send contact data!
                            // (in a WebRTC-like implementation this would be a tad more complex)
                            let routes = self.sender.data.lock().unwrap();
                            ids.iter()
                                .map(|x| routes.contacts.get(x).unwrap().0.clone())
                                .collect()
                        }
                        _ => Vec::new(),
                    };
                    let res = SimulatedResponse {
                        payload: res,
                        contacts,
                    };
                    // Ignore error, if the other half ignores the response we don't care
                    let _ = wait.send(res);
                }
                ConnectTo { ids, res } => {
                    for (_, mailbox) in ids.iter() {
                        mailbox
                            .send(TransportMessage::Hello {
                                id: self.sender.id.clone(),
                                // Own mailbox
                                mex: self.sender.receiver.clone(),
                            })
                            .await
                            .expect("Failed to send hello");
                    }
                    let contacts = ids.iter()
                        .map(|(id, mailbox)| {
                            let mut transport = self.sender.data.lock().unwrap();
                            if transport.contacts.contains_key(id) {
                                // Prevent double-join (possible when two connections discover)
                                // the same address concurrently.
                                return SearchContact::Routed(id.clone());
                            }

                            let routed = listener.as_ref().on_connect(id);

                            transport.insert(id.clone(), mailbox.clone(), &self.sender.data, routed)
                        })
                        .collect();

                    let _ = res.send(contacts);
                }
                Barrier(b) => {
                    b.wait().await;
                }
            }
        }
    }
}

enum ContactLifetime {
    Routing,// Used for routing, should remain unless transport is lost
    Temporary(Weak<Inner>),// Only used temporarily, can be dropped when not used anymore
}

struct TransportData {
    contacts: HashMap<Id, (mpsc::Sender<TransportMessage>, ContactLifetime)>,
}

impl TransportData {
    fn insert(&mut self, id: Id, mailbox: mpsc::Sender<TransportMessage>, parent: &Arc<Mutex<TransportData>>, routed: bool) -> SearchContact {
        if routed {
            self.insert_routing(id.clone(), mailbox);
            SearchContact::Routed(id)
        } else {
            self.insert_temp(id.clone(), mailbox, parent)
        }
    }

    fn insert_routing(&mut self, id: Id, mailbox: mpsc::Sender<TransportMessage>) {
        self.contacts.insert(id, (mailbox, ContactLifetime::Routing));
    }

    fn insert_temp(&mut self, id: Id, mailbox: mpsc::Sender<TransportMessage>, parent: &Arc<Mutex<TransportData>>) -> SearchContact {
        match self.contacts.entry(id.clone()) {
            Entry::Occupied(mut x) => {
                match &mut x.get_mut().1 {
                    ContactLifetime::Routing => SearchContact::Routed(id),
                    ContactLifetime::Temporary(x) => {
                        match x.upgrade() {
                            Some(x) => SearchContact::Owned(x),
                            None => {
                                let inner = Arc::new(Inner {
                                    id,
                                    parent: parent.clone(),
                                });
                                *x = Arc::downgrade(&inner);
                                SearchContact::Owned(inner)
                            },
                        }
                    },
                }
            },
            Entry::Vacant(x) => {
                let inner = Arc::new(Inner {
                    id,
                    parent: parent.clone(),
                });
                x.insert((mailbox, ContactLifetime::Temporary(Arc::downgrade(&inner))));
                SearchContact::Owned(inner)
            },
        }
    }

    fn on_contact_drop(&mut self, id: &Id) {
        if let Some(x) = self.contacts.get_mut(id) {
            if let ContactLifetime::Temporary(_) = x.1 {
                self.contacts.remove(id);
            }
        }
    }
}

#[derive(Clone)]
pub struct Sender {
    id: Id,
    data: Arc<Mutex<TransportData>>,
    receiver: mpsc::Sender<TransportMessage>,
}

impl Sender {
    async fn send_req(self, id: Id, msg: Request) -> Result<RawResponse<SearchContact>, TransportError> {
        trace!("send_req({:?} to {:?}, {:?})", self.id, id, msg);
        let sender = {
            let data = self.data.lock().unwrap();
            data.contacts.get(&id)
                .ok_or(TransportError::ContactLost)?
                .0
                .clone()
        };
        let (tx, rx) = oneshot::channel();

        sender.send(TransportMessage::Request {
            id: self.id.clone(),
            msg: msg.clone(),
            res: tx,
        }).await.expect("Failed to send request");
        let SimulatedResponse {
            payload,
            contacts
        } = rx.await.expect("Error receiving response");

        debug!("{:?} -> {:?} = {:?}? {:?}", self.id, id, msg, payload);

        use RawResponse::*;
        let payload = match payload {
            FoundNodes(nodes) => {
                let x = nodes.into_iter().zip(contacts.into_iter()).collect();
                let (tx, rx) = oneshot::channel();
                self.receiver.send(TransportMessage::ConnectTo {
                    ids: x,
                    res: tx,
                }).await.unwrap();
                FoundNodes(rx.await.unwrap())
            },
            FoundData(x) => FoundData(x),
            Done => Done,
            Error => Error,
        };
        Ok(payload)
    }

    pub async fn connect_to(&self, ids: Vec<(&Id, &Sender)>) {
        let (tx, rx) = oneshot::channel();

        let ids = ids.iter()
            .map(|(id, sender)| ((*id).clone(), sender.receiver.clone()))
            .collect();
        self.receiver.send(TransportMessage::ConnectTo {
            ids,
            res: tx,
        }).await.unwrap();
        rx.await.unwrap();
    }

    pub async fn barrier_sync(&self, barrier: Arc<Barrier>) {
        self.receiver.send(TransportMessage::Barrier(barrier)).await.unwrap();
    }

    pub fn introspect(&self) -> IntrospectionData {
        let data = self.data.lock().unwrap();
        IntrospectionData {
            connection_count: data.contacts.len(),
        }
    }
}

impl TransportSender for Sender {
    fn ping(&self, _id: &Id) {
        // Yes, we don't simulate node failure yet
    }

    type Fut = impl Future<Output=Result<RawResponse<Self::Contact>, TransportError>>;
    fn send(&self, id: &Id, msg: Request) -> Self::Fut {
        let s = self.clone();
        s.send_req(id.clone(), msg)
    }

    fn wrap_contact(&self, id: Id) -> Self::Contact {
        SearchContact::Routed(id)
    }

    type Contact = SearchContact;
}

pub trait IntoDot {
    fn to_dot_string(self) -> String;
}

impl<'a, T> IntoDot for T where T: Iterator<Item = &'a Sender> {
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
    use std::{cmp::Reverse, time::Duration};

    use itertools::Itertools;
    use log::info;
    use rand::{prelude::{StdRng, SliceRandom}, SeedableRng, Rng};

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
        let a = AsyncSimulatedTransport::spawn(config.clone(), aid.clone(), killswitch.subscribe());

        let bid = Id::from_hex("ba");

        let b = AsyncSimulatedTransport::spawn(config, bid.clone(), shutdown);

        // Connect b to a (and vice-versa)
        b.transport().connect_to(vec![(&aid, &a.transport)]).await;

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
        assert_eq!(
            res.iter().map(|x| x.id().clone()).collect::<Vec<_>>(),
            vec![bid.clone(), aid.clone()]
        );

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
            .map(|id| AsyncSimulatedTransport::spawn(config.clone(), id, killswitch.subscribe()))
            .collect::<Vec<_>>();

        // "aaaaaaaa" is the rendevouz DHT (a.k.a. bootstrap dht)
        for i in 1..ids.len() {
            info!("----- NODE {:?} ----", ids[i]);
            dhts[i].transport().connect_to(vec![(&ids[0], &dhts[0].transport)]).await;
            // Bootstrap
            dhts[i].query_nodes(ids[i].clone(), search_options.clone()).await;
        }

        // Everyone is bootstrapped

        // Node-querying test
        let target = Id::from_hex("123456ff");// Note: this node does not exist
        let found = dhts[4].query_nodes(target.clone(), search_options.clone()).await;
        // How can we check that node orderings are equivalent?
        // We should check that the ordering has the best XOR distance from the target node
        assert_eq!(
            found
                .iter()
                .map(|x| (*x.id() ^ target).leading_zeros())
                .collect::<Vec<_>>(),
            ids.iter()
                .map(|x| (*x ^ target).leading_zeros())
                .sorted_by_key(|x| Reverse(*x))
                .take(config.routing.bucket_size)
                .collect::<Vec<_>>()
        );

        // Insertion test
        let data = vec![3u8, 1, 4, 1, 5];
        let d = dhts[4].insert(target.clone(), Duration::from_secs(4), data.clone()).await.unwrap();
        assert_eq!(d, config.routing.bucket_size);// There should be no insertion errors
        let found = dhts[9].query_value(target, search_options.clone()).await;
        // TODO: assert_matches... when it gets stabilized
        assert_eq!(&found.unwrap(), &data);

        // Uncomment to write dot graph file (for visualization)
        /*File::create("sim10.dot").unwrap().write_all(
            dhts.iter()
                .map(|x| x.transport())
                .to_dot_string()
                .as_bytes()
        ).unwrap();*/

        killswitch.send(()).unwrap();
    }

    /// Very expensive test that simulates 100k nodes
    /// takes around 3GiB and (in my crappy laptop) ~5 minutes.
    /// It'd be better to use somewhat parallel bootstrapping.
    #[tokio::test]
    #[ignore]// Intensive test
    async fn simulate_100k() {
        init();
        let mut rng = StdRng::seed_from_u64(0x123456789abcdef0);

        let (killswitch, _shutdown) = broadcast::channel(1);

        let config: SystemConfig = Default::default();
        let search_options = BasicSearchOptions {
            parallelism: 4,
        };

        let n_max = 100_000usize;
        let ids: Vec<Id> = (0..n_max)
            .map(|_| rng.gen())
            .collect();

        let dhts = ids.iter()
            .cloned()
            .map(|id| AsyncSimulatedTransport::spawn(config.clone(), id, killswitch.subscribe()))
            .collect::<Vec<_>>();


        info!("Bootstrapping nodes...");
        // the first node is the rendevouz DHT (a.k.a. bootstrap dht)
        for i in 1..ids.len() {
            if i % 1000 == 0 {
                info!("{i}/{n_max}");
            }
            dhts[i].transport().connect_to(vec![(&ids[0], &dhts[0].transport)]).await;
            // Bootstrap
            dhts[i].bootstrap(search_options.clone(), &mut rng).await;
        }

        let (min, max, avg) = dhts.iter()
            .map(|x| x.transport().introspect().connection_count)
            .fold((std::usize::MAX, 0usize, 0usize), |a, b| {
                (a.0.min(b), a.1.max(b), a.2 + b)
            });
        let avg = avg as f32 / dhts.len() as f32;

        info!("Connections:\n\
        min/max/avg\n\
        {min}/{max}/{avg:.3}");

        info!("Starting node search test...");
        // Node querying tests:
        for _ in 0..1000 {
            // Node-querying test
            let target: Id = rng.gen();
            let receiver = dhts.choose(&mut rng).unwrap();
            debug!("Searching {:?} from {:?}", target, receiver.id());
            let found = receiver.query_nodes(target.clone(), search_options.clone()).await;
            // How can we check that node orderings are equivalent?
            // We should check that the ordering has the best XOR distance from the target node
            assert_eq!(
                found
                    .iter()
                    .map(|x| (*x.id() ^ target).leading_zeros())
                    .collect::<Vec<_>>(),
                ids.iter()
                    .map(|x| (*x ^ target).leading_zeros())
                    .sorted_by_key(|x| Reverse(*x))
                    .take(config.routing.bucket_size)
                    .collect::<Vec<_>>()
            );
        }

        info!("Starting insertion/retrieval test...");

        // Insertion & retrieval test
        for _ in 0..100 {
            let target: Id = rng.gen();
            let (pusher, receiver) = dhts.choose_multiple(&mut rng, 2).next_tuple().unwrap();
            let data = rng.gen::<u128>().to_be_bytes().to_vec();

            debug!("Inserting {:?} from {:?}", target, pusher.id());
            let received = pusher.insert(target.clone(), Duration::from_secs(1), data.clone()).await.unwrap();
            assert_eq!(received, config.routing.bucket_size);
            debug!("Retrieving {:?} from {:?}", target, receiver.id());
            let found = receiver.query_value(target, search_options.clone()).await.unwrap();
            assert_eq!(found, data);
        }
        info!("Shutting system down");

        killswitch.send(()).unwrap();
    }
}
