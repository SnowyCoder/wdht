use std::num::NonZeroU64;


#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct SystemConfig {
    pub routing: RoutingConfig,
    pub storage: StorageConfig,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RoutingConfig {
    // Also called k in the original paper
    pub bucket_size: usize,

    // Size of the replacements cache (nodes known but not used
    // for routing unless older nodes go offline)
    pub bucket_replacement_size: usize,

    // This increases the routing table exponentially!!
    // (but decreases routing hops)
    pub buckets_per_bit: usize,

    // Max number of connected nodes
    pub max_connections: Option<NonZeroU64>,

    // Max number of nodes in routing table
    pub max_routing_count: Option<NonZeroU64>,
}

impl Default for RoutingConfig {
    fn default() -> Self {
        Self {
            bucket_size: 4,
            bucket_replacement_size: 2,
            buckets_per_bit: 1,
            max_connections: None,
            max_routing_count: None,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StorageConfig {
    // Maximum stored data size (in bytes)
    pub max_size: usize,

    // Maximum stored lifetime (in seconds)
    pub max_lifetime: u32,

    // Maximum number of stored entries
    pub max_entries: usize,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            max_size: 128 * 1024,  // 128 KiB
            max_lifetime: 60 * 60, // 1h
            max_entries: 1024,     // so 128Mib
        }
    }
}
