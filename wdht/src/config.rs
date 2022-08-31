use std::num::NonZeroU64;

use serde::{Deserialize, Serialize};


#[derive(Clone, PartialEq, Eq, Debug, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct TransportConfig {
    pub stun_servers: Vec<String>,

    // Max number of connected nodes
    pub max_connections: Option<NonZeroU64>,
}
