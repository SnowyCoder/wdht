mod base;
mod error;

#[cfg(test)]
#[cfg(target_arch = "wasm32")]
#[doc(hidden)]
pub use base::test::*;

use base::Backend;
pub use error::{CryptoError, Result};


#[derive(Clone, PartialEq, Eq)]
pub struct SigningKey(base::SigningKey);

#[derive(Clone, PartialEq, Eq)]
pub struct VerifyingKey(base::VerifyingKey);

pub struct Crypto(base::Backend);

const HASH_SIZE: usize = 256 / 8;

impl Crypto {
    pub fn new() -> Self {
        Crypto(Backend::new())
    }

    pub async fn import_pub(&self, key_data: &[u8]) -> Result<VerifyingKey> {
        self.0.import_pub(key_data).await.map(VerifyingKey)
    }

    pub async fn generate_pair(&self) -> Result<SigningKey> {
        self.0.generate_pair().await.map(SigningKey)
    }

    pub async fn sign(&self, key: &SigningKey, data: &[u8]) -> Result<Vec<u8>> {
        self.0.sign(&key.0, data).await
    }

    pub async fn verify(&self, key: &VerifyingKey, signature: &[u8], data: &[u8]) -> bool {
        self.0.verify(&key.0, signature, data).await
    }

    pub fn export_public_key<'a>(&self, key: &'a SigningKey) -> &'a [u8] {
        self.0.export_public_key(&key.0)
    }

    pub async fn hash(&self, data: &[u8]) -> Result<[u8; HASH_SIZE]> {
        self.0.hash(&data).await
    }
}
