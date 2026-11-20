//! AfterTone Application Settings & Custom Addon Sources

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddonSourceConfig {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub enabled: bool,
    pub allow_downloads: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub audio_quality: String, // "high", "auto", "low"
    pub crossfade_sec: u32,    // 0, 3, 6, 9, 12
    pub automix: bool,
    pub automix_bars: u32,
    pub show_nerd_stats: bool,
    pub sleep_timer_minutes: Option<u32>,
    pub sleep_timer_deadline_ms: Option<i64>,
    pub sleep_timer_after_track: bool,
    pub discord_rpc_enabled: bool,
    pub jiosaavn_enabled: bool,
    pub sources: Vec<AddonSourceConfig>,
    pub lyrics_source_priority: Vec<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            audio_quality: "high".to_string(),
            crossfade_sec: 3,
            automix: false,
            automix_bars: 8,
            show_nerd_stats: false,
            sleep_timer_minutes: None,
            sleep_timer_deadline_ms: None,
            sleep_timer_after_track: false,
            discord_rpc_enabled: true,
            jiosaavn_enabled: true,
            sources: vec![
                AddonSourceConfig {
                    id: "jiosaavn".to_string(),
                    name: "JioSaavn (320kbps High Quality)".to_string(),
                    base_url: "https://www.jiosaavn.com".to_string(),
                    enabled: true,
                    allow_downloads: true,
                },
            ],
            lyrics_source_priority: vec![
                "lrc-red".to_string(),
                "bini".to_string(),
                "better-lyrics".to_string(),
                "unison".to_string(),
                "kugou".to_string(),
                "lrclib".to_string(),
                "genius".to_string(),
            ],
        }
    }
}

pub struct SettingsManager {
    settings_file: PathBuf,
    settings: Arc<RwLock<AppSettings>>,
}

impl SettingsManager {
    pub fn new() -> Arc<Self> {
        let settings_file = config_file_path();
        let loaded = if settings_file.exists() {
            std::fs::read_to_string(&settings_file)
                .ok()
                .and_then(|s| serde_json::from_str::<AppSettings>(&s).ok())
                .unwrap_or_default()
        } else {
            AppSettings::default()
        };

        Arc::new(Self {
            settings_file,
            settings: Arc::new(RwLock::new(loaded)),
        })
    }

    pub async fn get(&self) -> AppSettings {
        self.settings.read().await.clone()
    }

    pub async fn update(&self, new_settings: AppSettings) {
        {
            let mut write = self.settings.write().await;
            *write = new_settings.clone();
        }
        if let Some(parent) = self.settings_file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json_str) = serde_json::to_string_pretty(&new_settings) {
            let _ = std::fs::write(&self.settings_file, json_str);
        }
    }
}

fn config_file_path() -> PathBuf {
    if let Ok(appdata) = std::env::var("APPDATA") {
        return PathBuf::from(appdata).join("AfterTone").join("settings.json");
    }
    PathBuf::from("settings.json")
}
