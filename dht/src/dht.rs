use std::sync::{RwLock, Mutex, Arc};

use log::{error, debug};

use crate::{ktree::KTree, config::SystemConfig, contacter::{Transport, Request, Response}, id::Id, storage::Storage, search::{BasicSearch, BasicSearchOptions}};


// TODO: push syncronization down the line to improve async performance
pub struct KademliaDht<T: Transport> {
    // Immutable data
    config: SystemConfig,
    id: Id,
    // Mutable runtime data
    pub transport: T,
    pub tree: Mutex<KTree>,// TODO: dashmap?
    pub storage: RwLock<Storage>,
}

impl<T: Transport> KademliaDht<T> {
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

    pub fn on_connect(&self, id: &Id) {
        self.tree.lock()
            .unwrap()
            .insert(id.clone(), &self.transport);
    }

    pub fn on_disconnect(&self, id: &Id) {
        self.tree.lock()
            .unwrap()
            .remove(id);
    }

    pub fn on_request(&self, sender: &Id, message: Request) -> Response {
        debug!("Request from {:?}: {:?}", sender, message);
        let mut tree = self.tree.lock().unwrap();
        tree.refresh(sender);

        match message {
            Request::FindNodes(x) => {
                // TODO: how many nodes to search?
                let found = tree.get_closer_n(&x, self.config.routing.bucket_size);

                debug!("Find closer {:?}: {:?}", x, found);
                Response::FoundNodes(
                    found.iter().map(|x| (*x).clone()).collect()
                )
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
                match storage.insert(&self.id, file_id, lifetime, data) {
                    Ok(_) => Response::Done,
                    Err(x) => {
                        error!("Error inserting value: {}", x);
                        Response::Error
                    }
                }
            }
        }
    }

    pub async fn query_value(self: Arc<Self>, _key: Id) -> Option<Vec<u8>> {
        todo!()
    }

    pub async fn query_nodes(&self, key: Id, options: BasicSearchOptions) -> Vec<Id> {
        let bucket = self.tree.lock().unwrap()
            .get_closer_n(&key, self.config.routing.bucket_size)
            .iter()
            .cloned()
            .cloned()
            .collect();
        let searcher = BasicSearch::create(self, options, key);
        searcher.search(bucket).await
    }
}
