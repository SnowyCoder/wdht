use std::borrow::Cow;

use serde::{Serialize, Deserialize};
use wdht_logic::Id;
use wdht_wrtc::SessionDescription;


#[derive(Debug, Serialize, Deserialize)]
pub struct ConnectRequest {
    pub id: Id,
    pub offer: SessionDescription,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "result")]
#[serde(rename_all = "snake_case")]
pub enum ConnectResponse<'a> {
    Ok {
        answer: SessionDescription,
    },
    Error {
        description: Cow<'a, str>,
    },
}
