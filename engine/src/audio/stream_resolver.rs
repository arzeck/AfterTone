//! AfterTone Stream Resolver
//! Resolves high-fidelity (320kbps) audio streams from CDN and YouTube.

use regex::Regex;
use reqwest::Client;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::process::Command;
use tokio::sync::RwLock;

use super::des_cipher::decrypt_saavn_media_url;

struct CacheEntry {
    url: String,
    expires_at: Instant,
}

const CACHE_TTL: Duration = Duration::from_secs(30 * 60); // 30 minutes

pub struct StreamResolver {
    client: Client,
    cache: Arc<RwLock<HashMap<String, CacheEntry>>>,
}

impl Default for StreamResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamResolver {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(8))
                .build()
                .unwrap_or_default(),
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Resolves YouTube audio stream URL using yt-dlp with player client extraction
    pub async fn resolve_youtube_stream_url(video_id: &str) -> Option<String> {
        let clean_id = video_id.trim();
        if clean_id.is_empty() {
            return None;
        }

        let yt_url = format!("https://www.youtube.com/watch?v={}", clean_id);
        let output = Command::new("yt-dlp")
            .args([
                "--force-ipv4",
                "-g",
                "-f",
                "140/251/ba/bestaudio",
                "--extractor-args",
                "youtube:player_client=visionos,android_creator,web_creator",
                &yt_url,
            ])
            .output()
            .await
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = stdout.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();

        let stream_url = lines
            .iter()
            .find(|l| l.contains("videoplayback") && !l.contains(".m3u8"))
            .or_else(|| lines.iter().find(|l| !l.contains(".m3u8")))
            .or_else(|| lines.first())?;

        if stream_url.starts_with("http") {
            Some(stream_url.to_string())
        } else {
            None
        }
    }

    /// Resolves high-bitrate audio stream for a track from JioSaavn CDN
    pub async fn resolve_jiosaavn_stream_url(
        &self,
        title: &str,
        artist: &str,
        video_id: Option<&str>,
    ) -> Option<String> {
        self.resolve_audio_stream(title, artist, video_id).await
    }

    /// Resolves both JioSaavn and YouTube streams concurrently and returns whatever finishes first.
    pub async fn resolve_fastest_stream(
        &self,
        title: &str,
        artist: &str,
        video_id: Option<&str>,
    ) -> Option<(String, &'static str)> {
        let has_saavn_target = !title.trim().is_empty() || video_id.is_some();
        let yt_vid = match video_id {
            Some(v) if v.len() == 11 && v.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') => Some(v.to_string()),
            _ => None,
        };

        match (has_saavn_target, yt_vid) {
            (true, Some(vid)) => {
                let saavn_fut = self.resolve_audio_stream(title, artist, Some(&vid));
                let yt_fut = Self::resolve_youtube_stream_url(&vid);

                tokio::pin!(saavn_fut);
                tokio::pin!(yt_fut);

                tokio::select! {
                    saavn_res = &mut saavn_fut => {
                        if let Some(url) = saavn_res {
                            Some((url, "jiosaavn"))
                        } else {
                            yt_fut.await.map(|u| (u, "youtube"))
                        }
                    }
                    yt_res = &mut yt_fut => {
                        if let Some(url) = yt_res {
                            Some((url, "youtube"))
                        } else {
                            saavn_fut.await.map(|u| (u, "jiosaavn"))
                        }
                    }
                }
            }
            (true, None) => {
                self.resolve_audio_stream(title, artist, video_id).await.map(|u| (u, "jiosaavn"))
            }
            (false, Some(vid)) => {
                Self::resolve_youtube_stream_url(&vid).await.map(|u| (u, "youtube"))
            }
            (false, None) => None,
        }
    }

    /// Resolves high-bitrate audio stream for a track from JioSaavn CDN
    pub async fn resolve_audio_stream(
        &self,
        title: &str,
        artist: &str,
        video_id: Option<&str>,
    ) -> Option<String> {
        let cache_key = match video_id {
            Some(vid) if !vid.trim().is_empty() => vid.trim().to_string(),
            _ => format!("{}:::{}", title.trim(), artist.trim()),
        };

        let now = Instant::now();
        {
            let cache_read = self.cache.read().await;
            if let Some(entry) = cache_read.get(&cache_key) {
                if entry.expires_at > now {
                    return Some(entry.url.clone());
                }
            }
        }

        let paren_re = Regex::new(r"\([^)]*\)|\[[^\]]*\]").unwrap();
        let clean_title = paren_re.replace_all(title, "").replace(['®', '™'], "").trim().to_string();
        let clean_artist = artist.replace(['®', '™'], "").trim().to_string();

        let artist_split_re = Regex::new(r"(?i)[,&/]|(?:\b(?:feat|ft|vs|x)\b)").unwrap();
        let primary_artist = artist_split_re
            .split(&clean_artist)
            .next()
            .unwrap_or("")
            .trim()
            .to_string();

        let mut candidates = Vec::new();
        if !clean_title.is_empty() && !primary_artist.is_empty() {
            candidates.push(format!("{} {}", clean_title, primary_artist));
        }
        if !clean_title.is_empty() && !clean_artist.is_empty() && clean_artist != primary_artist {
            candidates.push(format!("{} {}", clean_title, clean_artist));
        }
        if !clean_title.is_empty() {
            candidates.push(clean_title);
        }

        for query in candidates {
            let encoded_q: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
            let endpoint = format!(
                "https://www.jiosaavn.com/api.php?__call=search.getResults&_format=json&api_version=4&ctx=android&q={}&n=5",
                encoded_q
            );

            let res = match self
                .client
                .get(&endpoint)
                .header(
                    "User-Agent",
                    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/134.0.0.0 Safari/537.36",
                )
                .send()
                .await
            {
                Ok(r) if r.status().is_success() => r,
                _ => continue,
            };

            let json_val: serde_json::Value = match res.json().await {
                Ok(v) => v,
                _ => continue,
            };

            let results = match json_val.get("results").and_then(|r| r.as_array()) {
                Some(arr) if !arr.is_empty() => arr,
                _ => continue,
            };

            let song = &results[0];
            let more_info = match song.get("more_info") {
                Some(m) => m,
                None => continue,
            };

            let enc_url = match more_info.get("encrypted_media_url").and_then(|u| u.as_str()) {
                Some(u) if !u.is_empty() => u,
                _ => continue,
            };

            let decrypted = decrypt_saavn_media_url(enc_url);
            if decrypted.is_empty() {
                continue;
            }

            let supports_320 = more_info
                .get("320kbps")
                .map(|v| v == "true" || v == true)
                .unwrap_or(false);

            let rendition_re = Regex::new(r"(?i)_(?:48|96|160|320)\.(mp4|aac|mp3)($|[?#])").unwrap();
            let final_url = if supports_320 {
                rendition_re.replace(&decrypted, "_320.$1$2").to_string()
            } else {
                rendition_re.replace(&decrypted, "_160.$1$2").to_string()
            };

            self.cache.write().await.insert(
                cache_key,
                CacheEntry {
                    url: final_url.clone(),
                    expires_at: Instant::now() + CACHE_TTL,
                },
            );

            return Some(final_url);
        }

        None
    }
}
