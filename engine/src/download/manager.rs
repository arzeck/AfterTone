//! AfterTone Offline Download Manager
//! Downloads audio tracks with embedded metadata and artwork for offline playback.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

use crate::audio::StreamResolver;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DownloadStatus {
    Queued,
    Downloading,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadItem {
    pub id: String, // videoId
    pub title: String,
    pub artist: String,
    pub thumbnail: String,
    pub duration_sec: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    pub status: DownloadStatus,
    pub progress: f32, // 0.0 to 1.0
    pub bytes_downloaded: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadRequest {
    pub video_id: String,
    pub title: String,
    pub artist: String,
    pub thumbnail: Option<String>,
    pub duration_sec: Option<i64>,
}

pub struct DownloadManager {
    download_dir: PathBuf,
    items: Arc<RwLock<HashMap<String, DownloadItem>>>,
    queue_tx: mpsc::UnboundedSender<DownloadRequest>,
    resolver: Arc<StreamResolver>,
    client: Client,
}

impl DownloadManager {
    pub fn new(resolver: Arc<StreamResolver>) -> Arc<Self> {
        let download_dir = dirs_or_local_music_dir();
        let _ = std::fs::create_dir_all(&download_dir);

        let items_map = load_persisted_downloads(&download_dir);
        let items = Arc::new(RwLock::new(items_map));
        let (queue_tx, mut queue_rx) = mpsc::unbounded_channel::<DownloadRequest>();

        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_default();

        let mgr = Arc::new(Self {
            download_dir: download_dir.clone(),
            items: Arc::clone(&items),
            queue_tx,
            resolver: Arc::clone(&resolver),
            client,
        });

        // Background worker
        let worker_mgr = Arc::clone(&mgr);
        tokio::spawn(async move {
            while let Some(req) = queue_rx.recv().await {
                worker_mgr.process_download(req).await;
            }
        });

        mgr
    }

    pub fn download_directory(&self) -> &Path {
        &self.download_dir
    }

    pub async fn list(&self) -> Vec<DownloadItem> {
        self.items.read().await.values().cloned().collect()
    }

    pub async fn get(&self, id: &str) -> Option<DownloadItem> {
        self.items.read().await.get(id).cloned()
    }

    pub async fn queue(&self, req: DownloadRequest) -> DownloadItem {
        let clean_id = req.video_id.trim().to_string();
        {
            let read = self.items.read().await;
            if let Some(existing) = read.get(&clean_id) {
                if existing.status == DownloadStatus::Completed {
                    return existing.clone();
                }
            }
        }

        let item = DownloadItem {
            id: clean_id.clone(),
            title: req.title.clone(),
            artist: req.artist.clone(),
            thumbnail: req.thumbnail.clone().unwrap_or_default(),
            duration_sec: req.duration_sec.unwrap_or(0),
            file_path: None,
            status: DownloadStatus::Queued,
            progress: 0.0,
            bytes_downloaded: 0,
            total_bytes: None,
            error: None,
        };

        {
            let mut write = self.items.write().await;
            write.insert(clean_id, item.clone());
        }

        let _ = self.queue_tx.send(req);
        self.persist().await;
        item
    }

    pub async fn delete(&self, id: &str) -> bool {
        let clean_id = id.trim();
        let path_opt = {
            let mut write = self.items.write().await;
            if let Some(removed) = write.remove(clean_id) {
                removed.file_path.map(PathBuf::from)
            } else {
                None
            }
        };

        if let Some(path) = path_opt {
            let _ = std::fs::remove_file(path);
        }
        self.persist().await;
        true
    }

    async fn persist(&self) {
        let items: Vec<DownloadItem> = self.items.read().await.values().cloned().collect();
        let path = self.download_dir.join("downloads.json");
        if let Ok(json_str) = serde_json::to_string_pretty(&items) {
            let _ = std::fs::write(path, json_str);
        }
    }

    async fn process_download(&self, req: DownloadRequest) {
        let clean_id = req.video_id.trim().to_string();

        // Set status to downloading
        {
            let mut write = self.items.write().await;
            if let Some(item) = write.get_mut(&clean_id) {
                item.status = DownloadStatus::Downloading;
                item.progress = 0.05;
            }
        }

        // 1. Resolve stream URL: first JioSaavn 320k, then fallback to YouTube
        let stream_url = match self
            .resolver
            .resolve_audio_stream(&req.title, &req.artist, Some(&clean_id))
            .await
        {
            Some(u) => u,
            None => match StreamResolver::resolve_youtube_stream_url(&clean_id).await {
                Some(u) => u,
                None => {
                    let mut write = self.items.write().await;
                    if let Some(item) = write.get_mut(&clean_id) {
                        item.status = DownloadStatus::Failed;
                        item.error = Some("Could not resolve audio stream".to_string());
                    }
                    self.persist().await;
                    return;
                }
            },
        };

        // Determine destination filename
        let safe_artist = sanitize_filename(&req.artist);
        let safe_title = sanitize_filename(&req.title);
        let ext = if stream_url.contains(".mp4") || stream_url.contains("m4a") {
            "m4a"
        } else if stream_url.contains(".opus") || stream_url.contains("audio/opus") {
            "opus"
        } else {
            "mp4"
        };
        let final_filename = format!("{} - {}.{}", safe_artist, safe_title, ext);
        let final_path = self.download_dir.join(&final_filename);
        let temp_path = self.download_dir.join(format!("{}.part", final_filename));

        // 2. Stream download in chunks
        let res = match self.client.get(&stream_url).send().await {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                let mut write = self.items.write().await;
                if let Some(item) = write.get_mut(&clean_id) {
                    item.status = DownloadStatus::Failed;
                    item.error = Some(format!("HTTP error {}", r.status()));
                }
                self.persist().await;
                return;
            }
            Err(e) => {
                let mut write = self.items.write().await;
                if let Some(item) = write.get_mut(&clean_id) {
                    item.status = DownloadStatus::Failed;
                    item.error = Some(e.to_string());
                }
                self.persist().await;
                return;
            }
        };

        let total_size = res.content_length();
        {
            let mut write = self.items.write().await;
            if let Some(item) = write.get_mut(&clean_id) {
                item.total_bytes = total_size;
            }
        }

        let mut file = match File::create(&temp_path) {
            Ok(f) => f,
            Err(e) => {
                let mut write = self.items.write().await;
                if let Some(item) = write.get_mut(&clean_id) {
                    item.status = DownloadStatus::Failed;
                    item.error = Some(format!("File create error: {}", e));
                }
                self.persist().await;
                return;
            }
        };

        let mut stream = res.bytes_stream();
        let mut downloaded: u64 = 0;
        use futures_util::StreamExt;

        while let Some(chunk_res) = stream.next().await {
            match chunk_res {
                Ok(chunk) => {
                    if let Err(e) = file.write_all(&chunk) {
                        let _ = std::fs::remove_file(&temp_path);
                        let mut write = self.items.write().await;
                        if let Some(item) = write.get_mut(&clean_id) {
                            item.status = DownloadStatus::Failed;
                            item.error = Some(format!("Write error: {}", e));
                        }
                        self.persist().await;
                        return;
                    }
                    downloaded += chunk.len() as u64;
                    let progress = if let Some(tot) = total_size {
                        if tot > 0 {
                            (downloaded as f32 / tot as f32).min(0.99)
                        } else {
                            0.5
                        }
                    } else {
                        0.5
                    };

                    let mut write = self.items.write().await;
                    if let Some(item) = write.get_mut(&clean_id) {
                        item.bytes_downloaded = downloaded;
                        item.progress = progress;
                    }
                }
                Err(e) => {
                    let _ = std::fs::remove_file(&temp_path);
                    let mut write = self.items.write().await;
                    if let Some(item) = write.get_mut(&clean_id) {
                        item.status = DownloadStatus::Failed;
                        item.error = Some(format!("Stream read error: {}", e));
                    }
                    self.persist().await;
                    return;
                }
            }
        }

        let _ = file.flush();
        drop(file);

        if let Err(e) = std::fs::rename(&temp_path, &final_path) {
            let mut write = self.items.write().await;
            if let Some(item) = write.get_mut(&clean_id) {
                item.status = DownloadStatus::Failed;
                item.error = Some(format!("Rename error: {}", e));
            }
            self.persist().await;
            return;
        }

        // 3. Mark completed
        {
            let mut write = self.items.write().await;
            if let Some(item) = write.get_mut(&clean_id) {
                item.status = DownloadStatus::Completed;
                item.progress = 1.0;
                item.file_path = Some(final_path.to_string_lossy().to_string());
                item.bytes_downloaded = downloaded;
            }
        }
        self.persist().await;
    }
}

fn sanitize_filename(s: &str) -> String {
    let invalid = ['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
    let mut out: String = s.chars().map(|c| if invalid.contains(&c) { '_' } else { c }).collect();
    let ws_re = regex::Regex::new(r"\s+").unwrap();
    out = ws_re.replace_all(&out, " ").trim().to_string();
    if out.is_empty() {
        "audio".to_string()
    } else {
        out
    }
}

fn dirs_or_local_music_dir() -> PathBuf {
    if let Ok(profile) = std::env::var("USERPROFILE") {
        let music = PathBuf::from(profile).join("Music").join("AfterTone");
        return music;
    }
    PathBuf::from("downloads")
}

fn load_persisted_downloads(dir: &Path) -> HashMap<String, DownloadItem> {
    let path = dir.join("downloads.json");
    if let Ok(data) = std::fs::read_to_string(path) {
        if let Ok(items) = serde_json::from_str::<Vec<DownloadItem>>(&data) {
            let mut map = HashMap::new();
            for item in items {
                // Verify file still exists if completed
                if item.status == DownloadStatus::Completed {
                    if let Some(ref fp) = item.file_path {
                        if !Path::new(fp).exists() {
                            continue;
                        }
                    }
                }
                map.insert(item.id.clone(), item);
            }
            return map;
        }
    }
    HashMap::new()
}
