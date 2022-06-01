mod utils;

use std::time::Duration;

use gloo_timers::future::TimeoutFuture;
use rand::{thread_rng, Rng};
use utils::set_panic_hook;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;
use wdht_logic::config::SystemConfig;
use wdht_transport::create_dht;

// When the `wee_alloc` feature is enabled, use `wee_alloc` as the global
// allocator.
#[cfg(feature = "wee_alloc")]
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[wasm_bindgen]
pub fn greet() {
    set_panic_hook();
    let s = format!("Hello, web-dht!, {}", 3 + 4);
    spawn_local(test_kad());
}

pub async fn test_kad() {
    let id = thread_rng().gen();

    let mut config: SystemConfig = Default::default();
    config.routing.max_connections = Some(128.try_into().unwrap());
    config.routing.max_routing_count = Some(64.try_into().unwrap());

    let kad = create_dht(config, id, ["http://localhost:3141"]).await;
    loop {
        kad.periodic_run();
        TimeoutFuture::new(10_000).await;
    }
}
