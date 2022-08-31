use serde::{Deserialize, Serialize};
use wdht_logic::{
    transport::{RawResponse, Request},
    Id,
};
use wdht_wrtc::SessionDescription;

use crate::serde::BytesOrB64;

type WrtcOffer = SessionDescription;
type WrtcAnswer = SessionDescription;

#[derive(Debug, Deserialize, Serialize)]
pub struct HandshakeRequest<'a> {
    #[serde(borrow)]
    pub identity: BytesOrB64<'a>,
    #[serde(borrow)]
    pub proof: BytesOrB64<'a>,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum WrtcRequest {
    Req(Request),
    ForwardOffer(Vec<(Id, WrtcOffer)>),
    TryOffer(Id, WrtcOffer),
    // Sent when the sender signals that he doesn't need the connection
    // anymore, if the other peer doesn't need the connection too he can close it
    // without any consequences. The sender should still try to keep the connection open
    // to their best ability, but may still drop it (ex. to make space for new connections)
    HalfClose,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum WrtcResponse {
    Ans(RawResponse<Id>),
    ForwardAnswers(Vec<Result<WrtcAnswer, String>>),
    OkAnswer(Result<WrtcAnswer, String>),
}

#[derive(Serialize, Deserialize, Debug)]
pub enum WrtcPayload {
    Req(WrtcRequest),
    Res(WrtcResponse),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct WrtcMessage {
    pub id: u32,
    pub payload: WrtcPayload,
}
