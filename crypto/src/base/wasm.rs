use std::iter::once;

use js_sys::{Array, Object, Uint8Array, Reflect, ArrayBuffer};
use wasm_bindgen::{JsValue, JsCast};
use wasm_bindgen_futures::JsFuture;
use web_sys::{window, SubtleCrypto, CryptoKey};

use crate::{error::{Result, CryptoError}, HASH_SIZE};

fn subtle() -> SubtleCrypto {
    window()
        .expect("No window object found")
        .crypto()
        .expect("Could not find crypto instance")
        .subtle()
}

fn create_algorithm() -> Object {
    let o = Object::new();
    Reflect::set(&o, &"name".into(), &"ECDSA".into()).unwrap();
    Reflect::set(&o, &"namedCurve".into(), &"P-256".into()).unwrap();
    o
}

fn create_sign_params() -> Object {
    let o = Object::new();
    Reflect::set(&o, &"name".into(), &"ECDSA".into()).unwrap();
    Reflect::set(&o, &"hash".into(), &"SHA-256".into()).unwrap();
    o
}

trait ToErrInner<T> {
    fn map_err_internal(self) -> core::result::Result<T, CryptoError>;
}
impl<T> ToErrInner<T> for core::result::Result<T, JsValue> {
    fn map_err_internal(self) -> core::result::Result<T, CryptoError> {
        self.map_err(|x| CryptoError::InternalError(format!("{:?}", x)))
    }
}

pub struct Backend {
    subtle: SubtleCrypto,
    algorithm: Object,
    sign_params: Object,
}

impl Backend {
    pub fn new() -> Self {
        Backend {
            subtle: subtle(),
            algorithm: create_algorithm(),
            sign_params: create_sign_params(),
        }
    }

    pub async fn import_pub(&self, key_data: &[u8]) -> Result<VerifyingKey> {
        VerifyingKey::import(&self, key_data).await
    }

    pub async fn generate_pair(&self) -> Result<SigningKey> {
        SigningKey::generate(&self).await
    }

    pub async fn sign(&self, key: &SigningKey, data: &[u8]) -> Result<Vec<u8>> {
        key.sign(self, data).await
    }

    pub async fn verify(&self, key: &VerifyingKey, signature: &[u8], data: &[u8]) -> bool {
        key.verify(self, signature, data).await
    }

    pub fn export_public_key<'a>(&self, key: &'a SigningKey) -> &'a [u8] {
        key.exported_public_key()
    }


    pub async fn hash(&self, data: &[u8]) -> Result<[u8; HASH_SIZE]> {
        // Safety: the first step of sign requires copying the buffer.
        let data: Uint8Array = unsafe { Uint8Array::view(data) };
        let promise = self.subtle.digest_with_str_and_buffer_source("SHA-256", &data)
            .map_err_internal()?;
        let buffer: ArrayBuffer = JsFuture::from(promise).await.map_err_internal()?.unchecked_into();
        let mut res_data = [0u8; HASH_SIZE];
        Uint8Array::new(&buffer).copy_to(&mut res_data);
        Ok(res_data)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct SigningKey {
    private: CryptoKey,
    public: Box<[u8]>,// Already pre-exported
}

impl SigningKey {
    async fn generate(ctx: &Backend) -> Result<Self> {
        let usages: Array = once("sign").map(JsValue::from).collect();

        let promise = ctx.subtle.generate_key_with_object(&ctx.algorithm, true, &usages)
            .map_err_internal()?;

        let key = JsFuture::from(promise).await.map_err_internal()?;

        let public_key: CryptoKey = Reflect::get(&key, &"publicKey".into()).unwrap().unchecked_into();
        let export_promise = ctx.subtle.export_key("raw", &public_key).map_err_internal()?;
        let exported: ArrayBuffer = JsFuture::from(export_promise).await.map_err_internal()?.unchecked_into();

        Ok(SigningKey {
            private: Reflect::get(&key, &"privateKey".into()).unwrap().unchecked_into(),
            public: Uint8Array::new(&exported).to_vec().into_boxed_slice(),
        })
    }

    async fn sign(&self, ctx: &Backend, data: &[u8]) -> Result<Vec<u8>> {
        // Safety: the first step of sign requires copying the buffer.
        let data: Uint8Array = unsafe { Uint8Array::view(data) };
        let promise = ctx.subtle.sign_with_object_and_buffer_source(&ctx.sign_params, &self.private, &data)
            .map_err_internal()?;
        let res: ArrayBuffer = JsFuture::from(promise).await
            .map_err_internal()?
            .unchecked_into();

        return Ok(Uint8Array::new(&res).to_vec())
    }

    fn exported_public_key(&self) -> &[u8] {
        &self.public
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct VerifyingKey(CryptoKey);

impl VerifyingKey {
    async fn import(ctx: &Backend, key_data: &[u8]) -> Result<Self> {
        let usages: Array = once("verify").map(JsValue::from).collect();

        // Safety: the first step of import_key requires copying the buffer.
        let key_data: Uint8Array = unsafe { Uint8Array::view(key_data) };
        let promise = ctx.subtle.import_key_with_object("raw", &key_data, &ctx.algorithm, false, &usages)
            .map_err(|_| CryptoError::ImportKeyError)?;
        let res = JsFuture::from(promise).await.map_err_internal()
            .map_err(|_| CryptoError::ImportKeyError)?;
        Ok(VerifyingKey(res.unchecked_into()))
    }

    async fn verify(&self, ctx: &Backend, signature: &[u8], data: &[u8]) -> bool {
        let signature: Uint8Array = unsafe { Uint8Array::view(signature) };
        let data: Uint8Array = unsafe { Uint8Array::view(data) };
        let promise = ctx.subtle.verify_with_object_and_buffer_source_and_buffer_source(&ctx.sign_params, &self.0, &signature, &data)
            .unwrap();// Key has been constructed with "verify" usage.
        let x = JsFuture::from(promise).await.unwrap();
        x.as_bool().unwrap()
    }
}

#[doc(hidden)]
#[cfg(test)]
pub use wasm_bindgen_test::wasm_bindgen_test as ttest;

#[cfg(test)]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);
