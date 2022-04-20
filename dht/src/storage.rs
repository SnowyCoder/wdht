use std::{collections::{HashMap, BinaryHeap}, time::{Instant, Duration}};

use log::info;
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
        while let Some((deadline, id)) = self.deadlines.peek() {
            if *deadline > now {
                break;
            }

            info!("Removing {id:?}");

            let id = self.deadlines.pop().unwrap().1;
            self.data.remove(&id);
        }
    }

    pub fn check_entry(config: &StorageConfig, _id: &Id, lifetime: u32, data: &[u8]) -> Result<(), Error> {
        if data.len() > config.max_size {
            Err(Error::InvalidData)
        } else if lifetime > config.max_lifetime {
            Err(Error::InvalidLifetime)
        } else {
            Ok(())
        }
    }

    pub fn insert(&mut self, id: Id, lifetime: u32, data: Vec<u8>) -> Result<(), Error> {
        // TODO: check distance?
        Self::check_entry(&self.config, &id, lifetime, &data)?;

        if self.data.len() >= self.config.max_entries {
            info!("Error inserting new value, too many entries");
            return Err(Error::TooManyEntries);
        }
        info!("Inserting {id:?} for {lifetime}s");

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
