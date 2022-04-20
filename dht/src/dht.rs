use std::{sync::{RwLock, Mutex}, time::Duration};

use futures::{stream::FuturesUnordered, StreamExt};
use log::{error, debug, info, warn};

use crate::{ktree::KTree, config::SystemConfig, transport::{TransportSender, Request, Response, TransportListener}, id::Id, storage::Storage, search::{BasicSearch, BasicSearchOptions, SearchType, SearchResult}};


// TODO: push syncronization down the line to improve async performance
pub struct KademliaDht<T: TransportSender> {
    // Immutable data
    config: SystemConfig,
    id: Id,
    // Mutable runtime data
    pub transport: T,
    pub tree: Mutex<KTree>,// TODO: dashmap?
    pub storage: RwLock<Storage>,
}

impl<T: TransportSender> KademliaDht<T> {
    pub fn new(config: SystemConfig, id: Id, transport: T) -> Self {
        Self {
            config: config.clone(),
            id: id.clone(),
            transport,
            tree: Mutex::new(KTree::new(id, config.routing)),
            storage: RwLock::new(Storage::new(config.storage)),
        }
    }

    pub fn config(&self) -> &SystemConfig {
        &self.config
    }

    pub fn id(&self) -> &Id {
        &self.id
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub fn periodic_run(&self) {
        self.storage.write().unwrap().periodic_run();
    }

    fn get_closer_bucket(&self, key: &Id) -> Vec<Id> {
        self.tree.lock().unwrap()
            .get_closer_n(key, self.config.routing.bucket_size)
            .iter()
            .cloned()
            .cloned()
            .collect()
    }

    pub async fn query_value(&self, key: Id, options: BasicSearchOptions) -> Option<Vec<u8>> {
        {// Check if it's already in storage
            let storage = self.storage.read().unwrap();
            let data = storage.get(&key);
            if let Some(data) = data {
                return Some(data.clone());
            }
        }

        let bucket = self.get_closer_bucket(&key);
        let searcher = BasicSearch::create(self, options, SearchType::Data, key);
        match searcher.search(bucket).await {
            SearchResult::CloserNodes(_) => None,
            SearchResult::DataFound(x) => Some(x),
        }
    }

    pub async fn query_nodes(&self, key: Id, options: BasicSearchOptions) -> Vec<Id> {
        let bucket = self.get_closer_bucket(&key);
        let searcher = BasicSearch::create(self, options, SearchType::Nodes, key);
        match searcher.search(bucket).await {
            SearchResult::CloserNodes(x) => x,
            SearchResult::DataFound(_) => unreachable!(),
        }
    }

    pub async fn insert(&self, key: Id, lifetime: Duration, value: Vec<u8>) -> Result<usize, crate::storage::Error> {
        // Insert key in the k closest nodes
        let lifetime = lifetime.as_secs() as u32;

        Storage::check_entry(&self.config.storage, &key, lifetime, &value)?;

        info!("Inserting {key:?} into the network for {lifetime}s");

        let search_options = BasicSearchOptions { parallelism: 4 };
        let nodes = self.query_nodes(key.clone(), search_options).await;

        let mut installation_count = 0;

        if nodes.iter().any(|x| *x == self.id) {
            self.storage.write().unwrap().insert(key.clone(), lifetime, value.clone()).unwrap();
            installation_count += 1;
        }

        let request = Request::Insert(key, lifetime, value);

        let mut answers = nodes.iter()
            .filter(|x| **x != self.id)
            .map(|x| async {
                // tag the future (to know which clients started it)
                (x.clone(), self.transport.send(x, request.clone()).await)
            })
            .collect::<FuturesUnordered<_>>();

        while let Some((id, x)) = answers.next().await {
            match x {
                Ok(Response::Done) => installation_count += 1,
                Ok(Response::Error) => warn!("{id:?} returned an error"),
                Ok(_) => warn!("Unknown response received from {id:?}"),
                Err(x) => warn!("Transport error querying {id:?}: {x}"),
            }
        }

        Ok(installation_count)
    }
}


impl<T: TransportSender> TransportListener for KademliaDht<T> {
    fn on_connect(&self, id: &Id) -> bool {
        info!("{:?}: Connnected {:?}", self.id, id);
        self.tree.lock()
            .unwrap()
            .insert(id.clone(), &self.transport)
    }

    fn on_disconnect(&self, id: &Id) {
        self.tree.lock()
            .unwrap()
            .remove(id);
    }

    fn on_request(&self, sender: &Id, message: Request) -> Response {
        debug!("{:?} Request from {:?}: {:?}", self.id(), sender, message);
        let mut tree = self.tree.lock().unwrap();
        tree.refresh(sender);

        match message {
            Request::FindNodes(x) => {
                // TODO: how many nodes to search?
                let found = tree.get_closer_n(&x, self.config.routing.bucket_size);
                let found = found.into_iter()
                    .filter(|x| *x != sender)
                    .map(|x| (*x).clone())
                    .collect();

                debug!("| Find closer {:?}: {:?}", x, found);
                Response::FoundNodes(found)
            }

            Request::FindData(x) => {
                // Send data if stored
                // Else send closer nodes known
                let storage = self.storage.read().unwrap();
                let res = match storage.get(&x) {
                    Some(x) => Response::FoundData(x.clone()),
                    None => Response::FoundNodes(
                        tree
                            .get_closer_n(&x, self.config.routing.bucket_size)
                            .iter()
                            .map(|x| (*x).clone())
                            .collect()
                    )
                };
                debug!("Find data {:?}: {:?}", x, res);
                res
            }

            Request::Insert(file_id, lifetime, data) => {
                // TODO: protection against SPAM attacks? (ex. merkle challenges?)
                debug!("Inserting value {:?} {} {:x?}", file_id, lifetime, data);
                let mut storage = self.storage.write().unwrap();
                match storage.insert(file_id, lifetime, data) {
                    Ok(_) => Response::Done,
                    Err(x) => {
                        error!("Error inserting value: {}", x);
                        Response::Error
                    }
                }
            }
        }
    }
}
