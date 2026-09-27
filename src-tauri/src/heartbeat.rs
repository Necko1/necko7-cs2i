use crate::{app::Shared, cloud};
use std::time::Duration;

// Caller owns operations: pairing, revocation and forwarding cannot race this check.
pub async fn check(state: &Shared) -> Result<(), String> {
    if state.inner.lock().unwrap().settings.pairing.is_none() {
        return Ok(());
    }
    let signed = state.signed(None, Some("heartbeat"))?;
    let result = cloud::send(
        &state.client,
        state.base.as_ref().map_err(Clone::clone)?,
        "cs2/devices/heartbeat",
        signed,
    )
    .await;
    match result {
        Ok(status) if matches!(status.as_u16(), 401 | 403) => {
            state.clear_pairing()?;
            state.inner.lock().unwrap().cloud = "Device revoked — pair again".into();
            Ok(())
        }
        Ok(status) if status.is_success() => {
            state.inner.lock().unwrap().cloud = "Desktop connected".into();
            Ok(())
        }
        Ok(status) => Err(format!(
            "Desktop status check rejected ({}). Check the clock and backend; retry.",
            status.as_u16()
        )),
        Err(error) => Err(error),
    }
}
pub async fn run(state: Shared) {
    let mut timer = tokio::time::interval(Duration::from_secs(45));
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last = None;
    loop {
        tokio::select! {
            _ = state.cancel.cancelled() => break,
            _ = timer.tick() => {},
            _ = state.heartbeat_requested.notified() => {},
        }
        if state.inner.lock().unwrap().settings.pairing.is_none() {
            continue;
        }
        if last.is_some_and(|t: std::time::Instant| t.elapsed() < Duration::from_secs(5)) {
            continue;
        }
        let _guard = state.operations.lock().await;
        last = Some(std::time::Instant::now());
        tokio::select! {
            _ = state.cancel.cancelled() => break,
            result = check(&state) => if let Err(error) = result { state.inner.lock().unwrap().cloud = error; }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::{App, Inner},
        settings::{Channel, Pairing, Settings},
    };
    use axum::{
        body::Bytes,
        http::{HeaderMap, StatusCode},
        routing::post,
        Router,
    };
    use base64::{engine::general_purpose::STANDARD, Engine};
    use std::sync::{atomic::AtomicBool, Arc, Mutex};

    #[tokio::test]
    async fn revoked_heartbeat_clears_channel_but_preserves_identity_and_gsi_then_repairs() {
        let key = ed25519_dalek::SigningKey::from_bytes(&[42; 32]);
        let public = key.verifying_key();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let response_status = Arc::new(std::sync::atomic::AtomicU16::new(401));
        let heartbeat_status = response_status.clone();
        let router = Router::new()
            .route("/api/v1/cs2/devices/heartbeat", post(move |headers: HeaderMap, body: Bytes| {
                    let heartbeat_status = heartbeat_status.clone();
                    async move {
                let signature = STANDARD.decode(headers["x-necko7-signature"].to_str().unwrap()).unwrap();
                public.verify_strict(&body, &ed25519_dalek::Signature::from_slice(&signature).unwrap()).unwrap();
                let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(payload["action"], "heartbeat");
                assert!(payload["seq"].as_i64().unwrap() > 0);
                assert!(payload.get("gsi").is_none());
                StatusCode::from_u16(heartbeat_status.load(std::sync::atomic::Ordering::SeqCst)).unwrap()
            }}))
            .route("/api/v1/cs2/devices/pair", post(|| async {
                axum::Json(serde_json::json!({"device_id": uuid::Uuid::new_v4(), "channel": {"twitch_id":"new", "username":"new", "display_name":"New", "avatar_url":null}}))
            }));
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let dir = std::env::temp_dir().join(format!("necko7-heartbeat-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("settings.json");
        let mut settings = Settings::load(&path).unwrap();
        let config = crate::gsi_config::install(&dir, &settings.local_token, 31337).unwrap();
        settings.config_path = Some(config.clone());
        settings.pairing = Some(Pairing {
            device_id: uuid::Uuid::new_v4(),
            channel: Channel {
                twitch_id: "old".into(),
                username: "old".into(),
                display_name: "Old".into(),
                avatar_url: None,
            },
        });
        let (pending, _) = tokio::sync::watch::channel(None);
        let state = Arc::new(App {
            inner: Mutex::new(Inner {
                settings,
                key: Ok(key),
                session: uuid::Uuid::new_v4(),
                seq: 0,
                heartbeat_session: uuid::Uuid::new_v4(),
                heartbeat_seq: 0,
                gsi: String::new(),
                listener: String::new(),
                cloud: String::new(),
                pairing_code: String::new(),
                pairing_busy: false,
                error: None,
            }),
            quitting: AtomicBool::new(false),
            heartbeat_requested: tokio::sync::Notify::new(),
            operations: tokio::sync::Mutex::new(()),
            client: reqwest::Client::new(),
            base: Ok(base),
            path: path.clone(),
            port: 31337,
            pending,
            cancel: tokio_util::sync::CancellationToken::new(),
            tasks: tokio_util::task::TaskTracker::new(),
        });
        check(&state).await.unwrap();
        assert_eq!(state.inner.lock().unwrap().seq, 0);
        assert!(state.inner.lock().unwrap().settings.pairing.is_none());
        assert!(Settings::load(&path).unwrap().pairing.is_none());
        assert_eq!(
            state
                .inner
                .lock()
                .unwrap()
                .key
                .as_ref()
                .unwrap()
                .verifying_key(),
            public
        );
        assert!(config.exists());
        crate::app::pair_device(
            state.clone(),
            crate::deep_link::parse("necko7-cs2i://pair?code=4EME-GX7G").unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(
            state
                .inner
                .lock()
                .unwrap()
                .settings
                .pairing
                .as_ref()
                .unwrap()
                .channel
                .twitch_id,
            "new"
        );
        response_status.store(204, std::sync::atomic::Ordering::SeqCst);
        let already_paired = crate::app::pair_device(state.clone(), "4EME-GX7G".into())
            .await
            .unwrap_err();
        assert!(already_paired.contains("Already paired"));
        response_status.store(503, std::sync::atomic::Ordering::SeqCst);
        assert!(crate::app::pair_device(state.clone(), "4EME-GX7G".into())
            .await
            .is_err());
        assert!(state.inner.lock().unwrap().settings.pairing.is_some());
        response_status.store(401, std::sync::atomic::Ordering::SeqCst);
        // Exercise reconciliation of stale paired state on a subsequent valid link.
        crate::app::pair_device(
            state.clone(),
            crate::deep_link::parse("necko7-cs2i://pair?code=4EME-GX7G").unwrap(),
        )
        .await
        .unwrap();
        assert!(state.inner.lock().unwrap().settings.pairing.is_some());
        server.abort();
        std::fs::remove_file(config).unwrap();
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
}
