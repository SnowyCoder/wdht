use datachannel::SessionDescription;
use serde::{Serialize, Deserialize};


#[derive(Debug, Serialize, Deserialize)]
pub struct ConnectRequest {
    pub offer: SessionDescription,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "result")]
#[serde(rename_all = "snake_case")]
pub enum ConnectResponse {
    Ok {
        answer: SessionDescription,
    },
    Error {
        description: String,
    },
}
