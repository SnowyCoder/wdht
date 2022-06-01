use std::{rc::Rc, cell::RefCell};

use js_sys::{Uint8Array, Reflect};
use send_wrapper::SendWrapper;
use tokio::sync::{oneshot, mpsc};
use tracing::{debug, error, instrument};
use wasm_bindgen::{JsValue, prelude::Closure, JsCast};
use wasm_bindgen_futures::JsFuture;
use web_sys::{RtcPeerConnection, MessageEvent, RtcDataChannel, RtcDataChannelInit, RtcDataChannelType, RtcConfiguration, RtcIceGatheringState, RtcIceConnectionState};

use crate::{WrtcError, ConnectionRole, SessionDescription as WrappedSessionDescription, WrtcChannel, WrtcDataChannel as WrappedWrtcDataChannel};

use super::common::ChannelHandler;

pub type SessionDescription = serde_json::Value;


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

pub struct WrtcDataChannel(SendWrapper<InnerWrtcDataChannel>);

struct InnerWrtcDataChannel {
    _connection: ConnectionHandler,
    channel: DataChannelHandler,
}

impl WrtcDataChannel {
    fn new(connection: ConnectionHandler, channel: DataChannelHandler) -> Self {
        WrtcDataChannel(SendWrapper::new(InnerWrtcDataChannel {
            _connection: connection,
            channel,
        }))
    }

    pub fn send(&mut self, msg: &[u8]) -> Result<(), WrtcError> {
        Ok(self.0.channel.channel.send_with_u8_array(msg)?)
    }
}

#[instrument(skip_all)]
pub async fn create_channel<E>(config: &RtcConfig, role: ConnectionRole<E>, answer: oneshot::Sender<WrappedSessionDescription>) -> Result<WrtcChannel, E>
    where E: From<WrtcError> {

    let (connection, con_ready_rx) = create_connection(config, answer)?;
    let (channel, chan_ready_rx, chan_inbound_rx)  = create_data_channel(&connection.connection);

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
            let answer = answer_rx.await.map_err(|_| WrtcError::SignalingFailed)??;
            debug!("Answer received");
            let js_answer = JsValue::from_serde(&answer).map_err(|_| WrtcError::InvalidDescription)?;
            JsFuture::from(connection.connection.set_remote_description(&js_answer.into())).await
        },
        ConnectionRole::Passive(offer) => {
            let js_offer = JsValue::from_serde(&offer.0).map_err(|_| WrtcError::InvalidDescription)?;
            JsFuture::from(conn.set_remote_description(&js_offer.into()))
                .await
                .map_err(|_| WrtcError::InvalidDescription)?;
            let answer = JsFuture::from(conn.create_answer())
                .await
                .map_err(WrtcError::from)?;
            JsFuture::from(conn.set_local_description(answer.unchecked_ref())).await
        },
    }.map_err(|_| WrtcError::InvalidDescription)?;

    debug!("Waiting for connection...");
    // Wait for the connection to open
    if !con_ready_rx.await.map_err(|_| WrtcError::SignalingFailed)? {
        return Err(WrtcError::SignalingFailed.into());
    }
    debug!("Wait for channel...");
    // Wait for the datachannel to be ready
    chan_ready_rx.await
        .map_err(|_| WrtcError::SignalingFailed)??;

    debug!("Datachannel open");

    // Return the newly created channel (along with the PeerConnection so it does not close)
    Ok(WrtcChannel {
        sender: WrappedWrtcDataChannel(WrtcDataChannel::new(connection, channel)),
        listener: chan_inbound_rx,
    })
}

fn create_data_channel(pc: &RtcPeerConnection) -> (DataChannelHandler, oneshot::Receiver<Result<(), WrtcError>>, mpsc::Receiver<Result<Vec<u8>, WrtcError>>) {
    let mut dc_config = RtcDataChannelInit::new();
    dc_config.id(0)
        .protocol("wrtc_json")
        .negotiated(true);
    let dc = pc.create_data_channel_with_data_channel_dict("wdht", &dc_config);
    dc.set_binary_type(RtcDataChannelType::Arraybuffer);

    let (ready_rx, inbound_rx, handler) = ChannelHandler::new();
    let handler0 = Rc::new(RefCell::new(handler));

    fn on_message(handler: &Rc<RefCell<ChannelHandler>>, ev: MessageEvent) {
        let array = Uint8Array::new(&ev.data());
        let data = array.to_vec();
        handler.borrow_mut().message(data);
    }

    let handler = handler0.clone();
    let onmessage = Closure::wrap(Box::new(move |ev: MessageEvent| on_message(&handler, ev)) as Box<dyn Fn(MessageEvent)>);
    dc.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));

    let handler = handler0.clone();
    let onerror = Closure::wrap(Box::new(move |ev: JsValue| {
        // ev is a RTCErrorEvent, but it's not present in rust
        let error = Reflect::get(&ev, &JsValue::from_str("error"))
            .and_then(|x| Reflect::get(&x, &JsValue::from_str("errorDetail")))
            .map(|x| x.as_string().unwrap_or_else(|| "Details are not a string".to_string()))
            .unwrap_or_else(|_| "No details provided".to_string());
        handler.borrow_mut().error(error);
    })  as Box<dyn Fn(JsValue)>);
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
    (handler, ready_rx, inbound_rx)
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
    signal_tx: oneshot::Sender<WrappedSessionDescription>,
) -> Result<(ConnectionHandler, oneshot::Receiver<bool>), WrtcError> {
    let mut pc_config = RtcConfiguration::new();
    let val = serde_json::json!([{
        "urls": config.ice_servers
    }]);
    pc_config.ice_servers(&JsValue::from_serde(&val).unwrap());
    let pc = Rc::new(RtcPeerConnection::new_with_configuration(&pc_config)?);

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
    let onicegatheringstatechange = Closure::wrap(Box::new(move || {
        let state = connection.ice_gathering_state();

        debug!("ICE gathering state change {state:?}");
        if RtcIceGatheringState::Complete == state {
            let signal_listener = match signal_tx.borrow_mut().take() {
                Some(x) => x,
                None => return,// Double listen (or we simply ignore the result)
            };

            let sess_desc = match connection.local_description() {
                Some(x) => x,
                None => {
                    error!("Gathering complete but no local description provided");
                    return;
                }
            };

            // Ignore if signal is not needed
            let description = sess_desc.into_serde().expect("Cannot convert local description to json");
            let _ = signal_listener.send(WrappedSessionDescription(description));
        }
    }) as Box<dyn Fn()>);
    pc.set_onicegatheringstatechange(Some(onicegatheringstatechange.as_ref().unchecked_ref()));

    let handler = ConnectionHandler {
        connection: pc,
        _oniceconnectionstatechange: oniceconnectionstatechange,
        _onicegatheringstatechange: onicegatheringstatechange,
    };
    Ok((handler, ready_rx))
}

struct ConnectionHandler {
    connection: Rc<RtcPeerConnection>,
    _oniceconnectionstatechange: Closure<dyn Fn()>,
    _onicegatheringstatechange: Closure<dyn Fn()>,
}

impl Drop for ConnectionHandler {
    fn drop(&mut self) {
        self.connection.set_oniceconnectionstatechange(None);
        self.connection.set_onicegatheringstatechange(None);
    }
}
