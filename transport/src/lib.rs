#![feature(type_alias_impl_trait)]
use std::{error::Error, fmt::Display, time::Duration};

use futures::future::join_all;
use http_api::{ConnectRequest, ConnectResponse};
use reqwest::IntoUrl;
use tokio::sync::mpsc;
use tracing::{info, Instrument};
use wdht_wasync::{Orc, Weak, sleep, spawn};
use wdht_logic::{config::SystemConfig, search::BasicSearchOptions, Id, KademliaDht};
use wdht_wrtc::{RawConnection, RawChannel};
use wrtc::{Connections, WrtcSender};

mod config;
mod http_api;
#[cfg(feature = "warp")]
pub mod warp_filter;
mod shutdown;
pub mod wrtc;

pub use shutdown::{ShutdownSender, ShutdownReceiver};
pub use config::TransportConfig;

pub struct ChannelOpenEvent {
    pub id: Id,
    pub connection: RawConnection,
    pub channel: RawChannel,
}

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
    transport_config: TransportConfig,
    id: Id,
    bootstrap: T,
) -> (Orc<KademliaDht<WrtcSender>>, ShutdownSender, mpsc::Receiver<ChannelOpenEvent>)
where
    T: IntoIterator,
    <T as IntoIterator>::Item: IntoUrl + Clone + Display,
{
    let shutdown_sender = ShutdownSender::new();
    let (chan_tx, chan_rx) = mpsc::channel(4);
    let dht = wrtc::Connections::create(config, transport_config, id, chan_tx);
    // Run periodic cleaner
    let task = run_periodic_clean(Orc::downgrade(&dht), shutdown_sender.subscribe());
    spawn(task.instrument(tracing::info_span!("Periodic cleaner")));


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

    (dht, shutdown_sender, chan_rx)
}

async fn run_periodic_clean(kad: Weak<KademliaDht<WrtcSender>>, mut shutdown: async_broadcast::Receiver<()>) {
    loop {
        tokio::select! {
            _ = sleep(Duration::from_secs(10)) => {},
            _ = shutdown.recv() => break,
        }
        let k = match kad.upgrade() {
            Some(x) => x,
            None => break,// Program exited
        };
        k.periodic_run();
    }
}
