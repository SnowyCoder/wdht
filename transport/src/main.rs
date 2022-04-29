use anyhow::Result;
use datachannel::{DataChannelHandler, PeerConnectionHandler, RtcConfig, RtcPeerConnection, RtcDataChannel};

struct MyChannel;

impl DataChannelHandler for MyChannel {
    fn on_open(&mut self) {
        // TODO: notify that the data channel is ready (optional)
    }

    fn on_message(&mut self, msg: &[u8]) {
        // TODO: process the received message
    }
}

struct MyConnection;

impl PeerConnectionHandler for MyConnection {
    type DCH = MyChannel;

    /// Used to create the `RtcDataChannel` received through `on_data_channel`.
    fn data_channel_handler(&mut self) -> Self::DCH {
        MyChannel
    }

    fn on_data_channel(&mut self, mut dc: Box<RtcDataChannel<Self::DCH>>) {
        // TODO: store `dc` to keep receiving its messages (otherwise it will be dropped)
    }
}

pub fn main() -> Result<()> {
    let ice_servers = vec!["stun:stun.l.google.com:19302"];
    let conf = RtcConfig::new(&ice_servers);

    let mut pc = RtcPeerConnection::new(&conf, MyConnection)?;

    let mut dc = pc.create_data_channel("test-dc", MyChannel)?;

    // TODO: exchange `SessionDescription` and `IceCandidate` with remote peer
    // TODO: wait for `dc` to be opened (should be signaled through `on_open`)
    // ...
    // Then send a message
    dc.send("Hello Peer!".as_bytes())?;
    Ok(())
}
