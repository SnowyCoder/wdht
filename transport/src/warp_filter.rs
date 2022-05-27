use std::sync::Arc;

use tracing::instrument;
use warp::{cors, Filter};
use wdht_logic::KademliaDht;

use crate::{wrtc::{WrtcSender, async_wrtc::WrtcError}, http_api::{ConnectRequest, ConnectResponse}};

#[instrument(level = "error", name = "http_kademlia", skip_all, fields(kad_id = %dht.id()))]
async fn dht_connect_handle(dht: Arc<KademliaDht<WrtcSender>>, req: ConnectRequest) -> ConnectResponse<'static> {
    match dht.transport().0.clone().create_passive(req.offer).await {
        Ok(x) => ConnectResponse::Ok {
            answer: x,
        },
        Err(WrtcError::ConnectionLimitReached) => ConnectResponse::Error {
            description: "Connection limit reached".into(),
        },
        Err(_) => ConnectResponse::Error {
            description: "Error creating Wrtc connection".into(),
        },
    }
}

pub fn dht_connect(
    dht: Arc<KademliaDht<WrtcSender>>,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::path::end()
        .and(warp::post())
        .and(warp::any().map(move || dht.clone()))
        .and(warp::body::content_length_limit(1024 * 4))
        .and(warp::body::json())
        .then(dht_connect_handle)
        .map(|x| warp::reply::json(&x))
        .with(cors().allow_any_origin().build())
}
