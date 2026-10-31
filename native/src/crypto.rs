//! OpenPGP passphrase encryption for capsules (roadmap item).
//!
//! A capsule is serialized to its `.eml` bytes and stored as an armored
//! `.eml.asc` message encrypted with a passphrase (AES-256, SEIPD v1,
//! iterated+salted S2K). Everything happens offline through rPGP; no keyring,
//! no gpg binary, no network.

use pgp::composed::{Deserializable, Message};
use pgp::crypto::hash::HashAlgorithm;
use pgp::crypto::sym::SymmetricKeyAlgorithm;
use pgp::types::StringToKey;
use rand::rngs::StdRng;
use rand::SeedableRng;

/// Errors surfaced to the UI as localized toasts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CryptoError {
    EmptyPassphrase,
    Encryption(String),
    Decryption(String),
}

impl std::fmt::Display for CryptoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CryptoError::EmptyPassphrase => write!(f, "empty passphrase"),
            CryptoError::Encryption(err) => write!(f, "encryption failed: {err}"),
            CryptoError::Decryption(err) => write!(f, "decryption failed: {err}"),
        }
    }
}

/// Encrypt `plain` with a passphrase into an ASCII-armored OpenPGP message.
pub fn encrypt_to_armored(plain: &[u8], passphrase: &str) -> Result<String, CryptoError> {
    if passphrase.is_empty() {
        return Err(CryptoError::EmptyPassphrase);
    }
    let message = Message::new_literal_bytes("capsule.eml", plain);
    let mut rng = StdRng::from_entropy();
    let password = passphrase.to_string();
    let encrypted = message
        .encrypt_with_password_seipdv1(
            &mut rng,
            StringToKey::IteratedAndSalted {
                hash_alg: HashAlgorithm::SHA2_256,
                salt: rand::random(),
                count: 96,
            },
            SymmetricKeyAlgorithm::AES256,
            move || password.clone(),
        )
        .map_err(|err| CryptoError::Encryption(err.to_string()))?;
    encrypted
        .to_armored_string(Default::default())
        .map_err(|err| CryptoError::Encryption(err.to_string()))
}

/// Decrypt an armored message produced by [`encrypt_to_armored`].
pub fn decrypt_from_armored(armored: &str, passphrase: &str) -> Result<Vec<u8>, CryptoError> {
    if passphrase.is_empty() {
        return Err(CryptoError::EmptyPassphrase);
    }
    let password = passphrase.to_string();
    let (message, _headers) = Message::from_string(armored.trim())
        .map_err(|err| CryptoError::Decryption(err.to_string()))?;
    let decrypted = message
        .decrypt_with_password(move || password.clone())
        .map_err(|_| CryptoError::Decryption("wrong passphrase or corrupted data".into()))?;
    decrypted
        .get_content()
        .map_err(|err| CryptoError::Decryption(err.to_string()))?
        .ok_or_else(|| CryptoError::Decryption("message carries no literal data".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_EML: &str = "From: a@b.c\r\nTo: d@e.f\r\nSubject: Encrypted test\r\nMessage-ID: <t1@local>\r\n\r\nHello, secret world!\r\n";

    #[test]
    fn roundtrip_recovers_eml_bytes() {
        let pass = "correct horse battery staple";
        let armored = encrypt_to_armored(SAMPLE_EML.as_bytes(), pass).expect("encrypt");
        assert!(armored.starts_with("-----BEGIN PGP MESSAGE-----"));
        assert!(armored.trim().ends_with("-----END PGP MESSAGE-----"));
        assert!(!armored.contains("secret world"), "plaintext must not leak");

        let plain = decrypt_from_armored(&armored, pass).expect("decrypt");
        assert_eq!(plain, SAMPLE_EML.as_bytes());
    }

    #[test]
    fn wrong_passphrase_is_rejected() {
        let armored = encrypt_to_armored(SAMPLE_EML.as_bytes(), "right").expect("encrypt");
        let err = decrypt_from_armored(&armored, "wrong").unwrap_err();
        assert!(matches!(err, CryptoError::Decryption(_)));
    }

    #[test]
    fn tampered_armored_is_rejected() {
        let armored = encrypt_to_armored(SAMPLE_EML.as_bytes(), "right").expect("encrypt");
        let broken = armored.replace("-----BEGIN PGP MESSAGE-----", "not armor at all");
        assert!(decrypt_from_armored(&broken, "right").is_err());
    }

    #[test]
    fn empty_passphrase_is_refused() {
        assert_eq!(
            encrypt_to_armored(b"x", ""),
            Err(CryptoError::EmptyPassphrase)
        );
        assert_eq!(
            decrypt_from_armored("y", ""),
            Err(CryptoError::EmptyPassphrase)
        );
    }
}
