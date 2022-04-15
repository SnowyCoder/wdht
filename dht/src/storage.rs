use std::{collections::{HashMap, BinaryHeap}, time::{Instant, Duration}};

use thiserror::Error;

use crate::{id::Id, config::StorageConfig};

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

// TODO: would it be better to have an opaque redirection layer? (ex: Id -> usize)
// TODO: add append-like storage
#[derive(Clone, Debug)]
pub struct Storage {
    config: StorageConfig,
    data: HashMap<Id, Vec<u8>>,
    deadlines: BinaryHeap<(Instant, Id)>,
    // TODO: cache
    // cache: HashMap<Id, Vec<u8>>,
    // cache_deadlines: BinaryHeap<(Instant, Id)>,
}

impl Storage {
    pub fn new(config: StorageConfig) -> Self {
        Storage {
            config,
            data: Default::default(),
            deadlines: Default::default(),
        }
    }

    pub fn get(&self, id: &Id) -> Option<&Vec<u8>> {
        self.data.get(id)
    }

    pub fn periodic_run(&mut self) {
        let now = Instant::now();
        // Remove old entries
        while let Some((deadline, _id)) = self.deadlines.peek() {
            if *deadline > now {
                break;
            }

            let id = self.deadlines.pop().unwrap().1;
            self.data.remove(&id);
        }
    }

    pub fn insert(&mut self, _self_id: &Id, id: Id, lifetime: u32, data: Vec<u8>) -> Result<(), Error> {
        // TODO: check distance?

        if self.data.len() >= self.config.max_entries {
            return Err(Error::TooManyEntries);
        }
        if data.len() > self.config.max_size {
            return Err(Error::InvalidData);
        }
        if lifetime > self.config.max_lifetime {
            return Err(Error::InvalidLifetime);
        }

        let deadline = Instant::now()
            .checked_add(Duration::from_secs(lifetime as u64));
        let deadline = match deadline {
            Some(x) => x,
            None => return Err(Error::InvalidLifetime),
        };

        self.data.insert(id.clone(), data);
        self.deadlines.push((deadline, id));

        Ok(())
    }
}