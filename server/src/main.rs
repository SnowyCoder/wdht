use log::info;
use wdht_logic::Id;
use wdht_transport::{warp_filter::dht_connect, wrtc::Connections};

#[tokio::main]
async fn main() {
    env_logger::init();
    info!("Starting up server");

    //let rand = StdRng;
    let id = Id::ZERO;
    let kad = Connections::create(Default::default(), id);

    warp::serve(dht_connect(kad))
        .run(([127, 0, 0, 1], 3030))
        .await;
}
