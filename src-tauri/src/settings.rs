use base64::{engine::general_purpose::STANDARD, Engine};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Serialize, Deserialize)]
pub struct Channel {
    pub twitch_id: String,
    pub username: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Pairing {
    pub device_id: uuid::Uuid,
    pub channel: Channel,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Settings {
    pub pairing: Option<Pairing>,
    pub minimize_to_tray: bool,
    pub local_token: String,
    pub config_path: Option<PathBuf>,
}
impl Settings {
    pub fn load(path: &Path) -> Result<Self, String> {
        if path.exists() {
            return serde_json::from_slice(&std::fs::read(path).map_err(|_| "Cannot read settings")?).map_err(|_| "Settings are corrupted; restore or rename settings.json and revoke the old device in the dashboard".into());
        }
        let mut token = [0u8; 32];
        OsRng.fill_bytes(&mut token);
        Ok(Self {
            pairing: None,
            minimize_to_tray: true,
            local_token: STANDARD.encode(token),
            config_path: None,
        })
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(self).map_err(|_| "Cannot serialize settings")?;
        let temp = path.with_extension("tmp");
        std::fs::write(&temp, bytes).map_err(|_| "Cannot write settings")?;
        std::fs::rename(&temp, path).map_err(|_| "Cannot replace settings".into())
    }
}
