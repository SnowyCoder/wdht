use std::{error::Error, time::Duration, sync::Mutex, collections::HashMap};

use async_broadcast::RecvError;
use futures::future::join_all;
use rand::Rng;
use tokio::sync::oneshot;
use tracing::{info, instrument};
use reqwest::Url;
use wdht_logic::{transport::Contact, Id};
use wdht_wasync::{Orc, Weak, sleep, spawn};

use crate::{events::{TransportEvent, DisconnectReason, wait_for_shutdown}, wrtc::{Connections, WrtcTransportError, HandshakeError}, http_api::{ConnectRequest, ConnectResponse}};

const NANOS_PER_SEC: u32 = 1_000_000_000;
const MAX_EXPONENTIAL_BACKOFF_SECS: u64 = 5 * 60;// 5 minutes


async fn bootstrap_connect(url: Url, connector: Orc<Connections>) -> Result<Id, Box<dyn Error + Send + Sync>> {
    let self_id = connector.self_id;
    let (offer, answer_tx, mut connection_rx) = connector.create_active(None).await?;

    let client = reqwest::Client::new();
    let offer = ConnectRequest { id: self_id, offer };

    let r: ConnectResponse = client.post(url)
        .json(&offer)
        .send()
        .await?
        .json()
        .await?;

    let ans = match r {
        ConnectResponse::Ok { answer } => answer,
        ConnectResponse::Error { description } => return Err(description.into()),
    };
    if answer_tx.send(Ok(ans)).is_err() {
        return Err("Failed to send answer".into());
    }

    let res = connection_rx.recv().await
        .map_err(|_| "no receiver")?;

    let id = match res {
        Ok(x) => x.id(),
        Err(WrtcTransportError::Handshake(HandshakeError::IdConflict(id))) => id,
        Err(e) => Err(e)?,
    };

    info!("Connected to: {:?}", id);
    Ok(id)
}

#[instrument(name = "url_connector", skip_all, fields(url = url.to_string()))]
async fn bootstrap_exponential_backoff_connect(
    url: &Url,
    connector: Weak<Connections>,
    mut initial_connection_report: Option<oneshot::Sender<Result<(), Box<dyn Error + Send + Sync>>>>,
) -> Result<Id, ()> {
    let mut wait_secs = 1u64;

    while let Some(connector) = connector.upgrade() {
        let res = bootstrap_connect(url.clone(), connector).await;

        let id = match &res {
            Ok(id) => Some(*id),
            Err(_) => None
        };

        if let Some(reporter) = initial_connection_report.take() {
            // Ignore sending error if present
            let _ = reporter.send(res.map(|_| {}));
        } else if let Err(err) = res {
            info!("Error connecting to {url}: {err}");
        }

        if let Some(id) = id {
            return Ok(id);
        }
        wait_secs = (wait_secs * 2).min(MAX_EXPONENTIAL_BACKOFF_SECS);
        let wait_nanos = rand::thread_rng().gen_range(0..NANOS_PER_SEC);
        info!("Sleeping for {wait_secs}s before next attempt");
        sleep(Duration::new(wait_secs, wait_nanos)).await;
    }
    Err(())
}

pub async fn bootstrap_reconnector(
    urls: Vec<Url>,
    mut events: async_broadcast::Receiver<TransportEvent>,
    connector: Weak<Connections>,
    initial_connected: oneshot::Sender<()>
) {
    let id_to_index = Orc::new(Mutex::new(HashMap::new()));

    let inactive_recv = events.clone().deactivate();

    let spawn_connector = |url: Url, index: usize, conn_tx: Option<oneshot::Sender<Result<(), Box<dyn Error + Send + Sync>>>>| {
        let connector = connector.clone();
        let id_to_index = id_to_index.clone();
        let mut events = inactive_recv.activate_cloned();
        spawn(async move {
            let url = url;
            let id = tokio::select! {
                x = bootstrap_exponential_backoff_connect(&url, connector, conn_tx) => x,
                _ = wait_for_shutdown(&mut events) => return,
            };
            if let Ok(id) = id {
                id_to_index.lock().unwrap().insert(id, index);
            }
        });
    };

    join_all(
        urls.iter()
        .cloned()
        .enumerate()
        .map(|(index, url)| {
            let (conn_tx, conn_rx) = oneshot::channel();
            spawn_connector(url.clone(), index, Some(conn_tx));
            async move {
                match conn_rx.await {
                    Ok(Err(x)) => info!("Error connecting to '{url}': {x}"),
                    Err(_) => info!("Major failure in bootstrap connection"),
                    Ok(Ok(())) => {},
                };
            }
        })
    ).await;
    let _ = initial_connected.send(());

    loop {
        match events.recv().await {
            Ok(TransportEvent::Disconnect(id, DisconnectReason::ConnectionLost | DisconnectReason::SendFail | DisconnectReason::TimeoutExpired)) => {
                // If the disconencted ID previously was a bootstrap node, try to reconnect.
                if let Some(index) = id_to_index.lock().unwrap().get(&id).copied() {
                    info!("Connection to bootstrap node closed, retrying {}", urls[index]);
                    spawn_connector(urls[index].clone(), index, None);
                }
            },
            Ok(TransportEvent::Shutdown) |
            Err(RecvError::Closed) => break,// Closed
            Ok(_) => {}, // Ignore other events
            Err(RecvError::Overflowed(_)) => {},// Should never happen
        }
    }

}

#[cfg(test)]
mod tests {
    use wdht_logic::config::SystemConfig;

    use crate::{TransportConfig, create_dht, warp_filter::dht_connect, events::wait_for_event};

    use super::*;

    #[test_log::test(tokio::test)]
    async fn server_reconnect_test() {
        let config = SystemConfig::default();
        let transport_config = TransportConfig::default();

        let print_server_events = |mut srv_events: async_broadcast::Receiver<TransportEvent>| {
            tokio::spawn(async move {
                loop {
                    match srv_events.recv().await {
                        Err(_) => break,
                        Ok(x) => tracing::info!("SERVER EV: {x:?}"),
                    };
                }
            });
        };

        // Spawn server on random port
        let (srv, srv_events) = create_dht(config.clone(), transport_config.clone(), vec![] as Vec<Url>).await;
        let (srv_shutdown_tx, srv_shutdown_rx) = oneshot::channel();
        let (addr, srv) = warp::serve(dht_connect(srv)).bind_with_graceful_shutdown(([127, 0, 0, 1], 0), async {
            let _ = srv_shutdown_rx.await;
        });
        tokio::spawn(srv);
        print_server_events(srv_events);

        let (dht, mut events) = create_dht(config.clone(), transport_config.clone(), vec![format!("http://localhost:{}", addr.port()).parse().unwrap()] as Vec<Url>).await;
        assert!(dht.transport().connection_count() == 1);

        // Shutdown server
        srv_shutdown_tx.send(()).unwrap();

        // Connection is closed
        wait_for_event(&mut events, |e| matches!(e, Ok(TransportEvent::Disconnect(..)))).await;
        assert!(dht.transport().connected_count() == 0);

        // Reopen server
        let (srv, srv_events) = create_dht(config.clone(), transport_config.clone(), vec![] as Vec<Url>).await;
        let (srv_shutdown_tx, srv_shutdown_rx) = oneshot::channel();
        let (_addr, srv) = warp::serve(dht_connect(srv)).bind_with_graceful_shutdown(addr, async {
            let _ = srv_shutdown_rx.await;
        });
        tokio::spawn(srv);
        print_server_events(srv_events);

        wait_for_event(&mut events, |e| matches!(e, Ok(TransportEvent::Connect(_)))).await;

        // Reconnection test done
        srv_shutdown_tx.send(()).unwrap();
        wait_for_event(&mut events, |e| matches!(e, Ok(TransportEvent::Disconnect(..)))).await;
        drop(dht);
        wait_for_shutdown(&mut events).await;
    }
}
