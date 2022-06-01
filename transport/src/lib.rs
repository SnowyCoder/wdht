#![feature(type_alias_impl_trait)]
use std::{error::Error, fmt::Display};

use futures::future::join_all;
use http_api::{ConnectRequest, ConnectResponse};
use reqwest::IntoUrl;
use tracing::info;
use wasync::Orc;
use wdht_logic::{config::SystemConfig, search::BasicSearchOptions, Id, KademliaDht};
use wrtc::{Connections, WrtcSender};

mod http_api;
#[cfg(feature = "warp")]
pub mod warp_filter;
pub mod wasync;
pub mod wrtc;

async fn bootstrap_connect<T: IntoUrl>(
    url: T,
    connector: Orc<Connections>,
) -> Result<(), Box<dyn Error>> {
    let self_id = connector.self_id;
    let (offer, answer_tx, mut connection_rx) = connector.create_active(None).await?;

    let client = reqwest::Client::new();
    let offer = ConnectRequest { id: self_id, offer };
    let r: ConnectResponse = client.post(url).json(&offer).send().await?.json().await?;

    let ans = match r {
        ConnectResponse::Ok { answer } => answer,
        ConnectResponse::Error { description } => return Err(description.into()),
    };
    if answer_tx.send(Ok(ans)).is_err() {
        return Err("Failed to send answer".into());
    }

    let x = connection_rx.recv().await?;
    info!("Connected to: {:?}", x);
    Ok(())
}

pub async fn create_dht<T>(
    config: SystemConfig,
    id: Id,
    bootstrap: T,
) -> Orc<KademliaDht<WrtcSender>>
where
    T: IntoIterator,
    <T as IntoIterator>::Item: IntoUrl + Clone + Display,
{
    let dht = wrtc::Connections::create(config, id);

    let connector = &dht.transport.0;

    join_all(bootstrap.into_iter().map(|url| async move {
        if let Err(x) = bootstrap_connect(url.clone(), connector.clone()).await {
            info!("Error connecting to '{}': {}", url, x);
        }
    }))
    .await;
    info!("Finished connecting to bootstrap nodes");
    let search_config = BasicSearchOptions { parallelism: 4 };
    let mut rng = rand::thread_rng();
    dht.bootstrap(search_config, &mut rng).await;
    info!("Bootstrap finished correctly");

    dht
}
