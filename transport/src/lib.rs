#![feature(type_alias_impl_trait)]
use std::{fmt::Display, time::Duration};

use async_broadcast::broadcast;
use events::TransportEvent;
use reqwest::Url;
use tokio::sync::oneshot;
use tracing::{info, Instrument, warn};
use wdht_wasync::{Orc, Weak, sleep, spawn};
use wdht_logic::{config::SystemConfig, search::BasicSearchOptions, KademliaDht};
use wrtc::WrtcSender;

mod identity;
mod config;
pub mod events;
mod http_api;
mod reconnect;
mod serde;
#[cfg(feature = "warp")]
pub mod warp_filter;
pub mod wrtc;

pub use config::TransportConfig;

use crate::events::wait_for_shutdown;

pub async fn create_dht<T, I>(
    config: SystemConfig,
    transport_config: TransportConfig,
    bootstrap: T,
) -> (Orc<KademliaDht<WrtcSender>>, async_broadcast::Receiver<TransportEvent>)
where
    T: IntoIterator<Item = I>,
    I: TryInto<Url>,
    <I as TryInto<Url>>::Error: Display,
{
    let (events_tx, events_rx) = broadcast(64);
    let dht = wrtc::Connections::create(config, transport_config, events_tx).await;
    // Run periodic cleaner
    let task = run_periodic_clean(Orc::downgrade(&dht), events_rx.clone());
    spawn(task.instrument(tracing::info_span!("Periodic cleaner")));


    let urls: Vec<_> = bootstrap.into_iter()
        .enumerate()
        .filter_map(|(i, x)| {
            match x.try_into() {
                Ok(x) => Some(x),
                Err(e) => {
                    warn!("Error connecting to bootstrap {i}: {e}");
                    None
                },
            }
        })
        .collect();

    let connector = &dht.transport.0;
    let (bootstrap_connect_tx, bootstrap_connect_rx) = oneshot::channel();
    let reconnector = reconnect::bootstrap_reconnector(urls, events_rx.clone(), Orc::downgrade(connector), bootstrap_connect_tx);
    spawn(reconnector.instrument(tracing::info_span!("Bootstrap reconnector")));
    bootstrap_connect_rx.await.expect("Major failure while connecting to bootstrap nodes");

    info!("Finished connecting to bootstrap nodes");
    let search_config = BasicSearchOptions { parallelism: 4 };
    let mut rng = rand::thread_rng();
    dht.bootstrap(search_config, &mut rng).await;
    info!("Bootstrap finished correctly");

    (dht, events_rx)
}

async fn run_periodic_clean(kad: Weak<KademliaDht<WrtcSender>>, mut events: async_broadcast::Receiver<TransportEvent>) {
    loop {
        tokio::select! {
            _ = sleep(Duration::from_secs(10)) => {},
            _ = wait_for_shutdown(&mut events) => break,
        }
        let k = match kad.upgrade() {
            Some(x) => x,
            None => break,// Program exited
        };
        k.periodic_run();
    }
}

#[cfg(test)]
mod tests {
    use wdht_logic::config::SystemConfig;

    use crate::{create_dht, TransportConfig, events::TransportEvent};

    #[test_log::test(tokio::test)]
    async fn drop_test() {
        let config = SystemConfig::default();
        let tconfig = TransportConfig::default();
        let (dht, mut events) = create_dht(config, tconfig, vec![] as Vec<&'static str>).await;
        drop(dht);
        assert!(matches!(events.recv().await, Ok(TransportEvent::Shutdown)));
    }
}
