use std::{net::SocketAddr, sync::Arc, num::NonZeroU64};

use log::info;
use rand::{thread_rng, Rng};
use reqwest::Url;
use wdht_logic::{Id, KademliaDht, config::SystemConfig};
use wdht_transport::{warp_filter::dht_connect, create_dht, wrtc::WrtcSender};

use clap::{Parser, Subcommand, Args};

/// Web-dht server (and tester client)
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct CliArgs {
    #[clap(subcommand)]
    command: Command,
}

#[derive(Args, Debug)]
struct CommonArgs {
    /// Node ID (default is random)
    #[clap(long)]
    id: Option<Id>,

    /// HTTP Bootstrap servers
    #[clap(long)]
    bootstrap: Vec<Url>,

    /// Maximum number of routing table connections
    #[clap(long)]
    max_routing_count: Option<NonZeroU64>,

    /// Maximum number of transport connections
    #[clap(long)]
    max_connections: Option<NonZeroU64>,
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
}

#[derive(Subcommand, Debug)]
enum Command {
    Server(ServerArgs),
    Client(ClientArgs),
}

#[tokio::main]
async fn main() {
    env_logger::init();

    let args = CliArgs::parse();

    match args.command {
        Command::Client(a) => start_client(&a).await,
        Command::Server(a) => start_server(&a).await,
    }
}

async fn start_kademlia(args: &CommonArgs) -> Arc<KademliaDht<WrtcSender>> {
    let id = args.id.unwrap_or_else(|| thread_rng().gen());
    info!("Id: {}", id);
    info!("Bootstrap: {:?}", args.bootstrap);

    let mut config: SystemConfig = Default::default();
    config.routing.max_connections = args.max_connections;
    config.routing.max_routing_count = args.max_routing_count;

    create_dht(config, id, args.bootstrap.clone()).await
}

async fn start_client(args: &ClientArgs) {
    let _kad = start_kademlia(&args.common).await;

    tokio::signal::ctrl_c().await.expect("Failed to listen to ctrl-c");
}

async fn start_server(args: &ServerArgs) {
    let kad = start_kademlia(&args.common).await;
    info!("Starting up server");

    warp::serve(dht_connect(kad))
        .run(args.bind)
        .await;
}
