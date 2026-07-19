//! Client authorization — the seam, not the token.
//!
//! The relay depends only on *"was this connection authorized?"*, never on
//! *how*. [`Authenticator::verify`] runs after the protocol-version check and
//! before the client joins the lobby; on failure the connection is rejected.
//! [`SharedTokenAuth`] is the reference implementation (a versioned shared
//! token); a platform-attestation authenticator can be swapped in later behind
//! the same trait with no relay changes.
//!
//! Honest threat model: a client cannot keep a secret, so this deters anonymous
//! clients and casual spam — it is not tamper-proofing.

use std::collections::HashMap;

use netplay_protocol::SharedTokenCredential;

/// What `verify` returns on success. Deliberately thin — this gate answers "is
/// this my app?", not "who is the user?". Real user identity, when wanted, slots
/// in behind this same seam.
#[derive(Clone, Copy, Debug)]
pub struct Identity {
    /// Which key id authorized this connection (useful during key rotation).
    pub key_id: u16,
}

/// Why authorization failed. The message is sent to the client before closing.
#[derive(Clone, Copy, Debug)]
pub enum AuthError {
    Malformed,
    UnknownKey,
    BadToken,
}

impl AuthError {
    pub fn message(self) -> &'static str {
        match self {
            AuthError::Malformed => "malformed credential",
            AuthError::UnknownKey => "unknown key id",
            AuthError::BadToken => "invalid token",
        }
    }
}

/// Authorizes a connecting client from its opaque credential (arbitrary JSON the
/// implementation interprets; the relay never inspects it).
pub trait Authenticator: Send + Sync {
    fn verify(&self, credential: &serde_json::Value) -> Result<Identity, AuthError>;
}

/// Reference authenticator: accept a versioned shared token. Holds a *set* of
/// valid keys (by id) so `N` and `N+1` can be accepted during a rotation.
pub struct SharedTokenAuth {
    keys: HashMap<u16, String>,
}

impl SharedTokenAuth {
    pub fn new(keys: HashMap<u16, String>) -> Self {
        Self { keys }
    }

    /// The development default key (matches `netplay_protocol::DEV_*`).
    pub fn dev() -> Self {
        let mut keys = HashMap::new();
        keys.insert(
            netplay_protocol::DEV_KEY_ID,
            netplay_protocol::DEV_TOKEN.to_string(),
        );
        Self::new(keys)
    }

    /// Parse `NETPLAY_TOKENS` (`"id:token,id:token,..."`) if set, else the dev
    /// default. Lets you rotate keys without a code change.
    pub fn from_env_or_dev() -> Self {
        match std::env::var("NETPLAY_TOKENS") {
            Ok(spec) if !spec.trim().is_empty() => {
                let mut keys = HashMap::new();
                for entry in spec.split(',') {
                    if let Some((id, token)) = entry.split_once(':') {
                        if let Ok(id) = id.trim().parse::<u16>() {
                            keys.insert(id, token.trim().to_string());
                        }
                    }
                }
                Self::new(keys)
            }
            _ => Self::dev(),
        }
    }
}

impl Authenticator for SharedTokenAuth {
    fn verify(&self, credential: &serde_json::Value) -> Result<Identity, AuthError> {
        let cred = SharedTokenCredential::from_value(credential).ok_or(AuthError::Malformed)?;
        match self.keys.get(&cred.key_id) {
            Some(expected) if *expected == cred.token => Ok(Identity {
                key_id: cred.key_id,
            }),
            Some(_) => Err(AuthError::BadToken),
            None => Err(AuthError::UnknownKey),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_token_rejects_others() {
        let auth = SharedTokenAuth::dev();
        let good = SharedTokenCredential {
            key_id: netplay_protocol::DEV_KEY_ID,
            token: netplay_protocol::DEV_TOKEN.into(),
        }
        .to_value();
        assert!(auth.verify(&good).is_ok());

        let wrong_token = SharedTokenCredential {
            key_id: netplay_protocol::DEV_KEY_ID,
            token: "nope".into(),
        }
        .to_value();
        assert!(matches!(
            auth.verify(&wrong_token),
            Err(AuthError::BadToken)
        ));

        let unknown_key = SharedTokenCredential {
            key_id: 999,
            token: netplay_protocol::DEV_TOKEN.into(),
        }
        .to_value();
        assert!(matches!(
            auth.verify(&unknown_key),
            Err(AuthError::UnknownKey)
        ));

        assert!(matches!(
            auth.verify(&serde_json::json!("garbage")),
            Err(AuthError::Malformed)
        ));
    }
}
