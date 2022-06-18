use std::{
    sync::{Mutex, RwLock},
    time::Duration,
};

use futures::{stream::FuturesUnordered, StreamExt};
use rand::Rng;
use tracing::{debug, error, event, info, instrument, warn, Level};

use crate::{
    config::SystemConfig,
    id::Id,
    ktree::KTree,
    search::{BasicSearch, BasicSearchOptions, SearchResult, SearchType},
    storage::Storage,
    transport::{Contact, RawResponse, Request, Response, TransportListener, TransportSender, TopicEntry},
};

// TODO: push syncronization down the line to improve async performance
pub struct KademliaDht<T: TransportSender> {
    // Immutable data
    config: SystemConfig,
    id: Id,
    // Mutable runtime data
    pub transport: T,
    pub tree: Mutex<KTree>, // TODO: dashmap?
    pub storage: RwLock<Storage>,
}

impl<T: TransportSender> KademliaDht<T> {
    pub fn new(config: SystemConfig, id: Id, transport: T) -> Self {
        Self {
            config: config.clone(),
            id,
            transport,
            tree: Mutex::new(KTree::new(id, config.routing)),
            storage: RwLock::new(Storage::new(config.storage)),
        }
    }

    pub fn config(&self) -> &SystemConfig {
        &self.config
    }

    pub fn id(&self) -> Id {
        self.id
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub fn periodic_run(&self) {
        self.storage.write().unwrap().periodic_run();
    }

    fn get_closer_bucket(&self, key: Id) -> Vec<T::Contact> {
        self.tree
            .lock()
            .unwrap()
            .get_closer_n(key, self.config.routing.bucket_size)
            .iter()
            .map(|x| self.transport.wrap_contact(*x))
            .collect()
    }

    pub async fn query_value(&self, key: Id, max_entry_count: u32, options: BasicSearchOptions) -> Option<Vec<TopicEntry>> {
        {
            // Check if it's already in storage
            let storage = self.storage.read().unwrap();
            let data = storage.get(key);
            if let Some(data) = data {
                return Some(data.clone());
            }
        }

        let bucket = self.get_closer_bucket(key);
        let searcher = BasicSearch::create(self, options, SearchType::Data(max_entry_count), key);
        match searcher.search(bucket).await {
            SearchResult::CloserNodes(_) => None,
            SearchResult::DataFound(x) => Some(x),
        }
    }

    pub async fn query_nodes(&self, key: Id, options: BasicSearchOptions) -> Vec<T::Contact> {
        let bucket = self.get_closer_bucket(key);
        let searcher = BasicSearch::create(self, options, SearchType::Nodes, key);
        match searcher.search(bucket).await {
            SearchResult::CloserNodes(x) => x,
            SearchResult::DataFound(_) => unreachable!(),
        }
    }

    pub async fn bootstrap<R: Rng>(&self, options: BasicSearchOptions, rng: &mut R) {
        let nodes = self.query_nodes(self.id, options.clone()).await;

        // We are at index 0, because no-one can be closer than us
        // TODO: what about conflicts? We should be able to handle these
        let closest_sibling = match nodes.get(1) {
            None => return, // DHT is empty, we are the only node
            Some(x) => x,
        };

        let max_leading_zeros = (self.id ^ closest_sibling.id()).leading_zeros();

        let mut fu = (0..max_leading_zeros)
            .rev()
            .map(|bucket| {
                let original_mask = Id::create_left_mask(bucket + 1);
                // Keep original bucket - 1 bits, invert the bucket bit, randomically generate other bits
                (self.id ^ Id::ZERO.set_bit(bucket) & original_mask)
                    | (rng.gen::<Id>() & !original_mask)
            })
            .map(|id| self.query_nodes(id, options.clone()))
            .collect::<FuturesUnordered<_>>();

        while fu.next().await.is_some() {
            continue;
        }
    }

    async fn send_request_and_count(&self, nodes: Vec<T::Contact>, request: Request) -> usize {
        let mut answers = nodes
            .iter()
            .filter(|x| x.id() != self.id)
            .map(|x| async {
                // tag the future (to know which clients started it)
                (
                    x.clone(),
                    self.transport.send(x.id(), request.clone()).await,
                )
            })
            .collect::<FuturesUnordered<_>>();

        let mut count = 0;

        while let Some((id, x)) = answers.next().await {
            match x {
                Ok(RawResponse::Done) => count += 1,
                Ok(RawResponse::Error) => warn!("{id:?} returned an error"),
                Ok(_) => warn!("Unknown response received from {id:?}"),
                Err(x) => warn!("Transport error querying {id:?}: {x}"),
            }
        }

        count
    }

    pub async fn insert(
        &self,
        key: Id,
        lifetime: Duration,
        value: Vec<u8>,
    ) -> Result<usize, crate::storage::Error> {
        // Insert key in the k closest nodes
        let lifetime = lifetime.as_secs() as u32;

        Storage::check_entry(&self.config.storage, key, self.id, lifetime, &value)?;

        info!("Inserting {key:?} into the network for {lifetime}s -> '{value:x?}'");

        let search_options = BasicSearchOptions { parallelism: 2 };
        let nodes = self.query_nodes(key, search_options).await;

        let mut installation_count = 0;

        if nodes.iter().any(|x| x.id() == self.id) {
            self.storage
                .write()
                .unwrap()
                .insert(key, self.id, lifetime, value.clone())
                .unwrap();
            installation_count += 1;
        }

        let request = Request::Insert(key, lifetime, value);

        installation_count += self.send_request_and_count(nodes, request).await;

        Ok(installation_count)
    }

    pub async fn remove(&self, key: Id) -> usize {
        info!("Removing {key:?} into the network");

        let search_options = BasicSearchOptions { parallelism: 2 };
        let nodes = self.query_nodes(key, search_options).await;

        let mut removed_count = 0;

        if nodes.iter().any(|x| x.id() == self.id) {
            self.storage
                .write()
                .unwrap()
                .remove(key, self.id);
                removed_count += 1;
        }

        let request = Request::Remove(key);

        removed_count += self.send_request_and_count(nodes, request).await;
        removed_count
    }
}

impl<T: TransportSender> TransportListener for KademliaDht<T> {
    fn on_connect(&self, id: Id) -> bool {
        event!(Level::INFO, kad_id=%self.id, "Connnected {id}");
        self.tree.lock().unwrap().insert(id, &self.transport)
    }

    fn on_disconnect(&self, id: Id) {
        event!(Level::INFO, kad_id=%self.id, "Disconnected {id}");
        self.tree.lock().unwrap().remove(id);
    }

    #[instrument(skip(self), fields(kad_id=%self.id, %sender))]
    fn on_request(&self, sender: Id, message: Request) -> Response {
        debug!("Request: {:?}", message);
        let mut tree = self.tree.lock().unwrap();
        tree.refresh(sender);

        match message {
            Request::FindNodes(topic) => {
                // TODO: how many nodes to search?
                let found = tree.get_closer_n(topic, self.config.routing.bucket_size);
                let found = found.into_iter().filter(|x| *x != sender).collect();

                debug!("| Find closer {topic:?}: {found:?}");
                Response::FoundNodes(found)
            }

            Request::FindData(topic, limit) => {
                // Send data if stored
                // Else send closer nodes known
                let storage = self.storage.read().unwrap();
                let res = match storage.get(topic) {
                    Some(entries) => Response::FoundData(
                        entries.iter()
                            // Always get the last entries (skip the first entries - limit entries)
                            .skip(entries.len().saturating_sub(limit as usize))
                            .cloned()
                            .collect()
                    ),
                    None => Response::FoundNodes(
                        tree.get_closer_n(topic, self.config.routing.bucket_size)
                            .into_iter()
                            .filter(|x| *x != sender)
                            .collect(),
                    ),
                };
                debug!("Find data {topic:?}({limit}): {res:?}");
                res
            }

            Request::Insert(topic, lifetime, data) => {
                // TODO: protection against SPAM attacks? (ex. merkle challenges?)
                debug!("| Insert {topic:?} {lifetime}s -> '{data:x?}'");
                let mut storage = self.storage.write().unwrap();
                match storage.insert(topic, sender, lifetime, data) {
                    Ok(_) => Response::Done,
                    Err(x) => {
                        error!("Error inserting value: {x}");
                        Response::Error
                    }
                }
            }

            Request::Remove(topic) => {
                debug!("| Remove {topic:?}");
                let mut storage = self.storage.write().unwrap();
                storage.remove(topic, sender);
                Response::Done
            }
        }
    }
}
