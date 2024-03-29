use std::sync::Arc;

use tracing::instrument;
use warp::{cors, Filter};
use wdht_logic::KademliaDht;

use crate::{
    http_api::{ConnectRequest, ConnectResponse},
    wrtc::{WrtcSender, WrtcTransportError},
};

#[instrument(level = "error", name = "http_kademlia", skip_all, fields(kad_id = %dht.id()))]
async fn dht_connect_handle(
    dht: Arc<KademliaDht<WrtcSender>>,
    req: ConnectRequest,
) -> ConnectResponse<'static> {
    match dht
        .transport()
        .0
        .clone()
        .create_passive(req.id, req.offer)
        .await
    {
        Ok((answer, _)) => ConnectResponse::Ok { answer },
        Err(WrtcTransportError::ConnectionLimitReached) => ConnectResponse::Error {
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
        .with(
            cors()
                .allow_any_origin()
                .allow_method("POST")
                .allow_header("content-type")
                .build(),
        )
}
