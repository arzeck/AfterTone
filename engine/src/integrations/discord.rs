//! Discord Rich Presence Integration
//! Communicates with Discord desktop over local IPC socket/named pipe.

use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
use std::sync::Arc;
use tokio::sync::Mutex;

const DISCORD_APP_ID: &str = "1208000000000000000"; // AfterTone Discord Client ID

pub struct DiscordRpcService {
    client: Arc<Mutex<Option<DiscordIpcClient>>>,
}

impl Default for DiscordRpcService {
    fn default() -> Self {
        Self::new()
    }
}

impl DiscordRpcService {
    pub fn new() -> Self {
        Self {
            client: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn connect(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut client_lock = self.client.lock().await;
        if client_lock.is_none() {
            let mut client = DiscordIpcClient::new(DISCORD_APP_ID);
            if client.connect().is_ok() {
                *client_lock = Some(client);
            }
        }
        Ok(())
    }

    pub async fn update_presence(
        &self,
        title: &str,
        artist: &str,
        album: Option<&str>,
        is_playing: bool,
        position_secs: i64,
        duration_secs: i64,
    ) {
        let mut client_lock = self.client.lock().await;
        if let Some(ref mut client) = *client_lock {
            let state_str = if is_playing {
                format!("by {}", artist)
            } else {
                format!("Paused • by {}", artist)
            };

            let details = format!("🎵 {}", title);
            let mut act = activity::Activity::new()
                .state(&state_str)
                .details(&details)
                .assets(activity::Assets::new().large_image("logo").large_text(album.unwrap_or("AfterTone Music")));

            if is_playing && duration_secs > 0 {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;
                let start = now - position_secs;
                let end = start + duration_secs;
                act = act.timestamps(activity::Timestamps::new().start(start).end(end));
            }

            let _ = client.set_activity(act);
        }
    }

    pub async fn clear(&self) {
        let mut client_lock = self.client.lock().await;
        if let Some(ref mut client) = *client_lock {
            let _ = client.clear_activity();
        }
    }
}

