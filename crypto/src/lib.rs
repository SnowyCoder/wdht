mod base;
mod error;

#[cfg(test)]
#[cfg(target_arch = "wasm32")]
#[doc(hidden)]
pub use base::test::*;

pub use error::{CryptoError, Result};


#[derive(Clone, PartialEq, Eq)]
pub struct SigningKey(base::SigningKey);

#[derive(Clone, PartialEq, Eq)]
pub struct VerifyingKey(base::VerifyingKey);

const HASH_SIZE: usize = 256 / 8;

// P-256
pub async fn import_pub_key(key_data: &[u8]) -> Result<VerifyingKey> {
    base::import_pub_key(key_data).await.map(VerifyingKey)
}

pub async fn generate_pair() -> Result<SigningKey> {
    base::generate_pair().await.map(SigningKey)
}

pub async fn sign(key: &SigningKey, data: &[u8]) -> Result<Vec<u8>> {
    base::sign(&key.0, data).await
}

pub async fn verify(key: &VerifyingKey, signature: &[u8], data: &[u8]) -> bool {
    base::verify(&key.0, signature, data).await
}

pub fn export_public_key<'a>(key: &'a SigningKey) -> &'a [u8] {
    base::export_public_key(&key.0)
}

// SHA2
pub async fn sha2_hash(context: &[u8], data: &[u8]) -> Result<[u8; HASH_SIZE]> {
    base::sha2_hash(&context, &data).await
}
