use std::{collections::HashSet, cmp::Reverse, iter};

use futures::stream::FuturesUnordered;
use futures::prelude::*;
use tracing::{warn, debug, instrument};

use crate::{transport::{TransportSender, RawResponse, TransportError, Request, Contact}, Id, KademliaDht};

#[derive(Clone, Debug)]
pub struct BasicSearchOptions {
    // Also called alpha in the original paper
    // n. of nodes searched in parallel
    pub parallelism: u32,
}

/// Basic search, taken from the Kademlia original paper
/// Works by keeping a bucket-size window of the closest node to the target id.
/// When a new node is discovered it's inserted ONLY IF it's in the k-closest ids.
/// Of these nodes only alpha are queried at a time, alpha is called the parallelism parameter.
/// 1. Search in the current node for the k closest nodes to target_id
/// 2. Search in alpha of the nodes in the window (not queried yet)
/// 3. If a node returns other nodes, try to put them in the window
/// 4. If all of the nodes in the closest window have been queried then there
///    are no closer nodes, finish the search.
///
pub struct BasicSearch<'a, T: TransportSender> {
    dht: &'a KademliaDht<T>,
    options: BasicSearchOptions,
    search_type: SearchType,
    target_id: Id,
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum QueryState {
    Waiting,
    Querying,
    Queried,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SearchType {
    Nodes,
    Data,
}

pub enum SearchResult<C: Contact> {
    CloserNodes(Vec<C>),
    DataFound(Vec<u8>),
}

impl<'a, T: TransportSender> BasicSearch<'a, T> {
    pub fn create(dht: &'a KademliaDht<T>, options: BasicSearchOptions, search_type: SearchType, target_id: Id) -> Self {
        Self {
            dht,
            options,
            search_type,
            target_id,
        }
    }

    fn start_query(&self, queries: &mut [(QueryState, T::Contact)])
            -> Option<impl Future<Output=(Id, Result<RawResponse<T::Contact>, TransportError>)>> {
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
        let used_id = *to.1.id();

        let message = match self.search_type {
            SearchType::Nodes => Request::FindNodes(self.target_id.clone()),
            SearchType::Data => Request::FindData(self.target_id.clone()),
        };

        let fut = self.dht.transport().send(&used_id, message);
        Some(fut.map(move |x| (used_id, x)))
    }

    fn sort_bucket(&self, bucket: &mut [(QueryState, T::Contact)]) {
        // Sort with leading zeros in descending order:
        // the first entries will have MORE leading zeros (so they'll be closer)
        bucket.sort_by_key(|x| Reverse((*x.1.id() ^ *self.target_id.id()).leading_zeros()));
    }

    #[instrument(skip_all)]
    pub async fn search(&self, first_bucket: Vec<T::Contact>) -> SearchResult<T::Contact> {
        let bucket_size = self.dht.config().routing.bucket_size;
        let parallelism = self.options.parallelism;

        let mut queried: HashSet<Id> = first_bucket.iter().map(|x| x.id().clone()).collect();
        queried.insert(self.dht.id().clone());// We already queried ourself
        debug!("First bucket: {:?}", first_bucket);

        let self_contact = self.dht.transport().wrap_contact(self.dht.id().clone());
        // Must always be of bucket length, similar to a window of the closest Ids that we know
        let mut to_query: Vec<(QueryState, T::Contact)> = first_bucket.into_iter()
                .map(|x| (QueryState::Waiting, x))
                .chain(iter::once((QueryState::Queried, self_contact)))
                .collect();
        self.sort_bucket(&mut to_query);

        let pending: FuturesUnordered<_> = (0..parallelism).into_iter()
                .filter_map(|_| self.start_query(&mut to_query))
                .collect();

        let mut available_futures = parallelism - pending.len() as u32;

        tokio::pin!(pending);
        while let Some((id, res)) = pending.next().await {
            available_futures += 1;// 1 space available again
            let entry = to_query.iter_mut().find(|x| x.1.id() == &id);

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
            debug!("Response from {:?}: {:?}", id, res);
            use RawResponse::*;
            match res {
                Err(x) => {
                    debug!("Error requesting from {:?}: {}", id, x);
                },
                Ok(FoundNodes(nodes)) => {
                    // found other nodes
                    to_query.extend(nodes.iter()
                        .cloned()// Transform &Id to Id
                        // Only take non-previously queried nodes
                        .filter(|x| queried.insert(x.id().clone()))
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
                Ok(FoundData(x)) => {
                    if self.search_type == SearchType::Data {
                        // TODO: handle multiple datas?
                        // If multiple data entries are available then we might need every response
                        // (at least, we might need the full response of the closest bucket)
                        return SearchResult::DataFound(x);
                    } else {
                        warn!("Node {:?} returned data even if only nodes are requested", id)
                    }
                },
                Ok(Error) => warn!("Node {:?} returned error", id),
                Ok(x) => warn!("Node {:?} returned invalid response: {:?}", id, x),
            }

            if to_query.iter().all(|x| x.0 == QueryState::Queried) {
                // All of the closest nodes responded, other queried nodes should not know any
                // other closer node
                break;
            }
        }

        let nodes = to_query.into_iter()
            .map(|x| x.1)
            .collect();
        SearchResult::CloserNodes(nodes)
    }
}
