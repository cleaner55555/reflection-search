//! Polar billing webhook: prepaid AI krediti iz plaćenih porudžbina.
//!
//! Protokol je Standard Webhooks: `webhook-id`, `webhook-timestamp`,
//! `webhook-signature: v1,<base64>` nad `"{id}.{ts}.{body}"`, HMAC-SHA256
//! ključem od celog `whsec_...` sekreta (Polar specifičnost).
//! Bez `POLAR_WEBHOOK_SECRET` ruta je ugašena (503), kao auth/AI sloj.

use base64::Engine as _;
use hmac::{Hmac, Mac};
use sha2::Sha256;

/// Dozvoljena starost događaja (Polar preporuka).
pub const TIMESTAMP_TOLERANCE_SECS: u64 = 300;

/// Greške verifikacije — klijentu se ne otkrivaju detalji.
#[derive(Debug, thiserror::Error)]
pub enum BillingError {
    /// Neispravan potpis ili zastareo timestamp.
    #[error("bad signature")]
    BadSignature,
    /// Nevalidan JSON / nepoznat oblik.
    #[error("bad payload")]
    BadPayload,
}

/// Proverava potpis i starost; vraća tip događaja.
pub fn verify(
    secret: &str,
    msg_id: &str,
    timestamp: &str,
    signature: &str,
    body: &[u8],
) -> Result<String, BillingError> {
    let ts: u64 = timestamp.parse().map_err(|_| BillingError::BadSignature)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| BillingError::BadSignature)?
        .as_secs();
    if now.saturating_sub(ts) > TIMESTAMP_TOLERANCE_SECS || ts.saturating_sub(now) > 60 {
        return Err(BillingError::BadSignature);
    }
    let signed = format!("{msg_id}.{timestamp}.{}", String::from_utf8_lossy(body));
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .map_err(|_| BillingError::BadSignature)?;
    mac.update(signed.as_bytes());
    let ok = signature
        .split_whitespace()
        .filter_map(|entry| entry.strip_prefix("v1,"))
        .any(|b64| {
            base64::engine::general_purpose::STANDARD
                .decode(b64.trim())
                .is_ok_and(|sig| mac.clone().verify_slice(&sig).is_ok())
        });
    if !ok {
        return Err(BillingError::BadSignature);
    }
    let event: serde_json::Value =
        serde_json::from_slice(body).map_err(|_| BillingError::BadPayload)?;
    event
        .get("type")
        .and_then(|t| t.as_str())
        .map(str::to_string)
        .ok_or(BillingError::BadPayload)
}

/// Kredit u USD iz `order.paid` događaja: `data.metadata.credits_usd`,
/// inače `data.amount/100` (centi). Vraća `None` kad nema iznosa.
pub fn credits_for(event: &serde_json::Value) -> Option<f64> {
    let data = event.get("data")?;
    if let Some(c) = data
        .get("metadata")
        .and_then(|m| m.get("credits_usd"))
        .and_then(serde_json::Value::as_f64)
    {
        return (c.is_finite() && c > 0.0).then_some(c);
    }
    data.get("amount")
        .and_then(serde_json::Value::as_f64)
        .map(|cents| cents / 100.0)
        .filter(|usd| usd.is_finite() && *usd > 0.0)
}

/// Email kupca iz `order.paid` događaja.
pub fn customer_email(event: &serde_json::Value) -> Option<String> {
    event
        .get("data")?
        .get("customer")?
        .get("email")?
        .as_str()
        .map(|e| e.trim().to_lowercase())
        .filter(|e| !e.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sign(secret: &str, id: &str, ts: &str, body: &[u8]) -> String {
        let signed = format!("{id}.{ts}.{}", String::from_utf8_lossy(body));
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("mac");
        mac.update(signed.as_bytes());
        let sig = base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());
        format!("v1,{sig}")
    }

    const BODY: &str = r#"{"type":"order.paid","data":{"amount":500,"metadata":{"credits_usd":5.0},"customer":{"email":"U@X.com"}}}"#;

    #[test]
    fn valid_signature_returns_type() {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_secs()
            .to_string();
        let sig = sign("whsec_test", "msg_1", &ts, BODY.as_bytes());
        let ty = verify("whsec_test", "msg_1", &ts, &sig, BODY.as_bytes()).expect("verify");
        assert_eq!(ty, "order.paid");
    }

    #[test]
    fn wrong_secret_rejected() {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_secs()
            .to_string();
        let sig = sign("whsec_test", "msg_1", &ts, BODY.as_bytes());
        assert!(verify("whsec_other", "msg_1", &ts, &sig, BODY.as_bytes()).is_err());
    }

    #[test]
    fn stale_timestamp_rejected() {
        let sig = sign("whsec_test", "msg_1", "1000000000", BODY.as_bytes());
        assert!(verify("whsec_test", "msg_1", "1000000000", &sig, BODY.as_bytes()).is_err());
    }

    #[test]
    fn credits_from_metadata_then_amount() {
        let v: serde_json::Value = serde_json::from_str(BODY).expect("json");
        assert!((credits_for(&v).expect("credits") - 5.0).abs() < 1e-9);
        assert_eq!(customer_email(&v).expect("email"), "u@x.com");
        let v2: serde_json::Value = serde_json::from_str(
            r#"{"type":"order.paid","data":{"amount":250,"customer":{"email":"a@b.com"}}}"#,
        )
        .expect("json");
        assert!((credits_for(&v2).expect("credits") - 2.5).abs() < 1e-9);
    }
}
