use std::{net::SocketAddr, num::NonZeroU64, sync::Arc, time::Duration};

use rand::{thread_rng, Rng};
use reqwest::Url;
use tracing::{info, span, Instrument, Level};
use tracing_subscriber::{prelude::*, EnvFilter};
use wdht_logic::{config::SystemConfig, Id, KademliaDht};
use wdht_transport::{create_dht, warp_filter::dht_connect, wrtc::WrtcSender, ShutdownSender, TransportConfig};

use clap::{Args, Parser, Subcommand};
use itertools::Itertools;

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

    /// Node ID (default is random)
    #[clap(long)]
    id: Option<Id>,

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

    /// Nodes ID (default is random)
    #[clap(long, multiple_values(true), multiple_occurrences(false))]
    ids: Vec<Id>,
}

#[derive(Subcommand, Debug)]
enum Command {
    Server(ServerArgs),
    Client(ClientArgs),
}

#[tokio::main]
async fn main() {
    let console_layer = console_subscriber::ConsoleLayer::builder().spawn();
    let fmt_layer = tracing_subscriber::fmt::layer();
    tracing_subscriber::registry()
        .with(console_layer)
        .with(fmt_layer.with_filter(EnvFilter::from_default_env()))
        .init();

    let args = CliArgs::parse();

    match args.command {
        Command::Client(a) => start_client(&a).await,
        Command::Server(a) => start_server(&a).await,
    }
}

async fn start_kademlia(args: &CommonArgs, id: Option<Id>) -> (Arc<KademliaDht<WrtcSender>>, ShutdownSender) {
    let id = id.unwrap_or_else(|| thread_rng().gen());

    let mut config: SystemConfig = Default::default();
    config.routing.max_routing_count = args.max_routing_count;
    let mut tconfig: TransportConfig = Default::default();
    tconfig.max_connections = args.max_connections;
    tconfig.stun_servers = args.stun_servers.iter().map(|x| x.to_string()).collect();

    let span = span!(Level::INFO, "create_dht", %id);
    let t = create_dht(config, tconfig, id, args.bootstrap.clone())
        .instrument(span)
        .await;

    (t.0, t.1)
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
            .zip_longest(args.ids.clone())
            .enumerate()
            .map(|(i, x)| async move {
                tokio::time::sleep(Duration::from_secs(5 * i as u64)).await;
                info!("Starting client {i}");
                start_kademlia(&args.common, x.right()).await
        }),
    )
    .await;
    info!("Clients started");

    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen to ctrl-c");
}

async fn start_server(args: &ServerArgs) {
    let (kad, _shutdown) = start_kademlia(&args.common, args.id).await;
    info!("Starting up server");

    warp::serve(dht_connect(kad)).run(args.bind).await;
}
