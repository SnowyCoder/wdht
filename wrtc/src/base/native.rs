use bytes::Bytes;
use tracing::debug;
use webrtc::api::APIBuilder;
use webrtc::api::setting_engine::SettingEngine;
use webrtc::api::{
    media_engine::MediaEngine,
    interceptor_registry::register_default_interceptors,
};
use webrtc::data::Error;
use webrtc::data::data_channel::DataChannel;
use webrtc::data_channel::data_channel_init::RTCDataChannelInit;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::peer_connection::configuration::RTCConfiguration;
use tokio::sync::{mpsc, oneshot};
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use std::sync::Arc;

use crate::{WrtcError, ConnectionRole, SessionDescription as WrappedSessionDescription, WrtcChannel, WrtcEvent, WrtcDataChannel as WrappedWrtcDataChannel};

pub type SessionDescription = RTCSessionDescription;
pub type RawConnection = ();// Not available on native!???? TODO
pub type RawChannel = ();// Not available on native!????

const MESSAGE_SIZE: usize = 4096;

pub struct WrtcDataChannel {
    // Only kept to keep the original connection from being deallocated
    _peer_connection: Arc<RTCPeerConnection>,
    data_channel: Arc<DataChannel>,
}

impl WrtcDataChannel {
    pub async fn send(&mut self, msg: &Bytes) -> Result<(), WrtcError> {
        self.data_channel
            .write(msg)
            .await
            .map_err(|_| WrtcError::DataChannelError("runtime error".into()))?;
        Ok(())
    }

    pub fn raw_connection(&self) -> RawConnection {
        ()
    }
}

pub struct RtcConfig(Vec<String>);

impl RtcConfig {
    pub fn new<S: AsRef<str>>(ice_servers: &[S]) -> Self {
        let ice_servers = ice_servers.iter()
            .map(|x| x.as_ref().to_string())
            .collect();
        RtcConfig(ice_servers)
    }

    pub fn get_config(&self) -> RTCConfiguration {
        RTCConfiguration {
            ice_servers: vec![RTCIceServer {
                urls: self.0.clone(),
                ..Default::default()
            }],
            ..Default::default()
        }
    }
}

pub async fn create_channel<E>(
    config: &RtcConfig,
    role: ConnectionRole<E>,
    answer: oneshot::Sender<WrappedSessionDescription>,
) -> Result<WrtcChannel, E>
where
    E: From<WrtcError>,
{
    let mut m = MediaEngine::default();
    m.register_default_codecs().unwrap();
    let mut registry = Registry::new();
    registry = register_default_interceptors(registry, &mut m).unwrap();
    let mut settings = SettingEngine::default();
    settings.detach_data_channels();

    let api = APIBuilder::new()
        .with_media_engine(m)
        .with_interceptor_registry(registry)
        .with_setting_engine(settings)
        .build();

    let config = config.get_config();

    let peer_connection = Arc::new(api.new_peer_connection(config).await.unwrap());
    let (conn_ready_tx, conn_ready_rx) = oneshot::channel();
    let mut conn_ready_tx = Some(conn_ready_tx);

    peer_connection
        .on_peer_connection_state_change(Box::new(move |s: RTCPeerConnectionState| {
            println!("Peer Connection State has changed: {}", s);

            let state = match s {
                RTCPeerConnectionState::Connected => Some(true),
                RTCPeerConnectionState::Failed |
                RTCPeerConnectionState::Closed => Some(false),
                _ => None
            };
            if let Some(state) = state {
                if let Some(x) = conn_ready_tx.take() {
                    let _ = x.send(state);
                }
            }

            Box::pin(async {})
        }))
        .await;
    //TODO: peer_connection.on_data_channel(f)
    let channel = peer_connection.create_data_channel("wdht", Some(RTCDataChannelInit {
        protocol: Some("wrtc_json".to_owned()),
        negotiated: Some(true),
        id: Some(1),
        ..Default::default()
    })).await.unwrap();

    let (inbound_tx, inbound_rx) = mpsc::channel(16);
    let (ready_tx, ready_rx) = oneshot::channel();
    let d = channel.clone();
    channel.on_open(Box::new(move || Box::pin(async move {
        let raw = d.detach().await.expect("Error detaching channel");
        let _ = ready_tx.send(true);

        tokio::spawn(read_loop(raw, inbound_tx));
    }))).await;

    let mut gather_complete = peer_connection.gathering_complete_promise().await;
    let send_complete_description = move |peer_connection: Arc<RTCPeerConnection>| async move {
        gather_complete.recv().await;
        let desc = peer_connection.local_description().await
            .ok_or_else(|| WrtcError::RuntimeError("No local description after gathering complete".into()))?;
        let _ = answer.send(WrappedSessionDescription(desc));
        Ok(())
    };

    match role {
        ConnectionRole::Active(answer_rx) => {
            let offer = peer_connection.create_offer(None).await
                .map_err(|_| WrtcError::SignalingFailed)?;

            peer_connection.set_local_description(offer).await
                .map_err(|_| WrtcError::SignalingFailed)?;

            send_complete_description(peer_connection.clone()).await?;

            let answer = answer_rx.await.map_err(|_| WrtcError::SignalingFailed)??;
            peer_connection.set_remote_description(answer.0).await
                .map_err(|_| WrtcError::InvalidDescription)?;
        }
        ConnectionRole::Passive(offer) => {
            peer_connection.set_remote_description(offer.0).await
                .map_err(|_| WrtcError::SignalingFailed)?;

            let answer = peer_connection.create_answer(None).await
                .map_err(|_| WrtcError::SignalingFailed)?;

            peer_connection.set_local_description(answer).await
                .map_err(|_| WrtcError::InvalidDescription)?;

            send_complete_description(peer_connection.clone()).await?;
        }
    }

    // Wait for connection to be ready
    if !conn_ready_rx.await.map_err(|_| WrtcError::SignalingFailed)? {
        return Err(WrtcError::SignalingFailed.into());
    }
    // Wait for the datachannel to be ready
    ready_rx.await.map_err(|_| WrtcError::SignalingFailed)?;

    debug!("Datachannel open");

    Ok(WrtcChannel {
        sender: WrappedWrtcDataChannel(WrtcDataChannel {
            _peer_connection: peer_connection,
            data_channel: channel.detach().await.expect("Data channel"),
        }),
        listener: inbound_rx,
    })
}

async fn read_loop(chan: Arc<DataChannel>, inbound_tx: mpsc::Sender<Result<WrtcEvent, WrtcError>>) {
    let mut buffer = vec![0u8; MESSAGE_SIZE];
    loop {
        let n = match chan.read(&mut buffer).await {
            Ok(n) => n,
            Err(err) => {
                if err != Error::ErrStreamClosed {
                    let _ = inbound_tx.send(Err(WrtcError::DataChannelError(err.to_string().into()))).await;
                }
                let _ = inbound_tx.send(Err(WrtcError::ConnectionLost)).await;

                return;
            }
        };

        let _ = inbound_tx.send(Ok(WrtcEvent::Data(buffer[..n].to_vec()))).await;
    }
}
