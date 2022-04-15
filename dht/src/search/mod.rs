use std::{collections::HashSet, cmp::Reverse};

use futures::stream::FuturesUnordered;
use futures::prelude::*;
use log::{warn, debug};

use crate::{contacter::{Transport, Response, TransportError, Request}, Id, KademliaDht};

#[derive(Clone, Debug)]
pub struct BasicSearchOptions {
    // Also called alpha in the original paper
    // n. of nodes searched in parallel
    pub parallelism: u32,
}


pub struct BasicSearch<'a, T: Transport> {
    dht: &'a KademliaDht<T>,
    options: BasicSearchOptions,
    target_id: Id,
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum QueryState {
    Waiting,
    Querying,
    Queried,
}

impl<'a, T: Transport> BasicSearch<'a, T> {
    pub fn create(dht: &'a KademliaDht<T>, options: BasicSearchOptions, target_id: Id) -> Self {
        Self {
            dht,
            options,
            target_id,
        }
    }

    fn start_query(&self, queries: &mut [(QueryState, Id)])
            -> Option<impl Future<Output=(Id, Result<Response, TransportError>)>> {
        let to = queries.iter_mut()
            .filter(|x| x.0 == QueryState::Waiting)
            .next();
        // Note: picking the first entry will always pick the closest node since they're
        // always ordered by increasing distance (or decreased xored leading zeroes).

        let to = match to {
            Some(x) => x,
            None => return None,
        };

        to.0 = QueryState::Querying;
        let used_id = to.1.clone();

        let fut = self.dht.transport().send(&to.1, Request::FindNodes(self.target_id.clone()));
        Some(fut.map(|x| (used_id, x)))
    }

    fn sort_bucket(&self, bucket: &mut [(QueryState, Id)]) {
        // Sort with leading zeros in descending order:
        // the first entries will have MORE leading zeros (so they'll be closer)
        bucket.sort_by_key(|x| Reverse(x.1.xor(&self.target_id).leading_zeros()));
    }

    pub async fn search(&self, first_bucket: Vec<Id>) -> Vec<Id> {
        let bucket_size = self.dht.config().routing.bucket_size;
        let parallelism = self.options.parallelism;

        let mut queried: HashSet<Id> = first_bucket.iter().cloned().collect();

        // Must always be of bucket length, similar to a window of the closest Ids that we know
        let mut to_query: Vec<(QueryState, Id)> = first_bucket.into_iter()
                .map(|x| (QueryState::Waiting, x))
                .collect();
        self.sort_bucket(&mut to_query);

        let pending: FuturesUnordered<_> = (0..parallelism).into_iter()
                .filter_map(|_| self.start_query(&mut to_query))
                .collect();

        let mut available_futures = parallelism - pending.len() as u32;

        tokio::pin!(pending);
        while let Some((id, res)) = pending.next().await {
            available_futures += 1;// 1 space available again
            let entry = to_query.iter_mut().find(|x| x.1 == id);

            match entry {
                Some(entry) => {
                    entry.0 = QueryState::Queried;
                },
                None => {
                    // We have requested response from a peer that fell out of the
                    // request window, we could ignore the result but it would be more
                    // efficient to check the answer for additional details
                }
            }
            use Response::*;
            match res {
                Err(x) => {
                    debug!("Error requesting from {:?}: {}", id, x);
                },
                Ok(FoundNodes(nodes)) => {
                    // found other nodes
                    to_query.extend(nodes.iter()
                        .cloned()// Transform &Id to Id
                        // Only take non-previously queried nodes
                        .filter(|x| queried.insert(x.clone()))
                        .map(|x| (QueryState::Waiting, x)));
                    self.sort_bucket(&mut to_query);
                    to_query.truncate(bucket_size);
                    while available_futures > 0 {
                        match self.start_query(&mut to_query) {
                            None => break,
                            Some(x) => pending.push(x),
                        };
                        available_futures -= 1;
                    }
                },
                Ok(FoundData(_)) => todo!(),// TODO: handle data retrieval
                Ok(Error) => warn!("Node {:?} returned error", id),
                Ok(x) => warn!("Node {:?} returned invalid response: {:?}", id, x),
            }

            if to_query.iter().all(|x| x.0 == QueryState::Queried) {
                // All of the closest nodes responded, other queried nodes should not know any
                // other closer node (if they know)
                break;
            }
        }
        to_query.into_iter()
            .map(|x| x.1)
            .collect()
    }
}