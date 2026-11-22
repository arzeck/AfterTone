//! Native YouTube Music InnerTube Client
//! Queries music.youtube.com using WEB_REMIX client context with zero external dependencies.

use regex::Regex;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT_LANGUAGE, CONTENT_TYPE, ORIGIN, REFERER, USER_AGENT};
use reqwest::Client;
use serde_json::{json, Value};
use std::sync::OnceLock;
use std::time::Duration;

use super::models::{BrowseResponse, HomeFeed, HomeShelf, InnertubeTrack, SearchResult};

static DURATION_RE: OnceLock<Regex> = OnceLock::new();
static PACKAGING_RE: OnceLock<Regex> = OnceLock::new();

fn duration_re() -> &'static Regex {
    DURATION_RE.get_or_init(|| Regex::new(r"^\d{1,2}:\d{2}(?::\d{2})?$").unwrap())
}

fn packaging_re() -> &'static Regex {
    PACKAGING_RE.get_or_init(|| {
        Regex::new(r"(?i)\s*[\(\[]\s*(?:official\s+(?:music\s+)?(?:video|audio)|music\s+video|lyric(?:s)?(?:\s+video)?|visualizer|audio\s+track|full\s+song|album\s+version|hd|4k|hq)[\)\]]").unwrap()
    })
}

pub struct InnerTubeClient {
    client: Client,
}

impl Default for InnerTubeClient {
    fn default() -> Self {
        Self::new()
    }
}

impl InnerTubeClient {
    pub fn new() -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36"),
        );
        headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.9"));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(ORIGIN, HeaderValue::from_static("https://music.youtube.com"));
        headers.insert(REFERER, HeaderValue::from_static("https://music.youtube.com/"));

        let client = Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(12))
            .build()
            .unwrap_or_default();

        Self { client }
    }

    fn client_context() -> Value {
        json!({
            "client": {
                "clientName": "WEB_REMIX",
                "clientVersion": "1.20250101.01.00",
                "hl": "en",
                "gl": "US"
            }
        })
    }

    fn parse_duration(s: &str) -> i64 {
        let parts: Vec<&str> = s.split(':').collect();
        match parts.len() {
            2 => {
                let m: i64 = parts[0].parse().unwrap_or(0);
                let sec: i64 = parts[1].parse().unwrap_or(0);
                m * 60 + sec
            }
            3 => {
                let h: i64 = parts[0].parse().unwrap_or(0);
                let m: i64 = parts[1].parse().unwrap_or(0);
                let sec: i64 = parts[2].parse().unwrap_or(0);
                h * 3600 + m * 60 + sec
            }
            _ => 0,
        }
    }

    fn format_duration(seconds: i64) -> String {
        if seconds <= 0 {
            return "0:00".to_string();
        }
        let m = seconds / 60;
        let s = seconds % 60;
        format!("{}:{:02}", m, s)
    }

    fn clean_title(raw: &str) -> String {
        packaging_re().replace_all(raw, "").trim().to_string()
    }

    fn get_high_res_thumb(thumbnails: Option<&Vec<Value>>, fallback_id: &str) -> String {
        if let Some(thumbs) = thumbnails {
            if let Some(last) = thumbs.last() {
                if let Some(mut url) = last.get("url").and_then(|u| u.as_str()).map(|s| s.to_string()) {
                    if url.contains("googleusercontent.com") {
                        let re = Regex::new(r"=w\d+-h\d+").unwrap();
                        url = re.replace(&url, "=w544-h544").to_string();
                    }
                    if url.starts_with("//") {
                        url = format!("https:{}", url);
                    }
                    if !url.is_empty() {
                        return url;
                    }
                }
            }
        }
        if !fallback_id.is_empty() {
            format!("https://i.ytimg.com/vi/{}/hqdefault.jpg", fallback_id)
        } else {
            String::new()
        }
    }

    fn parse_track_item(renderer: &Value) -> Option<InnertubeTrack> {
        let item = renderer.get("musicResponsiveListItemRenderer").unwrap_or(renderer);

        let flex_cols = item.get("flexColumns")?.as_array()?;
        if flex_cols.is_empty() {
            return None;
        }

        // Title
        let col0_runs = flex_cols[0]
            .pointer("/musicResponsiveListItemFlexColumnRenderer/text/runs")?
            .as_array()?;
        let raw_title = col0_runs
            .iter()
            .filter_map(|r| r.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("");
        if raw_title.is_empty() {
            return None;
        }
        let title = Self::clean_title(&raw_title);

        // Video ID
        let video_id = item
            .pointer("/playlistItemData/videoId")
            .and_then(|v| v.as_str())
            .or_else(|| col0_runs[0].pointer("/navigationEndpoint/watchEndpoint/videoId").and_then(|v| v.as_str()))
            .or_else(|| item.pointer("/navigationEndpoint/watchEndpoint/videoId").and_then(|v| v.as_str()))?
            .to_string();

        // Artist & metadata from col 1
        let mut artist = "Unknown Artist".to_string();
        let mut duration_str = String::new();
        let mut views = None;
        let mut album = None;

        if flex_cols.len() > 1 {
            if let Some(col1_runs) = flex_cols[1]
                .pointer("/musicResponsiveListItemFlexColumnRenderer/text/runs")
                .and_then(|r| r.as_array())
            {
                let parts: Vec<String> = col1_runs
                    .iter()
                    .filter_map(|r| r.get("text").and_then(|t| t.as_str()))
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty() && s != "•")
                    .collect();

                if let Some(first) = parts.first() {
                    artist = first.clone();
                }

                for p in parts.iter().skip(1) {
                    if duration_re().is_match(p) {
                        duration_str = p.clone();
                    } else if p.to_lowercase().contains("views") || p.to_lowercase().contains("plays") {
                        views = Some(p.clone());
                    } else if album.is_none() && !p.to_lowercase().contains("song") {
                        album = Some(p.clone());
                    }
                }
            }
        }

        // Fixed columns for duration fallback
        if duration_str.is_empty() {
            if let Some(fixed) = item.pointer("/fixedColumns/0/musicResponsiveListItemFixedColumnRenderer/text/runs/0/text").and_then(|t| t.as_str()) {
                duration_str = fixed.to_string();
            }
        }

        let duration = Self::parse_duration(&duration_str);
        let thumbs = item.pointer("/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails").and_then(|t| t.as_array());
        let thumbnail = Self::get_high_res_thumb(thumbs, &video_id);

        Some(InnertubeTrack {
            id: video_id,
            title,
            artist,
            duration,
            duration_formatted: if !duration_str.is_empty() { duration_str } else { Self::format_duration(duration) },
            thumbnail,
            views,
            album,
        })
    }

    /// Fetches YouTube Music Home Feed (Quick picks, community shelves, trending)
    pub async fn fetch_home(&self) -> Result<HomeFeed, Box<dyn std::error::Error + Send + Sync>> {
        let payload = json!({
            "context": Self::client_context(),
            "browseId": "FEmusic_home"
        });

        let res = self
            .client
            .post("https://music.youtube.com/youtubei/v1/browse?alt=json")
            .json(&payload)
            .send()
            .await?;

        let root: Value = res.json().await?;
        let mut quick_picks = Vec::new();
        let mut shelves = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();

        fn walk(node: &Value, list: &mut Vec<InnertubeTrack>, seen: &mut std::collections::HashSet<String>) {
            if let Some(track) = InnerTubeClient::parse_track_item(node) {
                if !seen.contains(&track.id) {
                    seen.insert(track.id.clone());
                    list.push(track);
                }
                return;
            }
            if let Some(arr) = node.as_array() {
                for item in arr {
                    walk(item, list, seen);
                }
            } else if let Some(obj) = node.as_object() {
                for (_k, val) in obj {
                    walk(val, list, seen);
                }
            }
        }

        // Walk tabs
        if let Some(tabs) = root.pointer("/contents/singleColumnBrowseResultsRenderer/tabs").and_then(|t| t.as_array()) {
            for tab in tabs {
                if let Some(section_list) = tab.pointer("/tabRenderer/content/sectionListRenderer/contents").and_then(|c| c.as_array()) {
                    for section in section_list {
                        let title = section
                            .pointer("/musicCarouselShelfRenderer/header/musicCarouselShelfBasicHeaderRenderer/title/runs/0/text")
                            .or_else(|| section.pointer("/musicDescriptionShelfRenderer/header/runs/0/text"))
                            .and_then(|t| t.as_str())
                            .unwrap_or("")
                            .to_string();

                        let mut shelf_tracks = Vec::new();
                        walk(section, &mut shelf_tracks, &mut seen_ids);

                        if !shelf_tracks.is_empty() {
                            if quick_picks.is_empty() && (title.to_lowercase().contains("quick pick") || title.to_lowercase().contains("listen again") || title.is_empty()) {
                                quick_picks = shelf_tracks.clone();
                            }
                            shelves.push(HomeShelf {
                                title: if !title.is_empty() { title } else { "Recommended".to_string() },
                                items: shelf_tracks,
                            });
                        }
                    }
                }
            }
        }

        let recents = quick_picks.iter().take(12).cloned().collect();

        Ok(HomeFeed {
            recents,
            quick_picks,
            albums_and_singles: Vec::new(),
            shelves,
            is_personalized: false,
        })
    }

    /// Searches YouTube Music catalogue for songs and tracks
    pub async fn search(&self, query: &str) -> Result<SearchResult, Box<dyn std::error::Error + Send + Sync>> {
        let clean_q = query.trim();
        if clean_q.is_empty() {
            return Ok(SearchResult { tracks: Vec::new() });
        }

        let payload = json!({
            "context": Self::client_context(),
            "query": clean_q
        });

        let res = self
            .client
            .post("https://music.youtube.com/youtubei/v1/search?alt=json")
            .json(&payload)
            .send()
            .await?;

        let root: Value = res.json().await?;
        let mut tracks = Vec::new();
        let mut seen = std::collections::HashSet::new();

        fn walk_search(node: &Value, tracks: &mut Vec<InnertubeTrack>, seen: &mut std::collections::HashSet<String>) {
            if let Some(track) = InnerTubeClient::parse_track_item(node) {
                if !seen.contains(&track.id) {
                    seen.insert(track.id.clone());
                    tracks.push(track);
                }
                return;
            }
            if let Some(arr) = node.as_array() {
                for item in arr {
                    walk_search(item, tracks, seen);
                }
            } else if let Some(obj) = node.as_object() {
                for (_k, val) in obj {
                    walk_search(val, tracks, seen);
                }
            }
        }

        walk_search(&root, &mut tracks, &mut seen);

        Ok(SearchResult { tracks })
    }

    /// Fetches YouTube Music Radio recommendations based on seed track ID
    pub async fn fetch_radio(&self, video_id: &str) -> Result<Vec<InnertubeTrack>, Box<dyn std::error::Error + Send + Sync>> {
        let payload = json!({
            "context": Self::client_context(),
            "videoId": video_id
        });

        let res = self
            .client
            .post("https://music.youtube.com/youtubei/v1/next?alt=json")
            .json(&payload)
            .send()
            .await?;

        let root: Value = res.json().await?;
        let mut tracks = Vec::new();
        let mut seen = std::collections::HashSet::new();
        seen.insert(video_id.to_string());

        fn walk_next(node: &Value, tracks: &mut Vec<InnertubeTrack>, seen: &mut std::collections::HashSet<String>) {
            if let Some(track) = InnerTubeClient::parse_track_item(node) {
                if !seen.contains(&track.id) {
                    seen.insert(track.id.clone());
                    tracks.push(track);
                }
                return;
            }
            if let Some(arr) = node.as_array() {
                for item in arr {
                    walk_next(item, tracks, seen);
                }
            } else if let Some(obj) = node.as_object() {
                for (_k, val) in obj {
                    walk_next(val, tracks, seen);
                }
            }
        }

        walk_next(&root, &mut tracks, &mut seen);

        Ok(tracks)
    }

    /// Fetches YouTube Music search suggestions for a query
    pub async fn fetch_suggestions(&self, query: &str) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
        let clean = query.trim();
        if clean.is_empty() {
            return Ok(Vec::new());
        }

        let payload = json!({
            "context": Self::client_context(),
            "input": clean
        });

        let res = self
            .client
            .post("https://music.youtube.com/youtubei/v1/music/get_search_suggestions?alt=json")
            .json(&payload)
            .send()
            .await?;

        let root: Value = res.json().await?;
        let mut suggestions = Vec::new();

        if let Some(contents) = root.pointer("/contents/0/searchSuggestionsSectionRenderer/contents").and_then(|c| c.as_array()) {
            for item in contents {
                if let Some(runs) = item.pointer("/searchSuggestionRenderer/suggestion/runs").and_then(|r| r.as_array()) {
                    let text = runs
                        .iter()
                        .filter_map(|r| r.get("text").and_then(|t| t.as_str()))
                        .collect::<Vec<_>>()
                        .join("");
                    let cleaned = text.trim();
                    if !cleaned.is_empty() && !suggestions.contains(&cleaned.to_string()) {
                        suggestions.push(cleaned.to_string());
                    }
                }
            }
        }

        Ok(suggestions)
    }

    /// Fetches full Album, Artist, Playlist, or Category data using InnerTube Browse endpoint
    pub async fn fetch_browse(
        &self,
        browse_id: &str,
        params: Option<&str>,
    ) -> Result<BrowseResponse, Box<dyn std::error::Error + Send + Sync>> {
        let clean_id = browse_id.trim();
        let mut payload_map = serde_json::Map::new();
        payload_map.insert("context".to_string(), Self::client_context());
        payload_map.insert("browseId".to_string(), json!(clean_id));
        if let Some(p) = params {
            if !p.trim().is_empty() {
                payload_map.insert("params".to_string(), json!(p.trim()));
            }
        }
        let payload = Value::Object(payload_map);

        let res = self
            .client
            .post("https://music.youtube.com/youtubei/v1/browse?alt=json")
            .json(&payload)
            .send()
            .await?;

        let root: Value = res.json().await?;

        // Determine page type
        let page_type = if clean_id.starts_with("MPREb_") {
            "album".to_string()
        } else if clean_id.starts_with("UC") || clean_id.contains("artist") {
            "artist".to_string()
        } else if clean_id.starts_with("VL") || clean_id.starts_with("RDCLAK") {
            "playlist".to_string()
        } else {
            "generic".to_string()
        };

        // Extract header
        let two_col_hdr = root.pointer("/contents/twoColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents/0/musicResponsiveHeaderRenderer");
        let header_node = two_col_hdr
            .or_else(|| root.pointer("/header/musicImmersiveHeaderRenderer"))
            .or_else(|| root.pointer("/header/musicVisualHeaderRenderer"))
            .or_else(|| root.pointer("/header/musicDetailHeaderRenderer"))
            .or_else(|| root.pointer("/header/musicResponsiveHeaderRenderer"));

        let mut title = String::new();
        let mut subtitle = None;
        let mut second_subtitle = None;
        let mut description = None;
        let mut thumbnail = None;

        if let Some(hdr) = header_node {
            if let Some(runs) = hdr.pointer("/title/runs").and_then(|r| r.as_array()) {
                title = runs.iter().filter_map(|r| r.get("text").and_then(|t| t.as_str())).collect::<Vec<_>>().join("");
            }
            if let Some(runs) = hdr.pointer("/subtitle/runs").and_then(|r| r.as_array()) {
                let sub = runs.iter().filter_map(|r| r.get("text").and_then(|t| t.as_str())).collect::<Vec<_>>().join(" ");
                if !sub.trim().is_empty() {
                    subtitle = Some(sub.trim().to_string());
                }
            }
            if let Some(runs) = hdr.pointer("/secondSubtitle/runs").and_then(|r| r.as_array()) {
                let sub2 = runs.iter().filter_map(|r| r.get("text").and_then(|t| t.as_str())).collect::<Vec<_>>().join(" ");
                if !sub2.trim().is_empty() {
                    second_subtitle = Some(sub2.trim().to_string());
                }
            }
            if let Some(runs) = hdr.pointer("/description/musicDescriptionShelfRenderer/description/runs").and_then(|r| r.as_array()) {
                let desc = runs.iter().filter_map(|r| r.get("text").and_then(|t| t.as_str())).collect::<Vec<_>>().join("");
                if !desc.trim().is_empty() {
                    description = Some(desc);
                }
            }
            let thumbs = hdr.pointer("/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails")
                .or_else(|| hdr.pointer("/thumbnail/croppedSquareThumbnailRenderer/thumbnail/thumbnails"))
                .and_then(|t| t.as_array());
            if let Some(th) = thumbs {
                let u = Self::get_high_res_thumb(Some(th), "");
                if !u.is_empty() {
                    thumbnail = Some(u);
                }
            }
        }

        // If title is empty, check microformat
        if title.is_empty() {
            if let Some(mf_title) = root.pointer("/microformat/microformatDataRenderer/title").and_then(|t| t.as_str()) {
                title = mf_title.to_string();
            }
        }
        if title.is_empty() {
            title = clean_id.to_string();
        }

        let mut tracks = Vec::new();
        let mut shelves = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();

        fn walk_browse(node: &Value, tracks: &mut Vec<InnertubeTrack>, seen: &mut std::collections::HashSet<String>) {
            if let Some(track) = InnerTubeClient::parse_track_item(node) {
                if !seen.contains(&track.id) {
                    seen.insert(track.id.clone());
                    tracks.push(track);
                }
                return;
            }
            if let Some(arr) = node.as_array() {
                for item in arr {
                    walk_browse(item, tracks, seen);
                }
            } else if let Some(obj) = node.as_object() {
                for (_k, val) in obj {
                    walk_browse(val, tracks, seen);
                }
            }
        }

        // Walk 2-column secondaryContents (Album & Playlist tracklist)
        if let Some(sec) = root.pointer("/contents/twoColumnBrowseResultsRenderer/secondaryContents/sectionListRenderer/contents").and_then(|c| c.as_array()) {
            for item in sec {
                walk_browse(item, &mut tracks, &mut seen_ids);
            }
        }

        // Walk 1-column tabs (Artist sections or fallback)
        if let Some(tabs) = root.pointer("/contents/singleColumnBrowseResultsRenderer/tabs").and_then(|t| t.as_array()) {
            for tab in tabs {
                if let Some(section_list) = tab.pointer("/tabRenderer/content/sectionListRenderer/contents").and_then(|c| c.as_array()) {
                    for section in section_list {
                        let shelf_title = section
                            .pointer("/musicCarouselShelfRenderer/header/musicCarouselShelfBasicHeaderRenderer/title/runs/0/text")
                            .or_else(|| section.pointer("/musicShelfRenderer/title/runs/0/text"))
                            .and_then(|t| t.as_str())
                            .unwrap_or("")
                            .to_string();

                        let mut shelf_tracks = Vec::new();
                        walk_browse(section, &mut shelf_tracks, &mut seen_ids);

                        if !shelf_tracks.is_empty() {
                            if tracks.is_empty() && (shelf_title.to_lowercase().contains("top songs") || shelf_title.to_lowercase().contains("songs")) {
                                tracks = shelf_tracks.clone();
                            }
                            shelves.push(HomeShelf {
                                title: if !shelf_title.is_empty() { shelf_title } else { "Tracks".to_string() },
                                items: shelf_tracks,
                            });
                        }
                    }
                }
            }
        }

        Ok(BrowseResponse {
            page_type,
            id: clean_id.to_string(),
            title,
            subtitle,
            second_subtitle,
            description,
            thumbnail,
            tracks,
            shelves,
        })
    }
}


