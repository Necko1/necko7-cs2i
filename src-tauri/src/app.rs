use crate::{
    cloud,
    settings::{Pairing, Settings},
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tauri::{AppHandle, Manager};
use tokio::sync::{watch, Mutex as AsyncMutex};
use tokio_util::sync::CancellationToken;
pub type Shared = Arc<App>;
pub struct App {
    pub inner: Mutex<Inner>,
    pub quitting: AtomicBool,
    pub heartbeat_requested: tokio::sync::Notify,
    pub operations: AsyncMutex<()>,
    pub client: reqwest::Client,
    pub base: Result<String, String>,
    pub path: PathBuf,
    pub port: u16,
    pub pending: watch::Sender<Option<cloud::Signed>>,
    pub cancel: CancellationToken,
    pub tasks: tokio_util::task::TaskTracker,
}
pub struct Inner {
    pub settings: Settings,
    pub key: Result<ed25519_dalek::SigningKey, String>,
    pub session: uuid::Uuid,
    pub seq: i64,
    pub heartbeat_session: uuid::Uuid,
    pub heartbeat_seq: i64,
    pub gsi: String,
    pub listener: String,
    pub cloud: String,
    pub pairing_code: String,
    pub pairing_busy: bool,
    pub error: Option<String>,
}
#[derive(Serialize)]
pub struct Status {
    pairing: Option<Pairing>,
    minimize_to_tray: bool,
    autostart: bool,
    gsi: String,
    listener: String,
    cloud: String,
    pairing_code: String,
    pairing_busy: bool,
    error: Option<String>,
    identity_error: Option<String>,
    version: &'static str,
}
impl App {
    pub fn new(path: PathBuf) -> Result<(Shared, watch::Receiver<Option<cloud::Signed>>), String> {
        let settings = Settings::load(&path)?;
        let key = crate::identity::load(settings.pairing.is_some());
        settings.save(&path)?;
        let (pending, rx) = watch::channel(None);
        let port = if cfg!(debug_assertions) {
            std::env::var("CS2_GSI_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .filter(|p| *p > 0)
                .unwrap_or(31337)
        } else {
            31337
        };
        Ok((
            Arc::new(Self {
                inner: Mutex::new(Inner {
                    settings,
                    key,
                    session: uuid::Uuid::new_v4(),
                    seq: 0,
                    heartbeat_session: uuid::Uuid::new_v4(),
                    heartbeat_seq: 0,
                    gsi: "Discovering CS2…".into(),
                    listener: "Starting…".into(),
                    cloud: "Waiting for GSI".into(),
                    pairing_code: String::new(),
                    pairing_busy: false,
                    error: None,
                }),
                quitting: AtomicBool::new(false),
                heartbeat_requested: tokio::sync::Notify::new(),
                operations: AsyncMutex::new(()),
                client: reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(8))
                    .redirect(reqwest::redirect::Policy::none())
                    .build()
                    .map_err(|_| "Cannot initialize networking")?,
                base: cloud::base_url(),
                path,
                port,
                pending,
                cancel: CancellationToken::new(),
                tasks: tokio_util::task::TaskTracker::new(),
            }),
            rx,
        ))
    }
    pub fn discover(&self) -> Result<(), String> {
        let mut inner = self.inner.lock().unwrap();
        let result = crate::steam::discover().and_then(|dir| {
            crate::gsi_config::install(&dir, &inner.settings.local_token, self.port)
        });
        match result {
            Ok(path) => {
                if let Some(old) = &inner.settings.config_path {
                    if *old != path {
                        crate::gsi_config::remove_owned(
                            old,
                            &inner.settings.local_token,
                            self.port,
                        );
                    }
                }
                inner.settings.config_path = Some(path);
                inner.settings.save(&self.path)?;
                inner.gsi = "GSI config installed — restart CS2 if it was running".into();
                Ok(())
            }
            Err(error) => {
                inner.gsi = error.clone();
                Err(error)
            }
        }
    }
    pub fn signed(
        &self,
        gsi: Option<serde_json::Value>,
        action: Option<&'static str>,
    ) -> Result<cloud::Signed, String> {
        let mut inner = self.inner.lock().unwrap();
        let device = inner
            .settings
            .pairing
            .as_ref()
            .ok_or("Pair a channel first")?
            .device_id;
        let (session_id, seq) = if action == Some("heartbeat") {
            inner.heartbeat_seq = inner
                .heartbeat_seq
                .checked_add(1)
                .ok_or("Heartbeat sequence exhausted")?;
            (inner.heartbeat_session, inner.heartbeat_seq)
        } else {
            inner.seq = inner
                .seq
                .checked_add(1)
                .ok_or("Sequence exhausted; restart the app")?;
            (inner.session, inner.seq)
        };
        cloud::sign(
            inner.key.as_ref().map_err(Clone::clone)?,
            &cloud::Envelope {
                device_id: device,
                session_id,
                seq,
                sent_at: chrono::Utc::now(),
                gsi,
                action,
            },
        )
    }
    pub fn clear_pairing(&self) -> Result<(), String> {
        let mut inner = self.inner.lock().unwrap();
        inner.settings.pairing = None;
        inner.pairing_code.clear();
        inner.error = None;
        self.pending.send_replace(None);
        inner.settings.save(&self.path)
    }
}
pub fn show(app: &AppHandle) {
    if let Some(state) = app.try_state::<Shared>() {
        state.heartbeat_requested.notify_one();
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}
pub fn shutdown(app: &AppHandle) {
    let state = app.state::<Shared>().inner().clone();
    if state.quitting.swap(true, Ordering::SeqCst) {
        return;
    }
    state.cancel.cancel();
    state.tasks.close();
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        // Bound shutdown even if a local HTTP peer leaves an incomplete request.
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), state.tasks.wait()).await;
        handle.exit(0);
    });
}
#[tauri::command]
pub fn status(app: AppHandle, state: tauri::State<'_, Shared>) -> Status {
    use tauri_plugin_autostart::ManagerExt;
    let inner = state.inner.lock().unwrap();
    let gsi = if inner
        .settings
        .config_path
        .as_ref()
        .is_some_and(|p| !p.exists())
    {
        "GSI config missing — Retry discovery".into()
    } else {
        inner.gsi.clone()
    };
    Status {
        pairing: inner.settings.pairing.clone(),
        minimize_to_tray: inner.settings.minimize_to_tray,
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        gsi,
        listener: inner.listener.clone(),
        cloud: inner.cloud.clone(),
        pairing_code: inner.pairing_code.clone(),
        pairing_busy: inner.pairing_busy,
        error: inner.error.clone(),
        identity_error: inner.key.as_ref().err().cloned(),
        version: env!("CARGO_PKG_VERSION"),
    }
}
pub async fn pair_device(state: Shared, code: String) -> Result<(), String> {
    let _guard = state.operations.lock().await;
    let code = crate::deep_link::normalize(&code)?;
    if state.inner.lock().unwrap().settings.pairing.is_some() {
        crate::heartbeat::check(&state).await?;
    }
    let public_key = {
        let mut inner = state.inner.lock().unwrap();
        if inner.settings.pairing.is_some() {
            return Err("Already paired. Unpair before connecting another channel.".into());
        }
        let public = STANDARD.encode(
            inner
                .key
                .as_ref()
                .map_err(Clone::clone)?
                .verifying_key()
                .as_bytes(),
        );
        inner.pairing_busy = true;
        inner.pairing_code = code.clone();
        inner.error = None;
        public
    };
    let result: Result<Pairing,String> = async {
        let base = state.base.as_ref().map_err(Clone::clone)?;
        let mut response = state.client.post(format!("{base}/api/v1/cs2/devices/pair"))
            .json(&serde_json::json!({ "code": code, "public_key": public_key, "app_version": env!("CARGO_PKG_VERSION") }))
            .send().await.map_err(|_| "Pairing server unavailable or timed out")?;
        if !response.status().is_success() { return Err(match response.status().as_u16() { 400|401|422 => "Pairing code invalid or expired. Get a new code in the dashboard.", 429 => "Too many attempts. Wait a minute and retry.", _ => "Pairing server rejected the request. Try again." }.into()); }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| "Cannot read pairing response")? { if bytes.len()+chunk.len()>16384 { return Err("Invalid pairing response".into()); } bytes.extend_from_slice(&chunk); }
        let pairing: Pairing = serde_json::from_slice(&bytes).map_err(|_| "Invalid pairing response")?;
        if pairing.channel.username.len() > 100 || pairing.channel.display_name.len() > 100 || pairing.channel.twitch_id.len() > 32 { return Err("Invalid channel metadata".into()); }
        Ok(pairing)
    }.await;
    let mut inner = state.inner.lock().unwrap();
    inner.pairing_busy = false;
    match result {
        Ok(pairing) => {
            inner.settings.pairing = Some(pairing);
            inner.pairing_code.clear();
            inner.cloud = "Paired — checking desktop connection".into();
            state.heartbeat_requested.notify_one();
            inner
                .settings
                .save(&state.path)
                .inspect_err(|e| inner.error = Some(e.clone()))
        }
        Err(error) => {
            inner.error = Some(error.clone());
            Err(error)
        }
    }
}
#[tauri::command]
pub async fn pair(state: tauri::State<'_, Shared>, code: String) -> Result<(), String> {
    pair_device(state.inner().clone(), code).await
}
#[tauri::command]
pub async fn unpair(state: tauri::State<'_, Shared>) -> Result<(), String> {
    let _guard = state.operations.lock().await;
    let signed = state.signed(None, Some("unpair"))?;
    let response = cloud::send(
        &state.client,
        state.base.as_ref().map_err(Clone::clone)?,
        "cs2/devices/unpair",
        signed,
    )
    .await?;
    if response.is_success() || matches!(response.as_u16(), 401 | 403) {
        state.clear_pairing()?;
        state.inner.lock().unwrap().cloud = "Unpaired".into();
        Ok(())
    } else {
        Err("Unable to revoke device. Retry when the cloud is available, or unpair in the dashboard.".into())
    }
}
#[tauri::command]
pub async fn retry_discovery(state: tauri::State<'_, Shared>) -> Result<(), String> {
    let state = state.inner().clone();
    tokio::task::spawn_blocking(move || state.discover())
        .await
        .map_err(|_| "Discovery failed")?
}
#[tauri::command]
pub fn preferences(
    app: AppHandle,
    state: tauri::State<'_, Shared>,
    autostart: bool,
    minimize: bool,
) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    if autostart {
        app.autolaunch().enable()
    } else {
        app.autolaunch().disable()
    }
    .map_err(|_| "Cannot change Windows startup setting")?;
    let mut inner = state.inner.lock().unwrap();
    inner.settings.minimize_to_tray = minimize;
    inner.settings.save(&state.path)
}
#[tauri::command]
pub async fn reset_identity(state: tauri::State<'_, Shared>) -> Result<(), String> {
    let _guard = state.operations.lock().await;
    let mut inner = state.inner.lock().unwrap();
    if inner.key.is_ok() {
        return Err("Identity is healthy. Use Unpair to change channel.".into());
    }
    inner.key = crate::identity::reset();
    inner.key.as_ref().map_err(Clone::clone)?;
    inner.settings.pairing = None;
    inner.error = None;
    inner.settings.save(&state.path)
}
pub fn deep_link(app: &AppHandle, input: &str) {
    show(app);
    let Some(state) = app.try_state::<Shared>() else {
        return;
    };
    match crate::deep_link::parse(input) {
        Ok(code) => {
            state.inner.lock().unwrap().pairing_code = code.clone();
            let state = state.inner().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = pair_device(state.clone(), code).await {
                    state.inner.lock().unwrap().error = Some(error);
                }
            });
        }
        Err(error) => state.inner.lock().unwrap().error = Some(error),
    }
}
