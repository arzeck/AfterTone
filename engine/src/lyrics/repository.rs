//! Concurrent Lyrics Racing Repository

use reqwest::Client;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::sources::{
    clean_search_artist, clean_search_title, fetch_better_lyrics, fetch_bini_lyrics, fetch_kugou_lyrics, fetch_lrc_lib,
    fetch_lrc_red, fetch_simp_music_lyrics, LyricResult,
};

const PRIORITY_ORDER: &[&str] = &["lrc.red", "BiniLyrics", "BetterLyrics", "SimpMusic", "KuGou", "LRCLIB"];

fn priority_index(source: &str) -> usize {
    PRIORITY_ORDER
        .iter()
        .position(|&s| s == source)
        .unwrap_or(usize::MAX)
}

#[derive(Debug, Clone)]
pub struct CachedLyrics {
    pub active_result: LyricResult,
    pub all_results: HashMap<String, LyricResult>,
}

pub struct LyricsRepository {
    client: Client,
    cache: Arc<RwLock<HashMap<String, CachedLyrics>>>,
}

impl Default for LyricsRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl LyricsRepository {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn get_track_key(
        title: &str,
        artist: &str,
        duration_ms: Option<i64>,
        video_id: Option<&str>,
    ) -> String {
        let sec = duration_ms.filter(|&d| d > 0).map(|d| (d as f64 / 1000.0).round() as i64).unwrap_or(0);
        format!(
            "{}|{}|{}|{}",
            title.trim().to_lowercase(),
            artist.trim().to_lowercase(),
            sec,
            video_id.unwrap_or("").trim()
        )
    }

    pub async fn clear_cache(&self) {
        self.cache.write().await.clear();
    }

    /// Fetches lyrics by concurrently racing across providers.
    pub async fn fetch_lyrics(
        &self,
        title: &str,
        artist: &str,
        duration_ms: Option<i64>,
        video_id: Option<&str>,
        isrc: Option<&str>,
    ) -> (Option<LyricResult>, HashMap<String, LyricResult>) {
        let key = Self::get_track_key(title, artist, duration_ms, video_id);

        // Check cache
        {
            let cache_read = self.cache.read().await;
            if let Some(cached) = cache_read.get(&key) {
                return (Some(cached.active_result.clone()), cached.all_results.clone());
            }
        }

        let clean_title = clean_search_title(title);
        let clean_artist = clean_search_artist(artist);

        // Spawn concurrent racing tasks
        let c = &self.client;

        let simp_task = {
            let vid = video_id.map(|v| v.to_string());
            let cl = c.clone();
            tokio::spawn(async move {
                if let Some(v) = vid {
                    fetch_simp_music_lyrics(&cl, &v).await
                } else {
                    None
                }
            })
        };

        let lrc_red_task = {
            let t = clean_title.clone();
            let a = clean_artist.clone();
            let isrc_owned = isrc.map(|s| s.to_string());
            let cl = c.clone();
            tokio::spawn(async move {
                fetch_lrc_red(&cl, &t, &a, duration_ms, isrc_owned.as_deref()).await
            })
        };

        let bini_task = {
            let t = clean_title.clone();
            let a = clean_artist.clone();
            let isrc_owned = isrc.map(|s| s.to_string());
            let cl = c.clone();
            tokio::spawn(async move {
                fetch_bini_lyrics(&cl, &t, &a, duration_ms, isrc_owned.as_deref()).await
            })
        };

        let better_task = {
            let t = clean_title.clone();
            let a = clean_artist.clone();
            let cl = c.clone();
            tokio::spawn(async move {
                fetch_better_lyrics(&cl, &t, &a, duration_ms).await
            })
        };

        let kugou_task = {
            let t = clean_title.clone();
            let a = clean_artist.clone();
            let cl = c.clone();
            tokio::spawn(async move {
                fetch_kugou_lyrics(&cl, &t, &a, duration_ms).await
            })
        };

        let lrclib_task = {
            let t = clean_title;
            let a = clean_artist;
            let cl = c.clone();
            tokio::spawn(async move {
                fetch_lrc_lib(&cl, &t, &a, duration_ms).await
            })
        };

        let mut all_results = HashMap::new();
        let mut best_result: Option<LyricResult> = None;

        let record_result = |res: Option<LyricResult>,
                             all: &mut HashMap<String, LyricResult>,
                             best: &mut Option<LyricResult>| {
            if let Some(r) = res {
                if !r.lines.is_empty() {
                    let should_promote = match best {
                        None => true,
                        Some(ref cur) => {
                            if r.is_word_synced && !cur.is_word_synced {
                                true
                            } else if r.is_word_synced == cur.is_word_synced {
                                priority_index(&r.source) < priority_index(&cur.source)
                            } else {
                                false
                            }
                        }
                    };

                    all.insert(r.source.clone(), r.clone());
                    if should_promote {
                        *best = Some(r);
                    }
                }
            }
        };

        if let Ok(res) = simp_task.await {
            record_result(res, &mut all_results, &mut best_result);
        }
        if let Ok(res) = lrc_red_task.await {
            record_result(res, &mut all_results, &mut best_result);
        }
        if let Ok(res) = bini_task.await {
            record_result(res, &mut all_results, &mut best_result);
        }
        if let Ok(res) = better_task.await {
            record_result(res, &mut all_results, &mut best_result);
        }
        if let Ok(res) = lrclib_task.await {
            record_result(res, &mut all_results, &mut best_result);
        }
        if let Ok(res) = kugou_task.await {
            record_result(res, &mut all_results, &mut best_result);
        }

        if let Some(ref best) = best_result {
            self.cache.write().await.insert(
                key,
                CachedLyrics {
                    active_result: best.clone(),
                    all_results: all_results.clone(),
                },
            );
        }

        (best_result, all_results)
    }
}
