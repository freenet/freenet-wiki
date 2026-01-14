//! Utility functions for signing, verification, and hashing.

use ed25519_dalek::{Signature, SignatureError, Signer, SigningKey, VerifyingKey};
use serde::Serialize;

/// A fast 64-bit hash for content addressing.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct FastHash(pub u64);

/// Compute a fast hash of bytes using blake3, truncated to 64 bits.
pub fn fast_hash(data: &[u8]) -> FastHash {
    let hash = blake3::hash(data);
    let bytes = hash.as_bytes();
    FastHash(u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]))
}

/// Hash a line of text for content-addressed operations.
pub type LineHash = FastHash;

/// Compute hash of a line (trimmed, to handle whitespace variations).
pub fn line_hash(line: &str) -> LineHash {
    fast_hash(line.trim().as_bytes())
}

/// Sign a serializable struct using CBOR encoding.
pub fn sign_struct<T: Serialize>(message: &T, signing_key: &SigningKey) -> Signature {
    let mut data = Vec::new();
    ciborium::ser::into_writer(message, &mut data).expect("Serialization should not fail");
    signing_key.sign(&data)
}

/// Verify a signature over a serializable struct.
pub fn verify_struct<T: Serialize>(
    message: &T,
    signature: &Signature,
    verifying_key: &VerifyingKey,
) -> Result<(), SignatureError> {
    let mut data = Vec::new();
    ciborium::ser::into_writer(message, &mut data).expect("Serialization should not fail");
    verifying_key.verify_strict(&data, signature)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    #[test]
    fn test_sign_verify_roundtrip() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        #[derive(Serialize)]
        struct TestMessage {
            content: String,
            value: u64,
        }

        let message = TestMessage {
            content: "Hello, wiki!".to_string(),
            value: 42,
        };

        let signature = sign_struct(&message, &signing_key);
        assert!(verify_struct(&message, &signature, &verifying_key).is_ok());
    }

    #[test]
    fn test_fast_hash_deterministic() {
        let data = b"test content";
        let hash1 = fast_hash(data);
        let hash2 = fast_hash(data);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_line_hash_trims_whitespace() {
        let hash1 = line_hash("  hello  ");
        let hash2 = line_hash("hello");
        assert_eq!(hash1, hash2);
    }
}
