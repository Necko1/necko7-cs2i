use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{Signer, SigningKey};
use serde::Serialize;
use uuid::Uuid;
#[derive(Serialize)]
pub struct Envelope {
    pub device_id: Uuid,
    pub session_id: Uuid,
    pub seq: i64,
    pub sent_at: chrono::DateTime<chrono::Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gsi: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<&'static str>,
}
#[derive(Clone)]
pub struct Signed {
    pub device_id: Uuid,
    pub body: Vec<u8>,
    pub signature: String,
}
pub fn sign(key: &SigningKey, envelope: &Envelope) -> Result<Signed, String> {
    let body = serde_json::to_vec(envelope).map_err(|_| "Cannot serialize GSI")?;
    let signature = STANDARD.encode(key.sign(&body).to_bytes());
    Ok(Signed {
        device_id: envelope.device_id,
        body,
        signature,
    })
}
pub fn base_url() -> Result<String, String> {
    let raw = std::env::var("CS2_API_URL").unwrap_or_else(|_| "https://7.necko.moe".into());
    let url = url::Url::parse(&raw).map_err(|_| "Invalid CS2_API_URL")?;
    let local = cfg!(debug_assertions)
        && url.scheme() == "http"
        && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
    if (url.scheme() != "https" && !local)
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err(
            "CS2_API_URL must be an HTTPS origin (debug builds also allow HTTP localhost)".into(),
        );
    }
    Ok(raw.trim_end_matches('/').to_string())
}
pub async fn send(
    client: &reqwest::Client,
    base: &str,
    path: &str,
    signed: Signed,
) -> Result<reqwest::StatusCode, String> {
    client
        .post(format!("{base}/api/v1/{path}"))
        .header("Content-Type", "application/json")
        .header("X-Necko7-Signature", signed.signature)
        .body(signed.body)
        .send()
        .await
        .map(|r| r.status())
        .map_err(|_| "Cloud unavailable or timed out".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_envelope_bytes() {
        let key = SigningKey::from_bytes(&[42; 32]);
        let signed = sign(
            &key,
            &Envelope {
                device_id: Uuid::nil(),
                session_id: Uuid::nil(),
                seq: 1,
                sent_at: chrono::Utc::now(),
                gsi: Some(serde_json::json!({"player":{"name":"Test"}})),
                action: None,
            },
        )
        .unwrap();
        let sig =
            ed25519_dalek::Signature::from_slice(&STANDARD.decode(&signed.signature).unwrap())
                .unwrap();
        assert!(key
            .verifying_key()
            .verify_strict(&signed.body, &sig)
            .is_ok());
        let mut altered = signed.body.clone();
        altered.push(b' ');
        assert!(key.verifying_key().verify_strict(&altered, &sig).is_err());
        let parsed: serde_json::Value = serde_json::from_slice(&signed.body).unwrap();
        assert_eq!(parsed["seq"], 1);
        assert!(parsed.get("action").is_none());
    }
}
