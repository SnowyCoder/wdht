use serde::Serialize;
use wdht_logic::Id;
use wdht_wrtc::{WrtcChannel, WrtcError};

use crate::identity::Identity;

use super::{conn::PeerMessageError, protocol::{HandshakeRequest}};

fn encode_data<T: Serialize>(data: &T) -> Result<Vec<u8>, PeerMessageError> {
    serde_json::to_vec(data).map_err(|x| PeerMessageError::InternalError(Box::new(x)))
}

async fn receive_message(chan: &mut WrtcChannel) -> Result<Vec<u8>, PeerMessageError> {
    chan
        .listener
        .recv()
        .await
        .ok_or(WrtcError::ConnectionLost)??
        .data()
        .ok_or(PeerMessageError::HandshakeError("opened channel".into()))
}

pub async fn handshake(conn: &mut WrtcChannel, identity: &Identity) -> Result<Id, PeerMessageError> {
    // Compute local proof
    let fp = conn.sender.local_certificate_fingerprint()?;
    let proof = identity.create_proof(&fp).await;

    let msg = HandshakeRequest {
        identity: identity.export_key().into(),
        proof: proof.into(),
    };

    // Send local proof
    conn.sender.send(&encode_data(&msg)?)
        .map_err(|_| WrtcError::ConnectionLost)?;

    // Receive remote proof
    let msg = receive_message(conn).await?;
    let req = serde_json::from_slice::<HandshakeRequest>(&msg)?;

    // Check remote proof and derive ID
    let other_fingerprint = conn.sender.remote_certificate_fingerprint()?;
    let peer_id = identity.check_identity_proof(&req.identity, &other_fingerprint, &req.proof).await
        .map_err(|_| PeerMessageError::HandshakeError("invalid identity".into()))?;

    Ok(peer_id)
}
