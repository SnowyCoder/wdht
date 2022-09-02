use std::{net::SocketAddr, num::NonZeroU64, sync::Arc, time::Duration};

use reqwest::Url;
use tracing::{info, span, Instrument, Level};
use tracing_subscriber::{prelude::*, EnvFilter};
use wdht::{create_dht, warp_filter::dht_connect, TransportConfig, Dht, logic::config::SystemConfig};

use clap::{Args, Parser, Subcommand};

/// Web-dht server (and tester client)
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct CliArgs {
    #[clap(subcommand)]
    command: Command,
}

#[derive(Args, Debug)]
struct CommonArgs {
    /// HTTP Bootstrap servers
    #[clap(long)]
    bootstrap: Vec<Url>,

    /// Maximum number of routing table connections
    #[clap(long)]
    max_routing_count: Option<NonZeroU64>,

    /// Maximum number of transport connections
    #[clap(long)]
    max_connections: Option<NonZeroU64>,

    /// STUN Servers
    #[clap(long)]
    stun_servers: Vec<Url>,
}

#[derive(Parser, Debug)]
struct ServerArgs {
    #[clap(flatten)]
    common: CommonArgs,

    /// Bind address
    #[clap(long, default_value = "127.0.0.1:3141")]
    bind: SocketAddr,
}

#[derive(Parser, Debug)]
struct ClientArgs {
    #[clap(flatten)]
    common: CommonArgs,

    #[clap(short = 'n', long, default_value = "1")]
    count: u32,
}

#[derive(Subcommand, Debug)]
enum Command {
    Server(ServerArgs),
    Client(ClientArgs),
}

#[tokio::main]
async fn main() {
    let fmt_layer = tracing_subscriber::fmt::layer();
    tracing_subscriber::registry()
        .with(fmt_layer.with_filter(EnvFilter::from_default_env()))
        .init();

    let args = CliArgs::parse();

    match args.command {
        Command::Client(a) => start_client(&a).await,
        Command::Server(a) => start_server(&a).await,
    }
}

async fn start_kademlia(args: &CommonArgs) -> Arc<Dht> {
    let mut config: SystemConfig = Default::default();
    config.routing.max_routing_count = args.max_routing_count;
    let mut tconfig: TransportConfig = Default::default();
    tconfig.max_connections = args.max_connections;
    tconfig.stun_servers = args.stun_servers.iter().map(|x| x.to_string()).collect();

    let span = span!(Level::INFO, "create_dht");
    let t = create_dht(config, tconfig, args.bootstrap.clone())
        .instrument(span)
        .await;

    t.0
}

async fn start_client(args: &ClientArgs) {
    /*let mut kads = Vec::new();
    for i in 0..args.count {
        println!("Starting: {i}");
        kads.push(start_kademlia(&args.common).await);
    }*/
    let _kads = futures::future::join_all(
        (0..args.count)
            .into_iter()
            .map(|i| async move {
                tokio::time::sleep(Duration::from_secs(5 * i as u64)).await;
                info!("Starting client {i}");
                start_kademlia(&args.common).await
        }),
    )
    .await;
    info!("Clients started");

    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen to ctrl-c");
}

async fn start_server(args: &ServerArgs) {
    let kad = start_kademlia(&args.common).await;
    info!("Starting up server");

    warp::serve(dht_connect(kad)).run(args.bind).await;
}
