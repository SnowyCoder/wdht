use std::sync::Arc;

use either::Either;
use tracing::instrument;
use warp::{Filter, Reply, reply::html, cors};
use wdht::{logic::KademliaDht, wrtc::WrtcSender};

#[instrument(level = "error", name = "http_kademlia_query", skip_all, fields(kad_id = %dht.id()))]
fn dht_query_handle(dht: Arc<KademliaDht<WrtcSender>>) -> impl Reply {
    let transport = dht.transport();

    let id = dht.id().as_short_hex();
    let config = transport.config();
    let connections = transport.connection_count();
    let connections_limit = match config.max_connections {
      Some(x) => Either::Left(x.get()),
      None => Either::Right("inf"),
    };
    let connected = transport.connected_count();
    let half_closed = transport.half_closed_count();

    let body = format!(r#"
    <html>
    <head>
      <title>WebDHT</title>
    </head>
    <body>
      <h1>Welcome to WebDHT!</h1>
      <h3><a href="https://github.com/SnowyCoder/wdht">Visit us</a> to find out more</h3>
      <h4>
        Id: {id}<br>
        Connections: {connections}/{connections_limit}<br>
        Connected: {connected}<br>
        Half closed: {half_closed}
      </h4>
    </body>
    </html>
    "#);

    html(body)
}

pub fn dht_query(
    dht: Arc<KademliaDht<WrtcSender>>,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::path::end()
        .and(warp::get())
        .and(warp::any().map(move || dht.clone()))
        .map(dht_query_handle)
        .with(
            cors()
                .allow_any_origin()
                .allow_method("GET")
                .build(),
        )
}
