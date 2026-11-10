use hmac::{Hmac, Mac};
use secrecy::{ExposeSecret, SecretString};
use sha2::Sha256;

use crate::error::GhCoreError;

type HmacSha256 = Hmac<Sha256>;

/// Verifies GitHub webhook signature from the `X-Hub-Signature-256` header against the raw payload bytes.
/// Comparison is executed in constant time.
pub fn verify_webhook_signature(
    secret: &SecretString,
    signature_header: &str,
    raw_payload: &[u8],
) -> Result<(), GhCoreError> {
    let hex_sig = signature_header
        .strip_prefix("sha256=")
        .ok_or_else(|| GhCoreError::InvalidHeader("Signature header must start with 'sha256='".to_string()))?;

    let expected_sig = hex::decode(hex_sig.trim())?;

    let mut mac = HmacSha256::new_from_slice(secret.expose_secret().as_bytes())
        .map_err(|_| GhCoreError::HmacInvalidKeyLength)?;
    mac.update(raw_payload);

    mac.verify_slice(&expected_sig)
        .map_err(|_| GhCoreError::InvalidSignature)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifies_valid_signature() {
        let secret = SecretString::from("test-secret".to_string());
        let payload = b"{\"action\":\"opened\"}";

        // Compute valid signature
        let mut mac = HmacSha256::new_from_slice(secret.expose_secret().as_bytes()).unwrap();
        mac.update(payload);
        let valid_hex = hex::encode(mac.finalize().into_bytes());
        let header = format!("sha256={valid_hex}");

        assert!(verify_webhook_signature(&secret, &header, payload).is_ok());
    }

    #[test]
    fn rejects_tampered_payload() {
        let secret = SecretString::from("test-secret".to_string());
        let payload = b"{\"action\":\"opened\"}";

        let mut mac = HmacSha256::new_from_slice(secret.expose_secret().as_bytes()).unwrap();
        mac.update(payload);
        let valid_hex = hex::encode(mac.finalize().into_bytes());
        let header = format!("sha256={valid_hex}");

        let tampered_payload = b"{\"action\":\"closed\"}";
        assert!(matches!(
            verify_webhook_signature(&secret, &header, tampered_payload),
            Err(GhCoreError::InvalidSignature)
        ));
    }

    #[test]
    fn rejects_missing_sha256_prefix() {
        let secret = SecretString::from("test-secret".to_string());
        let payload = b"test";
        let res = verify_webhook_signature(&secret, "deadbeef", payload);
        assert!(matches!(res, Err(GhCoreError::InvalidHeader(_))));
    }
}
