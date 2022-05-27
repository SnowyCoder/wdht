#![feature(type_alias_impl_trait)]

use std::{sync::Arc, error::Error, fmt::Display};

use futures::future::join_all;
use http_api::{ConnectResponse, ConnectRequest};
use tracing::info;
use reqwest::IntoUrl;
use wdht_logic::{config::SystemConfig, KademliaDht, Id, search::BasicSearchOptions};
use wrtc::{WrtcSender, Connections};

pub mod wrtc;
mod http_api;
#[cfg(feature = "warp_filter")]
pub mod warp_filter;


async fn bootstrap_connect<T: IntoUrl>(url: T, connector: Arc<Connections>) -> Result<(), Box<dyn Error>> {
    let (offer, answer_tx, connection_rx) = connector.create_active().await?;

    let client = reqwest::Client::new();
    let offer = ConnectRequest {
        offer
    };
    let r: ConnectResponse = client.post(url)
        .json(&offer)
        .send()
        .await?
        .json().await?;

    let ans = match r {
        ConnectResponse::Ok { answer } => answer,
        ConnectResponse::Error { description } => return Err(description.into()),
    };
    if let Err(_) = answer_tx.send(ans) {
        return Err("Failed to send answer".into());
    }

    let x = connection_rx.await?;
    info!("Connected to: {:?}", x);
    Ok(())
}

pub async fn create_dht<T>(config: SystemConfig, id: Id, bootstrap: T) -> Arc<KademliaDht<WrtcSender>>
    where T: IntoIterator, <T as IntoIterator>::Item: IntoUrl + Clone + Display {
    let dht = wrtc::Connections::create(config, id);

    let connector = &dht.transport.0;

    join_all(bootstrap.into_iter()
        .map(|url| async move {
            if let Err(x) = bootstrap_connect(url.clone(), connector.clone()).await {
                info!("Error connecting to '{}': {}", url, x);
            }
        })
    ).await;
    info!("Finished connecting to bootstrap nodes");
    let search_config = BasicSearchOptions {
        parallelism: 4,
    };
    let mut rng = rand::thread_rng();
    dht.bootstrap(search_config, &mut rng).await;
    info!("Bootstrap finished correctly");

    dht
}
