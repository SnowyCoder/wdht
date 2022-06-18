use std::{rc::Rc, time::Duration};

use js_sys::{Uint8Array, Promise, Array, Object, Reflect};
use rand::{thread_rng, Rng};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;
use wdht_logic::{config::SystemConfig, KademliaDht, Id, search::BasicSearchOptions, transport::TopicEntry};
use wdht_transport::{create_dht, wrtc::WrtcSender, ShutdownSender};

// When the `wee_alloc` feature is enabled, use `wee_alloc` as the global
// allocator.
#[cfg(feature = "wee_alloc")]
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[wasm_bindgen(start)]
pub fn on_start() {
    // When the `console_error_panic_hook` feature is enabled, we can call the
    // `set_panic_hook` function at least once during initialization, and then
    // we will get better error messages if our code ever panics.
    //
    // For more details see
    // https://github.com/rustwasm/console_error_panic_hook#readme
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();

    let _ = tracing_wasm::try_set_as_global_default();
}

#[wasm_bindgen]
pub struct WebDht {
    kad: Rc<KademliaDht<WrtcSender>>,
    _shutdown: ShutdownSender,
}

#[wasm_bindgen]
impl WebDht {
    #[wasm_bindgen(constructor)]
    pub async fn new(bootstrap: JsValue) -> Self {
        let id = thread_rng().gen();

        let mut config: SystemConfig = Default::default();
        config.routing.max_connections = Some(128.try_into().unwrap());
        config.routing.max_routing_count = Some(64.try_into().unwrap());

        let bootstrap: Vec<String> = bootstrap.into_serde().expect("Invalid bootstrap value");

        let (kad, shutdown) = create_dht(config, id, bootstrap).await;

        WebDht {
            kad,
            _shutdown: shutdown,
        }
    }

    pub fn insert(&self, key: String, lifetime: f64, value: Uint8Array) -> Promise {
        let kad = self.kad.clone();
        let fut = async move {
            let key: Id = key.parse().map_err(|_| "Failed to convert id")?;
            let lifetime = Duration::from_secs_f64(lifetime);

            Ok(kad.insert(key, lifetime, value.to_vec()).await
                .map(|x| (x as u32).into())
                .map_err(|x| x.to_string())?)
        };
        future_to_promise(fut)
    }

    pub fn remove(&self, key: String) -> Promise {
        let kad = self.kad.clone();
        let fut = async move {
            let key: Id = key.parse().map_err(|_| "Failed to convert id")?;
            let count = kad.remove(key).await;
            Ok((count as u32).into())
        };
        future_to_promise(fut)
    }

    pub fn query(&self, key: String, limit: u32) -> Promise {
        let kad = self.kad.clone();
        let fut = async move {
            let key: Id = key.parse().map_err(|_| "Failed to convert id")?;

            let search_options = BasicSearchOptions {
                parallelism: 4,
            };

            Ok(kad.query_value(key, limit, search_options).await
                .map(convert_entry_list)
                .into())
        };
        future_to_promise(fut)
    }
}

fn convert_entry_list(entries: Vec<TopicEntry>) -> Array {
    entries.into_iter().map(convert_entry).collect()
}

fn convert_entry(entry: TopicEntry) -> Object {
    let hex = entry.publisher.as_short_hex();
    let data = Uint8Array::from(entry.data.as_slice());
    let res = Object::new();
    Reflect::set(&res, &"data".into(), &data).unwrap();
    Reflect::set(&res, &"publisher".into(), &hex.into()).unwrap();
    return res
}
