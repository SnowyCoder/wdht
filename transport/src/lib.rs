#![feature(type_alias_impl_trait)]

use std::{sync::Arc, error::Error};

use futures::future::join_all;
use http_api::ConnectResponse;
use reqwest::IntoUrl;
use wdht_logic::{config::SystemConfig, KademliaDht, Id, search::BasicSearchOptions};
use wrtc::WrtcSender;

pub mod wrtc;
mod http_api;
#[cfg(feature = "warp_filter")]
pub mod warp_filter;


pub async fn create_dht<T>(config: SystemConfig, id: Id, bootstrap: T) -> Arc<KademliaDht<WrtcSender>>
    where T: IntoIterator, <T as IntoIterator>::Item: IntoUrl {
    let dht = wrtc::Connections::create(config, id);

    let connector = &dht.transport.0;

    join_all(bootstrap.into_iter()
        .map(|url| {
            let connector = connector.clone();
            async {
                let (offer, answer_tx, connection_rx) = connector.create_active().await?;

                let client = reqwest::Client::new();
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
                    return Ok(());
                }

                let _ = connection_rx.await;

                Ok::<_, Box<dyn Error>>(())
            }
        })
    ).await;
    let search_config = BasicSearchOptions {
        parallelism: 4,
    };
    let mut rng = rand::thread_rng();
    dht.bootstrap(search_config, &mut rng).await;

    dht
}
