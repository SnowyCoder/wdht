use std::{sync::{Mutex, Arc}, collections::{HashSet, HashMap}};

use wdht_logic::{KademliaDht, Id};

use crate::util::ArcKey;

use self::conn::{MyTransportSender, WrtcConnection};

mod conn;
mod protocol;


struct Connections {
    pub dht: KademliaDht<MyTransportSender>,
    pub connections: Mutex<HashSet<ArcKey<WrtcConnection>>>,
    pub connections_by_id: Mutex<HashMap<Id, Arc<WrtcConnection>>>,
}

impl Connections {
    pub fn create(&self) {


    }
}
