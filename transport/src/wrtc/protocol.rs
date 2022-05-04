use datachannel::SessionDescription;
use serde::{Deserialize, Serialize};
use wdht_logic::{Id, transport::{Request, RawResponse}};

type WrtcOffer = SessionDescription;
type WrtcAnswer = SessionDescription;

#[derive(Debug, Deserialize, Serialize)]
pub struct HandshakeRequest {
    pub my_id: Id,
    // TODO: encryption data
}

#[derive(Debug, Deserialize, Serialize)]
pub enum HandshakeResponse {
    Ok {
        my_id: Id
        // TODO: encryption data
    },
    Error {
        error: String,
    }
}

#[derive(Serialize, Deserialize)]
pub enum WrtcRequest {
    Req(Request),
    ForwardOffer(Vec<(Id, WrtcOffer)>),
    TryOffer(WrtcOffer),
}

#[derive(Serialize, Deserialize)]
pub enum WrtcResponse {
    Ans(RawResponse<Id>),
    ForwardAnswers(Vec<Result<WrtcAnswer, String>>),
    OkAnswer(Result<WrtcAnswer, String>),
}

#[derive(Serialize, Deserialize)]
pub enum WrtcPayload {
    Req(WrtcRequest),
    Res(WrtcResponse),
}

#[derive(Serialize, Deserialize)]
pub struct WrtcMessage {
    pub id: u32,
    pub payload: WrtcPayload,
}
