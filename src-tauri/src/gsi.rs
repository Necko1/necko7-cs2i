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
    Json(mut payload): Json<serde_json::Value>,
) -> StatusCode {
    if !authenticated(&payload, &state.inner.lock().unwrap().settings.local_token) {
        return StatusCode::UNAUTHORIZED;
    }
    // Local credentials must never be sent to cloud.
    if let Some(obj) = payload.as_object_mut() {
        obj.remove("auth");
    }
    state.inner.lock().unwrap().listener = "Receiving CS2 GSI".into();
    if let Ok(signed) = state.signed(Some(payload), None) {
        state.pending.send_replace(Some(signed));
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
pub async fn forward(
    state: Shared,
    mut rx: tokio::sync::watch::Receiver<Option<crate::cloud::Signed>>,
) {
    loop {
        tokio::select! { _ = state.cancel.cancelled() => break, changed = rx.changed() => { if changed.is_err() { break; } } }
        let signed = rx.borrow_and_update().clone();
        let Some(signed) = signed else {
            continue;
        };
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
                tokio::select! { _ = state.cancel.cancelled() => break, result = crate::cloud::send(&state.client,base,"cs2/gsi",signed) => result }
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
        state.inner.lock().unwrap().cloud = status;
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
}
