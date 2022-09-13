use std::{rc::Rc, time::Duration, cell::RefCell};

use js_sys::{Uint8Array, Array, Object, Reflect, Function};
use reqwest::Url;
use tracing::warn;
use wasm_bindgen::{prelude::*, JsCast};
use wasm_bindgen_futures::{future_to_promise, spawn_local};
use wdht::{create_dht, TransportConfig, events::TransportEvent, Dht, logic::{Id, config::SystemConfig, search::BasicSearchOptions, transport::{TopicEntry, Contact}, consts::ID_LEN}};
use wdht_crypto::sha2_hash;

// When the `wee_alloc` feature is enabled, use `wee_alloc` as the global
// allocator.
#[cfg(feature = "wee_alloc")]
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

const TOPIC_HASH_CONTEXT: &'static [u8] = b"wdht.topic";

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

#[wasm_bindgen(typescript_custom_section)]
const TS_TOPIC: &'static str = r#"
type Topic = string | {
    type: "topic" | "raw_id",
    key: string,
};

type BootstrapData = Array<string>;

type InsertPromise = Promise<number>;
type RemovePromise = Promise<number>;
type QueryPromise = Promise<Array<{
    data: Uint8Array,
    publisher: string,
}>>;
type ConnectToPromise = Promise<RTCPeerConnection>;
interface ChannelOpenEvent {
    peer_id: string,
    channel: RTCDataChannel,
    connection: RTCPeerConnection,
}
type ChannelOpenListener = (event: ChannelOpenEvent) => void;
"#;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "Topic")]
    pub type Topic;

    #[wasm_bindgen(typescript_type = "BootstrapData")]
    pub type BootstrapData;

    #[wasm_bindgen(typescript_type = "InsertPromise")]
    pub type InsertPromise;

    #[wasm_bindgen(typescript_type = "RemovePromise")]
    pub type RemovePromise;

    #[wasm_bindgen(typescript_type = "QueryPromise")]
    pub type QueryPromise;

    #[wasm_bindgen(typescript_type = "ConnectToPromise")]
    pub type ConnectToPromise;

    #[wasm_bindgen(typescript_type = "ChannelOpenListener")]
    pub type ChannelOpenListener;
}


#[wasm_bindgen]
pub struct WebDht {
    kad: Rc<Dht>,
    channel_open_listener: Rc<RefCell<Option<Function>>>,
}


#[wasm_bindgen]
impl WebDht {
    pub async fn create(bootstrap: BootstrapData) -> Result<WebDht, JsValue> {
        let mut config: SystemConfig = Default::default();
        config.routing.max_routing_count = Some(64.try_into().unwrap());
        let mut tconfig: TransportConfig = Default::default();
        tconfig.max_connections = Some(128.try_into().unwrap());
        // TODO: configuration from JS;
        tconfig.stun_servers = vec!["stun:stun.l.google.com:19302".to_owned(), "stun:stun.rossilorenzo.dev".to_owned()];

        let bootstrap: Vec<String> = bootstrap.into_serde().expect("Invalid bootstrap value");
        let bootstrap2: Vec<Url> = bootstrap.into_iter()
            .map(|x| x.parse())
            .collect::<Result<Vec<Url>, _>>()
            .map_err(|x| JsValue::from(format!("Invalid URL: {x}")))?;

        let (kad, mut events_rx) = create_dht(config, tconfig, bootstrap2).await;

        let listener: Rc<RefCell<Option<Function>>> = Rc::new(RefCell::new(None));
        let chan_listener = listener.clone();
        spawn_local(async move {
            loop {
                let ev = match events_rx.recv().await {
                    Ok(x) => x,
                    Err(_) => break,
                };
                match ev {
                    TransportEvent::ChannelOpen(chan) => {
                        if let Some(x) = chan_listener.borrow_mut().as_ref() {
                            let event = Object::new();
                            Reflect::set(&event, &"peer_id".into(), &chan.id.as_short_hex().into()).unwrap();
                            Reflect::set(&event, &"channel".into(), &chan.channel).unwrap();
                            Reflect::set(&event, &"connection".into(), &chan.connection).unwrap();
                            if let Err(x) = x.call1(
                                &JsValue::UNDEFINED,
                                &event,
                            ) {
                                warn!("open_data_channel handler returned error: {x:?}");
                            }
                        }
                    },
                    _ => {},
                }
            }
        });

        Ok(WebDht {
            kad,
            channel_open_listener: listener,
        })
    }

    #[wasm_bindgen(getter)]
    pub fn raw_connection_count(&self) -> u32 {
        self.kad.transport().connection_count() as u32
    }

    #[wasm_bindgen(getter)]
    pub fn connection_count(&self) -> u32 {
        self.kad.transport().connected_count() as u32
    }

    #[wasm_bindgen(getter)]
    pub fn id(&self) -> String {
        self.kad.id().as_short_hex()
    }

    pub fn insert(&self, topic: Topic, lifetime: f64, value: Option<Uint8Array>) -> InsertPromise {
        let kad = self.kad.clone();
        let fut = async move {
            let key = parse_topic(topic).await?;

            let lifetime = Duration::from_secs_f64(lifetime);

            Ok(kad.insert(key, lifetime, value.map_or(Vec::new(), |x| x.to_vec())).await
                .map(|x| (x as u32).into())
                .map_err(|x| x.to_string())?)
        };
        future_to_promise(fut).unchecked_into()
    }

    pub fn remove(&self, topic: Topic) -> RemovePromise {
        let kad = self.kad.clone();
        let fut = async move {
            let key = parse_topic(topic).await?;

            let count = kad.remove(key).await;
            Ok((count as u32).into())
        };
        future_to_promise(fut).unchecked_into()
    }

    pub fn query(&self, topic: Topic, limit: u32) -> QueryPromise {
        let kad = self.kad.clone();
        let fut = async move {
            let key = parse_topic(topic).await?;

            let search_options = BasicSearchOptions {
                parallelism: 4,
            };

            Ok(convert_entry_list(kad.query_value(key, limit, search_options).await).into())
        };
        future_to_promise(fut).unchecked_into()
    }

    pub fn connect_to(&self, key: String) -> ConnectToPromise {
        let kad = self.kad.clone();
        let fut = async move {
            let key: Id = key.parse().map_err(|e| format!("Failed to convert id: {e}"))?;

            let search_options = BasicSearchOptions {
                parallelism: 4,
            };
            let res = kad.query_nodes(key, search_options).await;
            if res.len() == 0 || res[0].id() != key {
                Err("Cannot find node")?;
            }
            let conn = res[0].raw_connection();
            Ok(conn.ok_or("Cannot open connection to self")?.into())
        };
        future_to_promise(fut).unchecked_into()
    }

    pub fn on_connection(&self, fun: Option<ChannelOpenListener>) {
        self.channel_open_listener.replace(fun.map(|x| x.unchecked_into()));
    }
}

async fn parse_topic(topic: Topic) -> Result<Id, JsValue> {
    if let Some(x) = topic.as_string() {
        return Ok(hash_key(x).await?);
    }
    if !topic.is_object() {
        return Err("Invalid topic type".into());
    }

    let get_or_invalid = |name: &str| {
        Reflect::get(&topic, &name.into())
            .ok()
            .and_then(|x| x.as_string())
            .ok_or_else(|| "Invalid topic type")
    };
    let ttype = get_or_invalid("type")?;
    let key = get_or_invalid("key")?;

    let res = match ttype.as_str() {
        "topic" => hash_key(key).await?,
        "raw_id" => key.parse::<Id>().map_err(|x| format!("Failed to parse raw id: {}", x.to_string()))?,
        _ => Err("Unrecognized topic type")?,
    };
    Ok(res)
}

async fn hash_key(key: String) -> Result<Id, &'static str> {
    if key.is_empty() {
        return Err("Key is empty");
    }
    let hash_data = sha2_hash(&TOPIC_HASH_CONTEXT, key.as_bytes()).await
        .map_err(|_| "Cryptographic error")?;
    let mut id = Id::ZERO;
    id.0[..ID_LEN].copy_from_slice(&hash_data[..ID_LEN]);
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
