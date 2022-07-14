use std::{cell::RefCell, rc::Rc};

use js_sys::{Reflect, Uint8Array};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, instrument};
use wasm_bindgen::{prelude::Closure, JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use wdht_wasync::SenderExt;
use web_sys::{
    MessageEvent, RtcConfiguration, RtcDataChannel, RtcDataChannelInit, RtcDataChannelType,
    RtcIceConnectionState, RtcPeerConnection, RtcPeerConnectionIceEvent, RtcDataChannelEvent
};

use crate::{
    ConnectionRole, SessionDescription as WrappedSessionDescription, WrtcChannel,
    WrtcDataChannel as WrappedWrtcDataChannel, WrtcError, WrtcEvent,
};

use super::common::ChannelHandler;

pub type SessionDescription = serde_json::Value;
pub type RawConnection = RtcPeerConnection;
pub type RawChannel = RtcDataChannel;

impl From<JsValue> for WrtcError {
    fn from(val: JsValue) -> Self {
        WrtcError::RuntimeError(format!("{:?}", val))
    }
}

#[derive(Clone, Debug)]
pub struct RtcConfig {
    ice_servers: Vec<String>,
}

impl RtcConfig {
    pub fn new<S: AsRef<str>>(ice_servers: &[S]) -> Self {
        RtcConfig {
            ice_servers: ice_servers.iter().map(|x| x.as_ref().to_string()).collect(),
        }
    }
}
pub struct WrtcDataChannel {
    connection: ConnectionHandler,
    channel: DataChannelHandler,
}

impl WrtcDataChannel {
    fn new(connection: ConnectionHandler, channel: DataChannelHandler) -> Self {
        WrtcDataChannel{
            connection,
            channel,
        }
    }

    pub fn send(&mut self, msg: &[u8]) -> Result<(), WrtcError> {
        Ok(self.channel.channel.send_with_u8_array(msg)?)
    }

    pub fn raw_connection(&self) -> RawConnection {
        self.connection.connection.clone()
    }

    fn desc_to_fingerprint(desc: &str) -> Result<Vec<u8>, WrtcError> {
        let sdp = desc.as_bytes();
        // Find fingerprint position
        const PREFIX: &'static [u8] = b"\na=fingerprint:";
        let pos = sdp.windows(PREFIX.len())
            .position(|w| w == PREFIX)
            .ok_or_else(|| WrtcError::RuntimeError("Cannot find fingerprint".into()))?;
        // Remove fingerprint prefix
        let sdp = &sdp[pos + PREFIX.len()..];
        // Remove suffix
        let sdp = sdp.split(|x| *x == b'\r').next().unwrap();
        Ok(sdp.to_vec())
    }

    pub fn local_certificate_fingerprint(&self) -> Result<Vec<u8>, WrtcError> {
        let desc = self.connection.connection.local_description()
            .ok_or_else(|| WrtcError::RuntimeError("No local description".into()))?;
        Self::desc_to_fingerprint(&desc.sdp())
    }

    pub fn remote_certificate_fingerprint(&self) -> Result<Vec<u8>, WrtcError> {
        let desc = self.connection.connection.remote_description()
            .ok_or_else(|| WrtcError::RuntimeError("No remote description".into()))?;
        Self::desc_to_fingerprint(&desc.sdp())
    }
}

#[instrument(skip_all)]
pub async fn create_channel<E>(
    config: &RtcConfig,
    role: ConnectionRole<E>,
    answer: oneshot::Sender<WrappedSessionDescription>,
) -> Result<WrtcChannel, E>
where
    E: From<WrtcError>,
{
    let (inbound_tx, inbound_rx) = mpsc::channel(16);
    let (connection, con_ready_rx) = create_connection(config, inbound_tx.clone(), answer)?;
    let (channel, chan_ready_rx) = create_data_channel(&connection.connection, inbound_tx);

    let conn = &connection.connection;
    match role {
        ConnectionRole::Active(answer_rx) => {
            debug!("Creating offer");
            let offer = JsFuture::from(conn.create_offer())
                .await
                .map_err(WrtcError::from)?;
            JsFuture::from(conn.set_local_description(offer.unchecked_ref()))
                .await
                .map_err(WrtcError::from)?;
            debug!("Waiting for answer");
            let answer = answer_rx.await.map_err(|_| WrtcError::SignalingFailed("Failed to receive SDP answer".into()))??;
            debug!("Answer received");
            let js_answer =
                JsValue::from_serde(&answer).map_err(|_| WrtcError::InvalidDescription)?;
            JsFuture::from(
                connection
                    .connection
                    .set_remote_description(&js_answer.into()),
            )
            .await
        }
        ConnectionRole::Passive(offer) => {
            let js_offer =
                JsValue::from_serde(&offer.0).map_err(|_| WrtcError::InvalidDescription)?;
            JsFuture::from(conn.set_remote_description(&js_offer.into()))
                .await
                .map_err(|_| WrtcError::InvalidDescription)?;
            let answer = JsFuture::from(conn.create_answer())
                .await
                .map_err(WrtcError::from)?;
            JsFuture::from(conn.set_local_description(answer.unchecked_ref())).await
        }
    }
    .map_err(|_| WrtcError::InvalidDescription)?;

    debug!("Waiting for connection...");
    // Wait for the connection to open
    if !con_ready_rx.await.unwrap_or(false){
        return Err(WrtcError::SignalingFailed("Failed to open connection".into()).into());
    }
    debug!("Wait for channel...");
    // Wait for the datachannel to be ready
    chan_ready_rx
        .await
        .map_err(|_| WrtcError::SignalingFailed("Failed to open channel".into()))??;

    debug!("Datachannel open");

    // Return the newly created channel (along with the PeerConnection so it does not close)
    Ok(WrtcChannel {
        sender: WrappedWrtcDataChannel(WrtcDataChannel::new(connection, channel)),
        listener: inbound_rx,
    })
}

#[allow(clippy::type_complexity)]
fn create_data_channel(
    pc: &RtcPeerConnection,
    inbound_tx: mpsc::Sender<Result<WrtcEvent, WrtcError>>,
) -> (
    DataChannelHandler,
    oneshot::Receiver<Result<(), WrtcError>>,
) {
    let mut dc_config = RtcDataChannelInit::new();
    dc_config.id(0).protocol("wrtc_json").negotiated(true);
    let dc = pc.create_data_channel_with_data_channel_dict("wdht", &dc_config);
    dc.set_binary_type(RtcDataChannelType::Arraybuffer);

    let (ready_rx, handler) = ChannelHandler::new(inbound_tx);
    let handler0 = Rc::new(RefCell::new(handler));

    fn on_message(handler: &Rc<RefCell<ChannelHandler>>, ev: MessageEvent) {
        let array = Uint8Array::new(&ev.data());
        let data = array.to_vec();
        handler.borrow_mut().message(data);
    }

    let handler = handler0.clone();
    let onmessage = Closure::wrap(
        Box::new(move |ev: MessageEvent| on_message(&handler, ev)) as Box<dyn Fn(MessageEvent)>
    );
    dc.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));

    let handler = handler0.clone();
    let onerror = Closure::wrap(Box::new(move |ev: JsValue| {
        // ev is a RTCErrorEvent, but it's not present in rust
        let error = Reflect::get(&ev, &JsValue::from_str("error"))
            .and_then(|x| Reflect::get(&x, &JsValue::from_str("errorDetail")))
            .map(|x| {
                x.as_string()
                    .unwrap_or_else(|| "Details are not a string".to_string())
            })
            .unwrap_or_else(|_| "No details provided".to_string());
        handler.borrow_mut().error(error);
    }) as Box<dyn Fn(JsValue)>);
    dc.set_onerror(Some(onerror.as_ref().unchecked_ref()));

    let handler = handler0.clone();
    let onopen = Closure::wrap(Box::new(move || {
        handler.borrow_mut().open();
    }) as Box<dyn Fn()>);
    dc.set_onopen(Some(onopen.as_ref().unchecked_ref()));

    let handler = handler0.clone();
    let onclose = Closure::wrap(Box::new(move || {
        handler.borrow_mut().closed();
    }) as Box<dyn Fn()>);
    dc.set_onclose(Some(onclose.as_ref().unchecked_ref()));
    let handler = DataChannelHandler {
        channel: dc,
        _handler: handler0,
        _onmessage: onmessage,
        _onerror: onerror,
        _onopen: onopen,
        _onclose: onclose,
    };
    (handler, ready_rx)
}

struct DataChannelHandler {
    channel: RtcDataChannel,
    _handler: Rc<RefCell<ChannelHandler>>,
    _onmessage: Closure<dyn Fn(MessageEvent)>,
    _onerror: Closure<dyn Fn(JsValue)>,
    _onopen: Closure<dyn Fn()>,
    _onclose: Closure<dyn Fn()>,
}

impl Drop for DataChannelHandler {
    fn drop(&mut self) {
        self.channel.set_onmessage(None);
        self.channel.set_onerror(None);
        self.channel.set_onopen(None);
        self.channel.set_onclose(None);
    }
}

fn create_connection(
    config: &RtcConfig,
    inbound_tx: mpsc::Sender<Result<WrtcEvent, WrtcError>>,
    signal_tx: oneshot::Sender<WrappedSessionDescription>,
) -> Result<(ConnectionHandler, oneshot::Receiver<bool>), WrtcError> {
    let mut pc_config = RtcConfiguration::new();
    if !config.ice_servers.is_empty() {
        let val = serde_json::json!([{
            "urls": config.ice_servers
        }]);
        pc_config.ice_servers(&JsValue::from_serde(&val).unwrap());
    }
    let pc = RtcPeerConnection::new_with_configuration(&pc_config)?;

    let (ready_tx, ready_rx) = oneshot::channel();
    let ready_tx = RefCell::new(Some(ready_tx));
    let connection = pc.clone();
    let oniceconnectionstatechange = Closure::wrap(Box::new(move || {
        let state = connection.ice_connection_state();
        debug!("Connection state change: {state:?}");
        use RtcIceConnectionState::*;
        let is_successful = match state {
            Connected => true,
            Disconnected | Failed | Closed => false,
            // new, checking, completed or anything else
            _ => return,
        };
        ready_tx.borrow_mut().take().map(|x| x.send(is_successful));
    }) as Box<dyn Fn()>);
    pc.set_oniceconnectionstatechange(Some(oniceconnectionstatechange.as_ref().unchecked_ref()));

    let connection = pc.clone();
    let signal_tx = RefCell::new(Some(signal_tx));
    let onicecandidate = Closure::wrap(Box::new(move |ev: RtcPeerConnectionIceEvent| {
        if ev.candidate().is_none() {
            debug!("ICE gathering candidates complete!");
            let signal_listener = match signal_tx.borrow_mut().take() {
                Some(x) => x,
                None => return, // Double listen (or we simply ignore the result)
            };

            let sess_desc = match connection.local_description() {
                Some(x) => x,
                None => {
                    error!("Gathering complete but no local description provided");
                    return;
                }
            };

            // Ignore if signal is not needed
            let description = sess_desc
                .into_serde()
                .expect("Cannot convert local description to json");
            let _ = signal_listener.send(WrappedSessionDescription(description));
        }
    }) as Box<dyn Fn(RtcPeerConnectionIceEvent)>);
    pc.set_onicecandidate(Some(onicecandidate.as_ref().unchecked_ref()));

    let ondatachannel = Closure::wrap(Box::new(move |ev: RtcDataChannelEvent| {
        let chan = ev.channel();

        let _ = inbound_tx.maybe_spawn_send(Ok(WrtcEvent::OpenChannel(chan)));
    }) as Box<dyn Fn(RtcDataChannelEvent)>);
    pc.set_ondatachannel(Some(ondatachannel.as_ref().unchecked_ref()));

    let handler = ConnectionHandler {
        connection: pc,
        _oniceconnectionstatechange: oniceconnectionstatechange,
        _onicecandidate: onicecandidate,
        _ondatachannel: ondatachannel,
    };
    Ok((handler, ready_rx))
}

struct ConnectionHandler {
    connection: RtcPeerConnection,
    _oniceconnectionstatechange: Closure<dyn Fn()>,
    _onicecandidate: Closure<dyn Fn(RtcPeerConnectionIceEvent)>,
    _ondatachannel: Closure<dyn Fn(RtcDataChannelEvent)>,
}

impl Drop for ConnectionHandler {
    fn drop(&mut self) {
        self.connection.set_oniceconnectionstatechange(None);
        self.connection.set_onicecandidate(None);
    }
}
