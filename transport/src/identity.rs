use wdht_crypto::{SigningKey, Crypto};
use wdht_logic::{Id, consts::ID_LEN};


pub struct Identity {
    crypto: Crypto,
    key: SigningKey,
}

impl Identity {
    pub async fn generate() -> Self {
        let crypto = Crypto::new();
        let key = crypto.generate_pair().await.expect("Failed to generate crypto key");
        Identity { crypto, key, }
    }

    pub fn export_key(&self) -> &[u8] {
        self.crypto.export_public_key(&self.key)
    }

    pub async fn generate_id(&self) -> Id {
        let key_data = self.export_key();
        self.compute_identity(key_data).await
    }

    async fn compute_identity(&self, key: &[u8]) -> Id {
        let hash_data = self.crypto.hash(key).await.expect("Failed to generate crypto ID");
        // Truncate hashed bytes into ID (hash is 256 bitsm ID should be 160 bits)
        let mut id = Id::ZERO;
        id.0[..ID_LEN].copy_from_slice(&hash_data[..ID_LEN]);
        id
    }

    pub async fn create_proof(&self, fingerprint: &[u8]) -> Vec<u8> {
        self.crypto.sign(&self.key, fingerprint).await.expect("Failed to generate proof")
    }

    pub async fn check_identity_proof(&self, key: &[u8], fingerprint: &[u8], signature: &[u8]) -> Result<Id, ()> {
        let raw_key = key;
        let key = self.crypto.import_pub(key).await
            .map_err(|_| ())?;
        if !self.crypto.verify(&key, signature, fingerprint).await {
            return Err(())
        }
        Ok(self.compute_identity(raw_key).await)
    }
}
