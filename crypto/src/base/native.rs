use p256::{
    ecdsa::{signature::{Signer, Signature, Verifier}}, elliptic_curve::{rand_core::OsRng, sec1::EncodedPoint}, NistP256
};
use sha2::{Sha256, Digest};
use crate::{Result, HASH_SIZE};

pub use p256::ecdsa::{SigningKey as RawSigningKey, VerifyingKey};

use crate::CryptoError;

#[derive(Clone, Eq, PartialEq)]
pub struct SigningKey {
    raw: RawSigningKey,
    encoded: EncodedPoint<NistP256>,
}
impl SigningKey {
    pub fn from_raw(raw: RawSigningKey) -> Self {
        let encoded = raw.verifying_key().to_encoded_point(false);

        SigningKey {
            raw,
            encoded,
        }
    }
}

pub async fn import_pub_key(key_data: &[u8]) -> Result<VerifyingKey> {
    VerifyingKey::from_sec1_bytes(key_data).map_err(|_| CryptoError::ImportKeyError)
}

pub async fn generate_pair() -> Result<SigningKey> {
    Ok(SigningKey::from_raw(RawSigningKey::random(OsRng)))
}

pub async fn sign(key: &SigningKey, data: &[u8]) -> Result<Vec<u8>> {
    let s = key.raw.sign(data);
    Ok(s.as_bytes().to_vec())
}

pub async fn verify(key: &VerifyingKey, signature: &[u8], data: &[u8]) -> bool {
    Signature::from_bytes(signature)
        .and_then(|signature| key.verify(data, &signature))
        .is_ok()
}

pub fn export_public_key<'a>(key: &'a SigningKey) -> &'a [u8] {
    key.encoded.as_bytes()
}

pub async fn sha2_hash(context: &[u8], data: &[u8]) -> Result<[u8; HASH_SIZE]> {
    let mut hasher = Sha256::new();

    hasher.update(context);
    hasher.update(data);
    Ok(hasher.finalize().as_slice().try_into().unwrap())
}

#[doc(hidden)]
#[cfg(test)]
pub use tokio::test as ttest;
