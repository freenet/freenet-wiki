//! Wiki delegate for key storage and signing operations.
//!
//! Handles:
//! - Storing the user's signing key
//! - Retrieving signing keys
//! - Managing wiki subscriptions

use freenet_stdlib::prelude::*;
use serde::{Deserialize, Serialize};

/// Request types for the wiki delegate.
#[derive(Debug, Serialize, Deserialize)]
pub enum WikiDelegateRequest {
    /// Store a signing key for a wiki.
    StoreKey { wiki_id: [u8; 32], key_bytes: [u8; 32] },
    /// Get the signing key for a wiki.
    GetKey { wiki_id: [u8; 32] },
    /// Check if we have a key for a wiki.
    HasKey { wiki_id: [u8; 32] },
}

/// Response types from the wiki delegate.
#[derive(Debug, Serialize, Deserialize)]
pub enum WikiDelegateResponse {
    /// Key stored successfully.
    KeyStored,
    /// Key retrieved.
    Key { key_bytes: [u8; 32] },
    /// Key not found.
    KeyNotFound,
    /// Key exists check result.
    HasKey { exists: bool },
    /// Error response.
    Error { message: String },
}

/// Serialize to CBOR bytes.
fn serialize<T: Serialize>(value: &T) -> Vec<u8> {
    let mut buf = Vec::new();
    ciborium::into_writer(value, &mut buf).expect("serialization failed");
    buf
}

/// Deserialize from CBOR bytes.
fn deserialize<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
    ciborium::from_reader(bytes).map_err(|e| e.to_string())
}

/// Generate the secret ID for storing a wiki's signing key.
fn wiki_key_secret_id(wiki_id: &[u8; 32]) -> SecretsId {
    let mut key = b"wiki-signing-key:".to_vec();
    key.extend_from_slice(wiki_id);
    SecretsId::new(key)
}

/// Wiki delegate for key storage.
pub struct WikiDelegate;

#[delegate]
impl DelegateInterface for WikiDelegate {
    fn process(
        _params: Parameters<'static>,
        _attested: Option<&'static [u8]>,
        message: InboundDelegateMsg,
    ) -> Result<Vec<OutboundDelegateMsg>, DelegateError> {
        match message {
            InboundDelegateMsg::ApplicationMessage(app_msg) => {
                let request: WikiDelegateRequest = deserialize(&app_msg.payload)
                    .map_err(|e| DelegateError::Other(e))?;

                match request {
                    WikiDelegateRequest::StoreKey { wiki_id, key_bytes } => {
                        let secret_id = wiki_key_secret_id(&wiki_id);
                        Ok(vec![
                            OutboundDelegateMsg::SetSecretRequest(SetSecretRequest {
                                key: secret_id,
                                value: Some(key_bytes.to_vec()),
                            }),
                            OutboundDelegateMsg::ApplicationMessage(
                                ApplicationMessage::new(
                                    app_msg.app,
                                    serialize(&WikiDelegateResponse::KeyStored),
                                )
                            ),
                        ])
                    }

                    WikiDelegateRequest::GetKey { wiki_id } => {
                        let secret_id = wiki_key_secret_id(&wiki_id);
                        Ok(vec![OutboundDelegateMsg::GetSecretRequest(
                            GetSecretRequest {
                                key: secret_id,
                                context: DelegateContext::default(),
                                processed: false,
                            },
                        )])
                    }

                    WikiDelegateRequest::HasKey { wiki_id } => {
                        let secret_id = wiki_key_secret_id(&wiki_id);
                        Ok(vec![OutboundDelegateMsg::GetSecretRequest(
                            GetSecretRequest {
                                key: secret_id,
                                context: DelegateContext::default(),
                                processed: false,
                            },
                        )])
                    }
                }
            }

            InboundDelegateMsg::GetSecretResponse(secret_response) => {
                let response = match secret_response.value {
                    Some(bytes) if bytes.len() == 32 => {
                        let mut key_bytes = [0u8; 32];
                        key_bytes.copy_from_slice(&bytes);
                        WikiDelegateResponse::Key { key_bytes }
                    }
                    _ => WikiDelegateResponse::KeyNotFound,
                };

                // Note: In a real implementation, we'd need to track the original
                // app ID to send the response back. For now, this is simplified.
                let _ = response;
                Ok(vec![])
            }

            _ => Ok(vec![]),
        }
    }
}
