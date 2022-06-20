use std::{rc::Rc, time::Duration};

use js_sys::{Uint8Array, Promise, Array, Object, Reflect};
use rand::{thread_rng, Rng};
use sha3::{Shake128, digest::{Update, ExtendableOutput, XofReader}};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;
use wdht_logic::{config::SystemConfig, KademliaDht, Id, search::BasicSearchOptions, transport::{TopicEntry, Contact}};
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

    // Insertion

    async fn insert0(kad: Rc<KademliaDht<WrtcSender>>, key: Id, lifetime: f64, value: Option<Uint8Array>) -> Result<JsValue, JsValue> {
        let lifetime = Duration::from_secs_f64(lifetime);

        Ok(kad.insert(key, lifetime, value.map_or(Vec::new(), |x| x.to_vec())).await
            .map(|x| (x as u32).into())
            .map_err(|x| x.to_string())?)
    }

    pub fn insert_raw(&self, key_id: String, lifetime: f64, value: Option<Uint8Array>) -> Promise {
        let kad = self.kad.clone();
        let fut = async move {
            let key: Id = key_id.parse().map_err(|_| "Failed to convert id")?;

            Self::insert0(kad, key, lifetime, value).await
        };
        future_to_promise(fut)
    }

    pub fn insert(&self, key: String, lifetime: f64, value: Option<Uint8Array>) -> Promise {
        let kad = self.kad.clone();
        let fut = async move {
            let key = hash_key(key)?;

            Self::insert0(kad, key, lifetime, value).await
        };
        future_to_promise(fut)
    }

    // Removal

    async fn remove0(kad: Rc<KademliaDht<WrtcSender>>, key: Id) -> Result<JsValue, JsValue> {
        let count = kad.remove(key).await;
        Ok((count as u32).into())
    }

    pub fn remove_raw(&self, key: String) -> Promise {
        let kad = self.kad.clone();
        let fut = async move {
            let key: Id = key.parse().map_err(|_| "Failed to convert id")?;
            Self::remove0(kad, key).await
        };
        future_to_promise(fut)
    }

    pub fn remove(&self, key: String) -> Promise {
        let kad = self.kad.clone();
        let fut = async move {
            let key = hash_key(key)?;
            Self::remove0(kad, key).await
        };
        future_to_promise(fut)
    }

    // Querying

    async fn query0(kad: Rc<KademliaDht<WrtcSender>>, key: Id, limit: u32) -> Result<JsValue, JsValue> {
        let search_options = BasicSearchOptions {
            parallelism: 4,
        };

        Ok(convert_entry_list(kad.query_value(key, limit, search_options).await).into())
    }

    pub fn query_raw(&self, key: String, limit: u32) -> Promise {
        let kad = self.kad.clone();
        let fut = async move {
            let key: Id = key.parse().map_err(|_| "Failed to convert id")?;

            Self::query0(kad, key, limit).await
        };
        future_to_promise(fut)
    }

    pub fn query(&self, key: String, limit: u32) -> Promise {
        let kad = self.kad.clone();
        let fut = async move {
            let key = hash_key(key)?;

            Self::query0(kad, key, limit).await
        };
        future_to_promise(fut)
    }

    pub fn connect_to(&self, key: String) -> Promise {
        let kad = self.kad.clone();
        let fut = async move {
            let key: Id = key.parse().map_err(|_| "Failed to convert id")?;

            let search_options = BasicSearchOptions {
                parallelism: 4,
            };
            let res = kad.query_nodes(key, search_options).await;
            if res[0].id() != key {
                Err("Cannot find node")?;
            }
            let conn = res[0].raw_connection();
            Ok(conn.ok_or("Cannot open connection to self")?.into())
        };
        future_to_promise(fut)
    }
}

fn hash_key(key: String) -> Result<Id, &'static str> {
    if key.is_empty() {
        return Err("Key is empty");
    }
    let mut hasher = Shake128::default();
    hasher.update(b"kad_query");
    hasher.update(key.as_bytes());
    let mut reader = hasher.finalize_xof();
    let mut id = Id::ZERO;
    reader.read(&mut id.0);
    Ok(id)
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
