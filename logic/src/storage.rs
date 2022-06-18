use instant::Instant;
use priority_queue::PriorityQueue;
use std::{
    collections::{HashMap, hash_map::Entry},
    time::Duration,
};

use thiserror::Error;
use tracing::info;

use crate::{config::StorageConfig, id::Id, transport::TopicEntry};

#[derive(Error, Debug)]
#[non_exhaustive]
pub enum Error {
    #[error("Too many entries stored")]
    TooManyEntries,
    #[error("Invalid data lifetime")]
    InvalidLifetime,
    #[error("Invalid data")]
    InvalidData,
}

#[derive(Clone, Debug)]
pub struct Storage {
    config: StorageConfig,
    entry_count: usize,
    topics: HashMap<Id, Vec<TopicEntry>>,
    deadlines: PriorityQueue<(Id, Id), Instant>,
    // TODO: cache
    // cache: HashMap<Id, Vec<u8>>,
    // cache_deadlines: BinaryHeap<(Instant, Id)>,
}

impl Storage {
    pub fn new(config: StorageConfig) -> Self {
        Storage {
            config,
            entry_count: 0,
            topics: Default::default(),
            deadlines: Default::default(),
        }
    }

    pub fn get(&self, id: Id) -> Option<&Vec<TopicEntry>> {
        self.topics.get(&id)
    }

    pub fn periodic_run(&mut self) {
        let now = Instant::now();
        // Remove old entries
        while let Some(((topic, user), deadline)) = self.deadlines.peek() {
            if *deadline > now {
                break;
            }

            info!("Removing topic: {topic:?} user: {user:?}");

            let id = self.deadlines.pop().unwrap().0;
            self.remove(id.0, id.1);
        }
    }

    pub fn check_entry(
        config: &StorageConfig,
        _topic: Id,
        _sender: Id,
        lifetime: u32,
        data: &[u8],
    ) -> Result<(), Error> {
        if data.len() > config.max_size {
            Err(Error::InvalidData)
        } else if lifetime > config.max_lifetime {
            Err(Error::InvalidLifetime)
        } else {
            Ok(())
        }
    }

    pub fn insert(&mut self, topic: Id, publisher: Id, lifetime: u32, data: Vec<u8>) -> Result<(), Error> {
        // TODO: check distance?
        Self::check_entry(&self.config, topic, publisher, lifetime, &data)?;

        self.remove(topic, publisher);

        if self.entry_count >= self.config.max_entries {
            info!("Error inserting new value, too many entries");
            return Err(Error::TooManyEntries);
        }
        info!("Inserting {topic:?}:{publisher:?} for {lifetime}s");

        let deadline = Instant::now().checked_add(Duration::from_secs(lifetime as u64));
        let deadline = match deadline {
            Some(x) => x,
            None => return Err(Error::InvalidLifetime),
        };

        let entry = TopicEntry {
            publisher,
            data,
        };
        self.topics.entry(topic).or_default().push(entry);
        self.deadlines.push((topic, publisher), deadline);
        self.entry_count += 1;

        Ok(())
    }

    pub fn remove(&mut self, topic: Id, user: Id) {
        if let Entry::Occupied(mut o) = self.topics.entry(topic) {
            // Search for position of publisher
            let pos = o.get_mut().iter().position(|x| x.publisher == user);
            // if the element is found
            if let Some(pos) = pos {
                // remove the element
                o.get_mut().remove(pos);
                self.entry_count -= 1;
                self.deadlines.remove(&(topic, user));
                // if the topic is empty, remove it from the map
                if o.get().is_empty() {
                    o.remove_entry();
                }
            }
        }
    }
}
