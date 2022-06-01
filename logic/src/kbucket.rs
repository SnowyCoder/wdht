use crate::{config::RoutingConfig, id::Id, transport::TransportSender};

#[derive(Debug, Default)]
pub struct KBucket {
    pub entries: Vec<Id>,
    pub replacement_cache: Vec<Id>,
}

impl KBucket {
    pub fn refresh_node(&mut self, id: Id) -> bool {
        let entry = self.entries.iter_mut().enumerate().find(|(_, x)| **x == id);

        match entry {
            Some((index, _entry)) => {
                // Bring element at index to the back
                self.entries[index..].rotate_right(1);
                true
            }
            None => false,
        }
    }

    pub fn has(&self, id: Id) -> bool {
        (self.entries.iter().chain(self.replacement_cache.iter())).any(|x| *x == id)
    }

    pub fn insert<T: TransportSender>(
        &mut self,
        id: Id,
        config: &RoutingConfig,
        contacter: &T,
    ) -> bool {
        if self.has(id) {
            return false;
        }
        if self.entries.len() < config.bucket_size {
            self.entries.push(id);
            return true;
        }

        if self.replacement_cache.len() < config.bucket_replacement_size {
            self.replacement_cache.push(id);
            for x in self.entries.iter() {
                contacter.ping(*x);
            }
            true
        } else {
            false
        }
    }

    pub fn remove(&mut self, id: Id) -> bool {
        let i = self.entries.iter().position(|x| *x == id);
        if let Some(i) = i {
            self.entries.remove(i);
            // Promote one item from the cache (if present)
            if !self.replacement_cache.is_empty() {
                self.entries.push(self.replacement_cache.remove(0));
            }
            true
        } else if let Some(i) = self.replacement_cache.iter().position(|x| *x == id) {
            self.replacement_cache.remove(i);
            true
        } else {
            false
        }
    }
}
