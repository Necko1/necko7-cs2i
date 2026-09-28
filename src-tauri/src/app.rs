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
use tokio::sync::{mpsc, Mutex as AsyncMutex};
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
    pub pending: mpsc::Sender<cloud::Signed>,
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
    pub last_gsi: Option<std::time::Instant>,
    pub last_gsi_at: Option<chrono::DateTime<chrono::Utc>>,
    pub forwarding_error: Option<String>,
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
    gsi_active: bool,
    last_gsi_at: Option<chrono::DateTime<chrono::Utc>>,
    forwarding_error: Option<String>,
    pairing_code: String,
    pairing_busy: bool,
    error: Option<String>,
    identity_error: Option<String>,
    version: &'static str,
}
impl App {
    pub fn new(path: PathBuf) -> Result<(Shared, mpsc::Receiver<cloud::Signed>), String> {
        let settings = Settings::load(&path)?;
        let key = if development_dir().is_some() {
            Ok(ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng))
        } else {
            crate::identity::load(settings.pairing.is_some())
        };
        settings.save(&path)?;
        let (pending, rx) = mpsc::channel(64);
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
                    last_gsi: None,
                    last_gsi_at: None,
                    forwarding_error: None,
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
        let directory = development_dir()
            .map(|dir| dir.join("cfg"))
            .map(Ok)
            .unwrap_or_else(crate::steam::discover);
        let result = directory.and_then(|dir| {
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
        inner.last_gsi = None;
        inner.last_gsi_at = None;
        inner.forwarding_error = None;
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
        gsi_active: gsi_fresh(inner.last_gsi.map(|time| time.elapsed())),
        last_gsi_at: inner.last_gsi_at,
        forwarding_error: inner.forwarding_error.clone(),
        pairing_code: inner.pairing_code.clone(),
        pairing_busy: inner.pairing_busy,
        error: inner.error.clone(),
        identity_error: inner.key.as_ref().err().cloned(),
        version: env!("CARGO_PKG_VERSION"),
    }
}

pub fn development_dir() -> Option<PathBuf> {
    if cfg!(debug_assertions) {
        std::env::var_os("CS2_DEVELOPMENT_DIR").map(PathBuf::from)
    } else {
        None
    }
}

fn gsi_fresh(age: Option<std::time::Duration>) -> bool {
    age.is_some_and(|age| age < std::time::Duration::from_secs(60))
}

#[tauri::command]
pub fn open_dashboard(app: AppHandle, state: tauri::State<'_, Shared>) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let base = state.base.as_ref().map_err(Clone::clone)?;
    let origin = std::env::var("CS2_DASHBOARD_URL").unwrap_or_else(|_| base.clone());
    let url = dashboard_url(&origin)?;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|_| "Unable to open the dashboard in your browser".into())
}
fn dashboard_url(origin: &str) -> Result<String, String> {
    let mut url = url::Url::parse(origin).map_err(|_| "Invalid dashboard URL")?;
    if !["https", "http"].contains(&url.scheme())
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("Invalid dashboard URL".into());
    }
    url.set_path("/scripts/cs2");
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.to_string())
}

#[tauri::command]
pub fn open_channel(app: AppHandle, state: tauri::State<'_, Shared>) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let inner = state.inner.lock().unwrap();
    let login = &inner
        .settings
        .pairing
        .as_ref()
        .ok_or("Pair a channel first")?
        .channel
        .username;
    if !login
        .bytes()
        .all(|c| c.is_ascii_alphanumeric() || c == b'_')
    {
        return Err("Invalid channel login".into());
    }
    app.opener()
        .open_url(format!("https://www.twitch.tv/{login}"), None::<&str>)
        .map_err(|_| "Unable to open Twitch in your browser".into())
}

#[cfg(test)]
mod activity_tests {
    use super::*;
    #[test]
    fn local_receipt_freshness_expires_at_sixty_seconds() {
        assert!(!gsi_fresh(None));
        assert!(gsi_fresh(Some(std::time::Duration::from_secs(59))));
        assert!(!gsi_fresh(Some(std::time::Duration::from_secs(60))));
    }
    #[test]
    fn dashboard_uses_configured_origin_without_credentials_or_query_data() {
        assert_eq!(
            dashboard_url("https://dashboard.example.test/old?code=private#frag").unwrap(),
            "https://dashboard.example.test/scripts/cs2"
        );
        assert_eq!(
            dashboard_url("http://127.0.0.1:4173").unwrap(),
            "http://127.0.0.1:4173/scripts/cs2"
        );
        assert!(dashboard_url("https://user:password@example.test").is_err());
        assert!(dashboard_url("javascript:alert(1)").is_err());
    }
    #[test]
    fn main_window_is_fixed_size_and_not_maximizable() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let window = &config["app"]["windows"][0];
        assert_eq!(window["title"], "necko7 CS2");
        assert_eq!(window["width"], 360);
        assert_eq!(window["height"], 280);
        assert_eq!(window["resizable"], false);
        assert_eq!(window["maximizable"], false);
        assert_eq!(window["fullscreen"], false);
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
