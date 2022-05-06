use crate::{config::RoutingConfig, consts::ID_LEN_BITS, kbucket::KBucket, id::Id, transport::TransportSender};


pub struct KTreeEntry {
    buckets: Vec<KBucket>,
}

impl KTreeEntry {
    pub fn new(config: &RoutingConfig) -> Self {
        KTreeEntry {
            // Create 2**buckets_per_bit buckets (each bit is one entry)
            buckets: (0..1 << (config.buckets_per_bit as usize - 1)).into_iter()
                    .map(|_| KBucket::default())
                    .collect(),
        }
    }
}

pub struct KTree {
    id: Id,
    config: RoutingConfig,
    nodes: [KTreeEntry; ID_LEN_BITS],
    size: u64,
}

impl KTree {
    pub fn new(id: Id, config: RoutingConfig) -> Self {
        let nodes = [(); ID_LEN_BITS].map(|_| KTreeEntry::new(&config));
        KTree {
            id,
            config,
            nodes,
            size: 0,
        }
    }

    fn get_bucket_index(&self, id: Id) -> (usize, usize) {
        let nid = self.id ^ id;
        let entryi = nid.leading_zeros()
            .min((ID_LEN_BITS - self.config.buckets_per_bit) as u8);
        let bucketi = if self.config.buckets_per_bit == 1 {
            0// fast path (please compiler optimize it away)
        } else {
            nid.bitslice(
                entryi as u32 + 1,
                self.config.buckets_per_bit as u8 - 1
            ) as usize
        };
        return (entryi as usize, bucketi);
    }

    fn get_bucket(&self, id: Id) -> &KBucket {
        let indexes = self.get_bucket_index(id);
        &self.nodes[indexes.0].buckets[indexes.1]
    }

    fn get_bucket_mut(&mut self, id: Id) -> &mut KBucket {
        let indexes = self.get_bucket_index(id);
        &mut self.nodes[indexes.0].buckets[indexes.1]
    }

    pub fn has(&self, id: Id) -> bool {
        return self.get_bucket(id).has(id);
    }

    pub fn insert<T: TransportSender>(&mut self, id: Id, contacter: &T) -> bool {
        if id == self.id {
            return false;
        }
        // Check max connection count
        if self.config.max_routing_count.map_or(false, |max| self.size >= max.get()) {
            return false;
        }
        let index = self.get_bucket_index(id);
        let inserted = self.nodes[index.0]
            .buckets[index.1]
            .insert(id, &self.config, contacter);

        if inserted {
            self.size += 1;
        }
        inserted
    }

    pub fn remove(&mut self, id: Id) -> bool {
        let removed = self.get_bucket_mut(id).remove(id);
        if removed {
            self.size -= 1;
        }
        removed
    }

    pub fn refresh(&mut self, id: Id) -> bool {
        self.get_bucket_mut(id).refresh_node(id)
    }

    pub fn get_closer_n(&self, closer_to: Id, size: usize) -> Vec<Id> {
        let mut res = NodeAggregator::new(size);
        let index = self.get_bucket_index(closer_to);

        let fentry = &self.nodes[index.0];
        res.add_bucket(&fentry.buckets[index.1]);
        if res.is_done() {
            // fast return n.1, everything is in one bucket
            return res.finish(closer_to);
        }

        // Add every other bucket
        for (_i, bucket) in fentry.buckets.iter()
                .enumerate()
                .filter(|(i, _)| *i != index.1) {
            res.add_bucket(bucket);
        }

        if res.is_done() {
            // fast return n.2, everything is in one entry
            return res.finish(closer_to);
        }

        // 00000000000000000000000000000000-0000000000000000-00000000-0000-00-0
        // Play with this ^ to understand the following method
        //
        // We need to explore buckets in some sort of order such that when we stop
        // we know that there are no other closer known ids.
        // The idea is, when we are in this code line
        // <--
        // We have already explored one node in the "tree", let's suppose we aren't
        // on one of the edges (since they are quire obvious cases).
        // 00000000000000000000000000000000-0000000000000000-00000000-0000-00-0
        //                                                   ~~~~~~~~
        //                                  |------16------| |---8--| |---7---|
        // Let's assume that we are in entry 0 < i < BITS - 1 (bits in key)
        // If we are then we know that on the right (i+1..BITS) we have multiple "nodes",
        // whose summed space is the same as our last explored space minus one,
        // and on the left we have a neighbour which size is double the last explored entry.
        // if we wanted an index that is in bucket i, then where can something in
        // a space closer to bucket i.
        // This space can be on the neighbour on the left (i-1) OR on all space on the right
        // (i+1..BITS). So we can:
        // - execute the search on the right (i+1..BITS) one by one
        // - when we found enough nodes, we also add our left neighbout nodes (i-1)
        // - we return and trim the searched nodes
        // This is a shortcut so that we don't need to search all of the right side
        // Also, if on the right side we can't find enough nodes we need to search
        // on all of the left side.
        //
        // Explaination:
        // Search 8 nodes
        // Contained nodes: 4                       5           2      4    3 1
        // 00000000000000000000000000000000-0000000000000000-00000000-0000-00-0
        // Index:           0                       1           2      3    4 5
        // - Search an id in 2, collected nodes: 2
        // - Search on the right side
        //   + Collect in 3 (collected: 2+4=6)
        //   + Collect in 4 (collected: 6+3=9 > 8!)
        // - Search also in left neighbour
        //   + Collect in 1 (collected: 9+5=14)
        // - Trim results (from 14 nodes to only the closest 8)
        // Solving the other 2 edge cases (literally)
        // - if we need to find something on index 0 we just need to search
        //   on the right side
        // - if we need to find something on index BITS-1 there is no right
        //   side so we just search on the left


        // So: search on the right side
        for entry in self.nodes.iter().skip(index.0 + 1) {
            res.add_entry(entry);
            if res.is_done() {
                // If we are done also add the first leftmost neighbour (if present)
                if index.0 != 0 {
                    res.add_entry(&self.nodes[index.0 - 1]);
                }
                return res.finish(closer_to);
            }
        }
        // We didn't find enough on the right side
        // search on the left
        for entry in self.nodes.iter().take(index.0).rev() {
            res.add_entry(entry);
            if res.is_done() {
                break;
            }
        }

        res.finish(closer_to)
    }
}

/// Utility struct that manages nodes aggregation for closer_n queries
struct NodeAggregator {
    nodes: Vec<Id>,
    limit: usize,
}

impl NodeAggregator {
    pub fn new(limit: usize) -> Self {
        NodeAggregator {
            nodes: Vec::new(),
            limit,
        }
    }

    pub fn is_done(&self) -> bool {
        self.nodes.len() >= self.limit
    }

    pub fn add_bucket(&mut self, bucket: &KBucket) {
        for x in bucket.entries.iter() {
            self.nodes.push(*x);
        }
    }

    pub fn add_entry(&mut self, entry: &KTreeEntry) {
        for x in entry.buckets.iter() {
            self.add_bucket(x);
        }
    }

    pub fn finish(self, closer_to: Id) -> Vec<Id> {
        let Self {nodes: mut vec, limit} = self;
        vec.sort_unstable_by_key(|x| closer_to ^ *x);
        vec.truncate(limit);
        vec
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, future, sync::{Arc, Mutex, MutexGuard}};

    use crate::transport::{Response, TransportError};

    use super::*;

    #[derive(Clone)]
    struct IgnoreContacter;

    impl TransportSender for IgnoreContacter {
        fn ping(&self, _id: &Id) {}

        type Fut = future::Ready<Result<Response, TransportError>>;
        fn send(&self, _id: &Id, _msg: crate::transport::Request) -> Self::Fut {
            panic!();
        }

        type Contact = Id;

        fn wrap_contact(&self, _id: Id) -> Self::Contact {
            panic!();
        }
    }

    #[test]
    fn basic() {
        let id = Id::from_hex("a0000000");
        let config = RoutingConfig {
            bucket_size: 2,
            bucket_replacement_size: 1,
            buckets_per_bit: 1,
            ..Default::default()
        };
        let mut tree = KTree::new(id, config);
        // Add similar entries to the same bucket,
        // since the bucket size is 2 it will overflow
        let contacter = &mut IgnoreContacter;
        assert_eq!(tree.insert(Id::from_hex("b0000001"), contacter), true);
        assert_eq!(tree.insert(Id::from_hex("b0000010"), contacter), true);
        assert_eq!(tree.insert(Id::from_hex("b0000011"), contacter), true);// cache
        assert_eq!(tree.insert(Id::from_hex("b0000100"), contacter), false);
        // Add similar entries but closer to the tree
        // closer bucket (0)
        assert_eq!(tree.insert(Id::from_hex("a0000001"), contacter), true);
        // bucket 1
        assert_eq!(tree.insert(Id::from_hex("a0000010"), contacter), true);
        assert_eq!(tree.insert(Id::from_hex("a0000011"), contacter), true);
        // bucket 2
        assert_eq!(tree.insert(Id::from_hex("a0000100"), contacter), true);
        assert_eq!(tree.insert(Id::from_hex("a0000101"), contacter), true);
        assert_eq!(tree.insert(Id::from_hex("a0000110"), contacter), true);// cache
        assert_eq!(tree.insert(Id::from_hex("a0000111"), contacter), false);// full

        // client a100 disconnects, so a110 enters cache and we can insert a111 (well, in the cache)
        tree.remove(Id::from_hex("a0000100"));
        assert_eq!(tree.insert(Id::from_hex("a0000111"), contacter), true);// cached
    }

    #[test]
    fn closer_n() {
        let id = Id::from_hex("a0000000");
        let config = RoutingConfig {
            bucket_size: 2,
            bucket_replacement_size: 1,
            buckets_per_bit: 1,
            ..Default::default()
        };

        let mut tree = KTree::new(id, config);
        let contacter = &mut IgnoreContacter;

        tree.insert(Id::from_hex("b0000000"), contacter);
        tree.insert(Id::from_hex("b0001000"), contacter);
        tree.insert(Id::from_hex("a0001000"), contacter);
        tree.insert(Id::from_hex("a0000001"), contacter);
        tree.insert(Id::from_hex("a0000010"), contacter);

        let actual = tree.get_closer_n(Id::from_hex("b0001001"), 3)
                .iter()
                .map(|x| (*x).clone())
                .collect::<Vec<_>>();
        assert_eq!(vec![
            Id::from_hex("b0001000"),
            Id::from_hex("b0000000"),
            Id::from_hex("a0001000"),
        ], actual);
    }

    #[derive(Clone)]
    struct MapContacter(pub Arc<Mutex<HashMap<Id, usize>>>);

    impl MapContacter {
        pub fn inner(&self) -> MutexGuard<HashMap<Id, usize>> {
            self.0.lock().unwrap()
        }
    }

    impl TransportSender for MapContacter {
        fn ping(&self, id: &Id) {
            *self.inner()
                .entry(id.clone())
                .or_insert(0) += 1;
        }

        type Fut = future::Ready<Result<Response, TransportError>>;
        fn send(&self, _id: &Id, _msg: crate::transport::Request) -> Self::Fut {
            panic!();
        }

        type Contact = Id;

        fn wrap_contact(&self, _id: Id) -> Self::Contact {
            panic!()
        }
    }

    #[test]
    fn ping() {
        let id = Id::from_hex("a0000000");
        let config = RoutingConfig {
            bucket_size: 2,
            bucket_replacement_size: 2,
            buckets_per_bit: 1,
            ..Default::default()
        };
        let mut tree = KTree::new(id, config);
        // Add similar entries to the same bucket,
        // since the bucket size is 2 it will overflow
        let mut contacter = MapContacter(Default::default());

        // closer bucket (0)
        assert_eq!(tree.insert(Id::from_hex("a0000001"), &mut contacter), true);
        // bucket 1
        assert_eq!(tree.insert(Id::from_hex("a0000010"), &mut contacter), true);
        assert_eq!(tree.insert(Id::from_hex("a0000011"), &mut contacter), true);
        // bucket 2
        assert_eq!(tree.insert(Id::from_hex("a0000100"), &mut contacter), true);
        assert_eq!(tree.insert(Id::from_hex("a0000101"), &mut contacter), true);
        assert!(contacter.inner().is_empty());
        assert_eq!(tree.insert(Id::from_hex("a0000110"), &mut contacter), true);// cache
        // should only ping bucket 2!
        assert_eq!(*contacter.inner(), HashMap::from([
            (Id::from_hex("a0000100"), 1usize),
            (Id::from_hex("a0000101"), 1),
        ]));
        // second cache entry SHOULD reping, it's the contacter job do deduplicate pings
        assert_eq!(tree.insert(Id::from_hex("a0000111"), &mut contacter), true);// cache 2
        assert_eq!(*contacter.inner(), HashMap::from([
            (Id::from_hex("a0000100"), 2usize),
            (Id::from_hex("a0000101"), 2),
        ]));

        let old_map = contacter.inner().clone();
        // client a100 disconnects, so a110 enters cache and we can insert a111 (well, in the cache)
        tree.remove(Id::from_hex("a0000100"));
        assert_eq!(*contacter.inner(), old_map);
        contacter.inner().clear();
        assert_eq!(tree.insert(Id::from_hex("a0000100"), &mut contacter), true);// cached
        assert_eq!(*contacter.inner(), HashMap::from([
            (Id::from_hex("a0000101"), 1),
            (Id::from_hex("a0000110"), 1),// promoted from cache and contacted
        ]));
    }

    #[test]
    fn multi_buckets_per_bit() {
        let id = Id::from_hex("a0000000");
        let config = RoutingConfig {
            bucket_size: 2,
            bucket_replacement_size: 1,
            buckets_per_bit: 2,
            ..Default::default()
        };
        let mut tree = KTree::new(id, config);

        // Add similar entries to the same bucket,
        // since the bucket size is 2 it will overflow
        let contacter = &mut IgnoreContacter;
        assert_eq!(tree.insert(Id::from_hex("b0000001"), contacter), true);
        assert_eq!(tree.insert(Id::from_hex("b0000010"), contacter), true);
        assert_eq!(tree.insert(Id::from_hex("b0000011"), contacter), true);// cache
        assert_eq!(tree.insert(Id::from_hex("b0000100"), contacter), false);
        // Add similar entries but with a different prefix
        //     bits _ xor a
        // a = 1010  0000
        // c = 1100  0110  (prefix: 10...)
        // e = 1110  0100  (prefix: 00...)
        // So c and e are at the same distance from a, but they have a different bit
        // they'll since be placed in different buckets
        assert_eq!(tree.insert(Id::from_hex("c0000001"), contacter), true);
        assert_eq!(tree.insert(Id::from_hex("c0000010"), contacter), true);
        assert_eq!(tree.insert(Id::from_hex("e0000001"), contacter), true);
        assert_eq!(tree.insert(Id::from_hex("e0000010"), contacter), true);
        assert_eq!(tree.insert(Id::from_hex("e0000011"), contacter), true);// cache
        assert_eq!(tree.insert(Id::from_hex("e0000100"), contacter), false);// full
    }
}
