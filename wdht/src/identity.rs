use wdht_crypto::{self as crypto, SigningKey}
;
use wdht_logic::{Id, consts::ID_LEN};

const KEY_HASH_CONTEXT: &'static [u8] = b"wdht.transport.identity";

pub struct Identity {
    key: SigningKey,
}

impl Identity {
    pub async fn generate() -> Self {
        let key = crypto::generate_pair().await.expect("Failed to generate crypto key");
        Identity { key, }
    }

    pub fn export_key(&self) -> &[u8] {
        crypto::export_public_key(&self.key)
    }

    pub async fn generate_id(&self) -> Id {
        let key_data = self.export_key();
        self.compute_identity(key_data).await
    }

    async fn compute_identity(&self, key: &[u8]) -> Id {
        let hash_data = crypto::sha2_hash(&KEY_HASH_CONTEXT, key).await.expect("Failed to generate crypto ID");
        // Truncate hashed bytes into ID (hash is 256 bitsm ID should be 160 bits)
        let mut id = Id::ZERO;
        id.0[..ID_LEN].copy_from_slice(&hash_data[..ID_LEN]);
        id
    }

    pub async fn create_proof(&self, fingerprint: &[u8]) -> Vec<u8> {
        crypto::sign(&self.key, fingerprint).await.expect("Failed to generate proof")
    }

    pub async fn check_identity_proof(&self, key: &[u8], fingerprint: &[u8], signature: &[u8]) -> Result<Id, ()> {
        let raw_key = key;
        let key = crypto::import_pub_key(key).await
            .map_err(|_| ())?;
        if !crypto::verify(&key, signature, fingerprint).await {
            return Err(())
        }
        Ok(self.compute_identity(raw_key).await)
    }
}
