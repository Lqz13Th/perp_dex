//! Credentials, signing and nonces of one Lighter API key slot (account index + API key index).
//!
//! Signing is byte-compatible with the official Go signer (lighter-go): transactions are hashed with Poseidon2 over
//! the Goldilocks field into a quintic-extension element and signed with Schnorr over ECgFp5. The Schnorr nonce is
//! random, so signatures differ per call; hashes and the tx_info layout do not
//! (see `tests/fixtures/lighter_signer_vectors.*`).

use std::{
    fmt,
    sync::atomic::{AtomicI64, Ordering},
};

use goldilocks_crypto::{Point, ScalarField, sign_hashed_message, verify_signature};
use poseidon_hash::{Goldilocks, hash_to_quintic_extension};

use extrema_infra::prelude::{InfraError, InfraResult};

use super::api_utils::{LighterTx, SignedLighterTx};

pub const LIGHTER_KEY_BYTES: usize = 40;
pub const LIGHTER_HASH_BYTES: usize = 40;
pub const LIGHTER_SIGNATURE_BYTES: usize = 80;

pub const LIGHTER_ENV_ACCOUNT_INDEX: &str = "LIGHTER_ACCOUNT_INDEX";
pub const LIGHTER_ENV_API_KEY_INDEX: &str = "LIGHTER_API_KEY_INDEX";
pub const LIGHTER_ENV_API_PRIVATE_KEY: &str = "LIGHTER_API_PRIVATE_KEY";

pub fn read_lighter_env_auth() -> InfraResult<LighterAuth> {
    let var =
        |name: &str| std::env::var(name).map_err(|_| InfraError::EnvVarMissing(name.to_string()));
    let account_index = var(LIGHTER_ENV_ACCOUNT_INDEX)?
        .trim()
        .parse()
        .map_err(|_| {
            InfraError::ApiCliError(format!("{LIGHTER_ENV_ACCOUNT_INDEX} is not an integer"))
        })?;
    let api_key_index = var(LIGHTER_ENV_API_KEY_INDEX)?
        .trim()
        .parse()
        .map_err(|_| {
            InfraError::ApiCliError(format!("{LIGHTER_ENV_API_KEY_INDEX} is not 0-254"))
        })?;
    LighterAuth::new(
        account_index,
        api_key_index,
        &var(LIGHTER_ENV_API_PRIVATE_KEY)?,
    )
}

#[derive(Clone)]
pub struct LighterAuth {
    pub account_index: i64,
    pub api_key_index: u8,
    key: LighterPrivateKey,
}

impl fmt::Debug for LighterAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LighterAuth")
            .field("account_index", &self.account_index)
            .field("api_key_index", &self.api_key_index)
            .field("key", &"<redacted>")
            .finish()
    }
}

impl LighterAuth {
    pub fn new(account_index: i64, api_key_index: u8, private_key_hex: &str) -> InfraResult<Self> {
        Ok(Self {
            account_index,
            api_key_index,
            key: LighterPrivateKey::from_hex(private_key_hex)?,
        })
    }

    pub fn key(&self) -> &LighterPrivateKey {
        &self.key
    }

    /// Signs a transaction into the `tx_type` / `tx_info` pair `sendTx` takes.
    pub fn sign_tx<T: LighterTx>(&self, mut tx: T, chain_id: u32) -> InfraResult<SignedLighterTx> {
        let hash = lighter_hash_fields(&tx.hash_fields(chain_id));
        tx.set_sig(self.key.sign_hash(&hash)?.to_vec());
        Ok(SignedLighterTx {
            tx_type: T::TX_TYPE,
            tx_info: serde_json::to_string(&tx)
                .map_err(|e| InfraError::ApiCliError(format!("Lighter tx_info: {e}")))?,
            tx_hash: encode_hex(&hash),
        })
    }

    /// `<deadline>:<account index>:<api key index>:<hex signature>`, accepted by private reads until
    /// `deadline_s` (Unix seconds).
    pub fn auth_token(&self, deadline_s: u64) -> InfraResult<String> {
        let message = format!("{deadline_s}:{}:{}", self.account_index, self.api_key_index);
        let sig = self
            .key
            .sign_hash(&lighter_hash_bytes(message.as_bytes()))?;
        Ok(format!("{message}:{}", encode_hex(&sig)))
    }
}

/// API private key of one (account, API key index) slot: 40 bytes, a little-endian ECgFp5 scalar.
#[derive(Clone)]
pub struct LighterPrivateKey([u8; LIGHTER_KEY_BYTES]);

impl fmt::Debug for LighterPrivateKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LighterPrivateKey(<redacted>)")
    }
}

impl LighterPrivateKey {
    /// 80 hex characters, with or without `0x`.
    pub fn from_hex(key: &str) -> InfraResult<Self> {
        let bytes = decode_hex(key.trim().trim_start_matches("0x"))?;
        let bytes: [u8; LIGHTER_KEY_BYTES] = bytes.try_into().map_err(|b: Vec<u8>| {
            InfraError::ApiCliError(format!(
                "Lighter API private key must be {LIGHTER_KEY_BYTES} bytes, got {}",
                b.len()
            ))
        })?;
        Ok(Self(bytes))
    }

    /// Encoded public key, as registered on the exchange for the API key slot.
    pub fn public_key(&self) -> InfraResult<[u8; LIGHTER_KEY_BYTES]> {
        let scalar = ScalarField::from_bytes_le(&self.0).map_err(InfraError::ApiCliError)?;
        Ok(Point::generator().mul(&scalar).encode().to_bytes_le())
    }

    /// Schnorr signature `s || e` of a 40-byte message hash.
    pub fn sign_hash(
        &self,
        hash: &[u8; LIGHTER_HASH_BYTES],
    ) -> InfraResult<[u8; LIGHTER_SIGNATURE_BYTES]> {
        let nonce = ScalarField::sample_crypto().to_bytes_le();
        let sig = sign_hashed_message(&self.0, hash, &nonce)
            .map_err(|e| InfraError::ApiCliError(format!("Lighter signing failed: {e}")))?;
        sig.try_into().map_err(|s: Vec<u8>| {
            InfraError::ApiCliError(format!("Lighter signature has {} bytes", s.len()))
        })
    }
}

pub fn lighter_verify(
    public_key: &[u8; LIGHTER_KEY_BYTES],
    hash: &[u8; LIGHTER_HASH_BYTES],
    signature: &[u8; LIGHTER_SIGNATURE_BYTES],
) -> bool {
    verify_signature(signature, hash, public_key).unwrap_or(false)
}

/// Transaction hash: Poseidon2 of the fields, each a canonical Goldilocks element.
pub fn lighter_hash_fields(fields: &[u64]) -> [u8; LIGHTER_HASH_BYTES] {
    let elems: Vec<Goldilocks> = fields
        .iter()
        .map(|&v| Goldilocks::from_canonical_u64(v))
        .collect();
    hash_to_quintic_extension(&elems).to_bytes_le()
}

/// Message hash of free-form bytes (auth tokens): 8-byte little-endian elements, the last one zero-padded.
pub fn lighter_hash_bytes(message: &[u8]) -> [u8; LIGHTER_HASH_BYTES] {
    let elems: Vec<Goldilocks> = message
        .chunks(8)
        .map(|chunk| {
            let mut word = [0u8; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            Goldilocks::from_canonical_u64(u64::from_le_bytes(word))
        })
        .collect();
    hash_to_quintic_extension(&elems).to_bytes_le()
}

pub fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn decode_hex(hex: &str) -> InfraResult<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return Err(InfraError::ApiCliError(format!(
            "odd-length hex string ({} chars)",
            hex.len()
        )));
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&hex[i..i + 2], 16)
                .map_err(|_| InfraError::ApiCliError(format!("invalid hex at {i}: {hex}")))
        })
        .collect()
}

/// Next nonce of the API key: Lighter wants it strictly +1 per transaction. Read from the exchange on first use
/// and after any rejected send; negative means unknown.
#[derive(Debug)]
pub struct LighterNonce(AtomicI64);

const NONCE_UNKNOWN: i64 = -1;

impl Default for LighterNonce {
    fn default() -> Self {
        Self(AtomicI64::new(NONCE_UNKNOWN))
    }
}

impl LighterNonce {
    pub(crate) fn take(&self, n: i64) -> Option<i64> {
        let mut v = self.0.load(Ordering::Acquire);
        loop {
            if v < 0 {
                return None;
            }
            match self
                .0
                .compare_exchange_weak(v, v + n, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => return Some(v),
                Err(current) => v = current,
            }
        }
    }

    /// Takes `n` nonces from `fresh` unless another caller seeded the counter first.
    pub(crate) fn seed_and_take(&self, fresh: i64, n: i64) -> i64 {
        loop {
            if let Some(start) = self.take(n) {
                return start;
            }
            if self
                .0
                .compare_exchange(
                    NONCE_UNKNOWN,
                    fresh + n,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
            {
                return fresh;
            }
        }
    }

    pub(crate) fn invalidate(&self) {
        self.0.store(NONCE_UNKNOWN, Ordering::Release);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    pub(crate) struct Vectors {
        pub private_key: String,
        pub public_key: String,
        pub chain_id: u32,
        pub txs: Vec<TxVector>,
        pub auth_message: String,
        pub auth_hash: String,
        pub auth_token: String,
    }

    #[derive(Deserialize)]
    pub(crate) struct TxVector {
        pub name: String,
        pub tx_type: u8,
        pub tx_info: String,
        pub hash: String,
    }

    pub(crate) fn vectors() -> Vectors {
        serde_json::from_str(include_str!(
            "../../../tests/fixtures/lighter_signer_vectors.json"
        ))
        .unwrap()
    }

    pub(crate) fn arr<const N: usize>(hex: &str) -> [u8; N] {
        decode_hex(hex).unwrap().try_into().unwrap()
    }

    #[test]
    fn public_key_matches_the_go_key_manager() {
        let v = vectors();
        let key = LighterPrivateKey::from_hex(&v.private_key).unwrap();
        assert_eq!(encode_hex(&key.public_key().unwrap()), v.public_key);
        assert!(format!("{key:?}").contains("redacted"));
    }

    #[test]
    fn signatures_verify_and_tampering_fails() {
        let v = vectors();
        let key = LighterPrivateKey::from_hex(&v.private_key).unwrap();
        let pk = key.public_key().unwrap();
        let hash: [u8; 40] = arr(&v.txs[0].hash);
        let sig = key.sign_hash(&hash).unwrap();
        assert!(lighter_verify(&pk, &hash, &sig));
        let mut bad = sig;
        bad[3] ^= 1;
        assert!(!lighter_verify(&pk, &hash, &bad));
        let mut other = hash;
        other[0] ^= 1;
        assert!(!lighter_verify(&pk, &other, &sig));
    }

    #[test]
    fn auth_message_hash_and_official_token_signature() {
        let v = vectors();
        assert_eq!(
            encode_hex(&lighter_hash_bytes(v.auth_message.as_bytes())),
            v.auth_hash
        );
        let (msg, sig) = v.auth_token.rsplit_once(':').unwrap();
        assert_eq!(msg, v.auth_message);
        let pk: [u8; 40] = arr(&v.public_key);
        assert!(lighter_verify(&pk, &arr(&v.auth_hash), &arr(sig)));
    }

    #[test]
    fn auth_token_has_the_go_layout_and_verifies() {
        let v = vectors();
        let auth = LighterAuth::new(758666, 4, &v.private_key).unwrap();
        let token = auth.auth_token(1791520000).unwrap();
        let (msg, sig) = token.rsplit_once(':').unwrap();
        assert_eq!(msg, v.auth_message);
        assert!(lighter_verify(
            &arr(&v.public_key),
            &arr(&v.auth_hash),
            &arr(sig)
        ));
        assert!(!format!("{auth:?}").contains(&v.private_key[..10]));
    }

    #[test]
    fn hex_round_trip_and_key_length() {
        assert_eq!(decode_hex("00ff10").unwrap(), vec![0, 255, 16]);
        assert_eq!(encode_hex(&[0, 255, 16]), "00ff10");
        assert!(decode_hex("abc").is_err());
        assert!(LighterPrivateKey::from_hex("0x00ff").is_err());
    }

    /// `LIGHTER_DUMP_SIGS=<path> cargo test --lib dump_signatures -- --ignored`, then feed the file to
    /// `go run ./cmd/golden verify` (see the fixture's Go source) to check our signatures with the official verifier.
    #[test]
    #[ignore]
    fn dump_signatures_for_the_go_verifier() {
        let v = vectors();
        let key = LighterPrivateKey::from_hex(&v.private_key).unwrap();
        let items: Vec<_> = v
            .txs
            .iter()
            .chain(v.txs.iter())
            .map(|t| {
                let sig = key.sign_hash(&arr(&t.hash)).unwrap();
                serde_json::json!({"name": t.name, "hash": t.hash, "signature": encode_hex(&sig)})
            })
            .collect();
        let out = serde_json::json!({"public_key": v.public_key, "items": items});
        std::fs::write(std::env::var("LIGHTER_DUMP_SIGS").unwrap(), out.to_string()).unwrap();
    }

    #[test]
    fn nonces_are_consecutive_and_reset_on_invalidate() {
        let nonce = LighterNonce::default();
        assert_eq!(nonce.take(1), None);
        assert_eq!(nonce.seed_and_take(5, 2), 5);
        assert_eq!(nonce.take(1), Some(7));
        assert_eq!(nonce.take(3), Some(8));
        assert_eq!(nonce.take(1), Some(11));
        nonce.invalidate();
        assert_eq!(nonce.take(1), None);
    }

    #[test]
    fn concurrent_takes_never_share_a_nonce() {
        let nonce = LighterNonce::default();
        let mut got: Vec<i64> = std::thread::scope(|s| {
            let handles: Vec<_> = (0..8)
                .map(|_| {
                    s.spawn(|| {
                        (0..200)
                            .map(|_| nonce.take(1).unwrap_or_else(|| nonce.seed_and_take(100, 1)))
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            handles
                .into_iter()
                .flat_map(|h| h.join().unwrap())
                .collect()
        });
        got.sort_unstable();
        assert_eq!(got, (100..100 + 1600).collect::<Vec<_>>());
    }
}
