use crate::app::Shared;
use axum::{
    extract::{DefaultBodyLimit, State},
    http::StatusCode,
    routing::post,
    Json, Router,
};
use subtle::ConstantTimeEq;
pub fn authenticated(value: &serde_json::Value, token: &str) -> bool {
    value
        .pointer("/auth/token")
        .and_then(|v| v.as_str())
        .is_some_and(|v| bool::from(v.as_bytes().ct_eq(token.as_bytes())))
}
async fn receive(
    State(state): State<Shared>,
    payload: Result<Json<serde_json::Value>, axum::extract::rejection::JsonRejection>,
) -> StatusCode {
    let Json(mut payload) = match payload {
        Ok(payload) => payload,
        Err(_) => {
            tracing::warn!(
                stage = "local_decode",
                reason = "invalid_json",
                "Local GSI request could not be decoded"
            );
            return StatusCode::BAD_REQUEST;
        }
    };
    if !authenticated(&payload, &state.inner.lock().unwrap().settings.local_token) {
        return StatusCode::UNAUTHORIZED;
    }
    // Local credentials must never be sent to cloud.
    if let Some(obj) = payload.as_object_mut() {
        obj.remove("auth");
    }
    {
        let mut inner = state.inner.lock().unwrap();
        inner.listener = "Receiving CS2 GSI".into();
        inner.last_gsi = Some(std::time::Instant::now());
        inner.last_gsi_at = Some(chrono::Utc::now());
    }
    // Preserve every accepted boundary in order; a watch channel coalesces them.
    let permit = match state.pending.try_reserve() {
        Ok(permit) => permit,
        Err(_) => {
            tracing::warn!(
                stage = "local_queue",
                reason = "queue_full",
                "GSI forwarding queue full; request rejected for retry"
            );
            state.inner.lock().unwrap().forwarding_error =
                Some("Forwarding is falling behind. Check your connection.".into());
            return StatusCode::SERVICE_UNAVAILABLE;
        }
    };
    if let Ok(signed) = state.signed(Some(payload), None) {
        permit.send(signed);
    }
    StatusCode::NO_CONTENT
}
pub async fn serve(state: Shared) {
    let listener = match tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, state.port))
        .await
    {
        Ok(listener) => listener,
        Err(_) => {
            state.inner.lock().unwrap().listener =
                "Listener unavailable — port in use. Close the conflicting app and restart.".into();
            return;
        }
    };
    state.inner.lock().unwrap().listener = format!("Listening on 127.0.0.1:{}", state.port);
    let router = Router::new()
        .route("/gsi", post(receive))
        .layer(DefaultBodyLimit::max(256 * 1024))
        .with_state(state.clone());
    let _ = axum::serve(listener, router)
        .with_graceful_shutdown(state.cancel.clone().cancelled_owned())
        .await;
}
pub async fn forward(state: Shared, mut rx: tokio::sync::mpsc::Receiver<crate::cloud::Signed>) {
    loop {
        let signed = tokio::select! { _ = state.cancel.cancelled() => break, signed = rx.recv() => { let Some(signed) = signed else { break }; signed } };
        let diagnostic: serde_json::Value =
            serde_json::from_slice(&signed.body).unwrap_or_default();
        let guard = state.operations.lock().await;
        if state
            .inner
            .lock()
            .unwrap()
            .settings
            .pairing
            .as_ref()
            .map(|p| p.device_id)
            != Some(signed.device_id)
        {
            continue;
        }
        let result = match &state.base {
            Ok(base) => {
                tokio::select! { _ = state.cancel.cancelled() => break, result = crate::cloud::send(&state.client,base,"cs2/gsi",signed.clone()) => result }
            }
            Err(error) => Err(error.clone()),
        };
        let mut backoff = false;
        let status = match result {
            Ok(status) if status.is_success() => "Connected".into(),
            Ok(status) if matches!(status.as_u16(), 401 | 403) => {
                if let Err(error) = state.clear_pairing() {
                    state.inner.lock().unwrap().error = Some(error);
                }
                "Device revoked or rejected — pair again".into()
            }
            Ok(status) => {
                backoff = true;
                format!(
                    "Cloud rejected GSI ({}); check clock and backend",
                    status.as_u16()
                )
            }
            Err(error) => {
                backoff = true;
                error
            }
        };
        if backoff {
            tracing::warn!(device_id=%signed.device_id, session_id=?diagnostic["session_id"], seq=?diagnostic["seq"], stage="forward", reason=%status, "GSI forwarding failed");
        }
        {
            let mut inner = state.inner.lock().unwrap();
            inner.forwarding_error = backoff.then(|| status.clone());
            inner.cloud = status;
        }
        drop(guard);
        if backoff {
            tokio::select! { _ = state.cancel.cancelled() => break, _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {} }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_authentication() {
        assert!(authenticated(
            &serde_json::json!({"auth":{"token":"secret"}}),
            "secret"
        ));
        assert!(!authenticated(
            &serde_json::json!({"auth":{"token":"wrong"}}),
            "secret"
        ));
        assert!(!authenticated(&serde_json::json!({}), "secret"));
    }
    #[tokio::test]
    async fn forwarding_queue_preserves_round_boundaries_and_rejects_overflow() {
        use crate::{
            app::{App, Inner},
            settings::{Channel, Pairing, Settings},
        };
        use std::sync::{atomic::AtomicBool, Arc, Mutex};
        let dir = std::env::temp_dir().join(format!("necko7-queue-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("settings.json");
        let mut settings = Settings::load(&path).unwrap();
        let token = settings.local_token.clone();
        let device = uuid::Uuid::new_v4();
        settings.pairing = Some(Pairing {
            device_id: device,
            channel: Channel {
                twitch_id: "qa".into(),
                username: "qa".into(),
                display_name: "QA".into(),
                avatar_url: None,
            },
        });
        let (pending, mut rx) = tokio::sync::mpsc::channel(2);
        let state = Arc::new(App {
            inner: Mutex::new(Inner {
                settings,
                key: Ok(ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng)),
                session: uuid::Uuid::new_v4(),
                seq: 0,
                heartbeat_session: uuid::Uuid::new_v4(),
                heartbeat_seq: 0,
                gsi: String::new(),
                listener: String::new(),
                cloud: String::new(),
                last_gsi: None,
                last_gsi_at: None,
                forwarding_error: None,
                pairing_code: String::new(),
                pairing_busy: false,
                error: None,
            }),
            quitting: AtomicBool::new(false),
            heartbeat_requested: tokio::sync::Notify::new(),
            operations: tokio::sync::Mutex::new(()),
            client: reqwest::Client::new(),
            base: Ok("http://127.0.0.1:1".into()),
            path,
            port: 31338,
            pending,
            cancel: tokio_util::sync::CancellationToken::new(),
            tasks: tokio_util::task::TaskTracker::new(),
        });
        for round in 0..2 {
            assert_eq!(receive(State(state.clone()),Ok(Json(serde_json::json!({"auth":{"token":token},"map":{"round":round},"round":{"phase":"over"}})))).await,StatusCode::NO_CONTENT);
        }
        assert_eq!(
            receive(
                State(state.clone()),
                Ok(Json(
                    serde_json::json!({"auth":{"token":token},"map":{"round":2}})
                ))
            )
            .await,
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(state.inner.lock().unwrap().seq, 2);
        assert!(state.inner.lock().unwrap().last_gsi.is_some());
        for round in 0..2 {
            let signed = rx.recv().await.unwrap();
            let body: serde_json::Value = serde_json::from_slice(&signed.body).unwrap();
            assert_eq!(body["gsi"]["map"]["round"], round);
            assert_eq!(body["seq"], round + 1);
            assert!(body["gsi"].get("auth").is_none());
            assert_eq!(signed.device_id, device);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
