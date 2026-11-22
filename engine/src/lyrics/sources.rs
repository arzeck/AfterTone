//! Multi-Source Lyric Providers & Query Sanitization

use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use std::time::Duration;

use super::lrc_parser::parse_any_lrc;
use super::models::{
    with_background_vocals, with_instrumental_gaps, LyricLine, LyricWord,
};
use super::ttml_parser::parse_ttml_lyrics;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricResult {
    pub source: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub isrc: Option<String>,
    pub is_word_synced: bool,
    pub lines: Vec<LyricLine>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_ttml: Option<String>,
}

// ─── Query Sanitization ──────────────────────────────────────────────────────

static CREDITS_RES: OnceLock<Vec<Regex>> = OnceLock::new();
static PACKAGING_RES: OnceLock<Vec<Regex>> = OnceLock::new();

fn credits_regexes() -> &'static [Regex] {
    CREDITS_RES.get_or_init(|| {
        vec![
            Regex::new(r"(?i)\s*\((?:feat\.?|ft\.?|featuring|with)\s+[^)]+\)").unwrap(),
            Regex::new(r"(?i)\s*\[(?:feat\.?|ft\.?|featuring|with)\s+[^\]]+\]").unwrap(),
        ]
    })
}

fn packaging_regexes() -> &'static [Regex] {
    PACKAGING_RES.get_or_init(|| {
        vec![
            Regex::new(r"(?i)\s*[\(\[]\s*(?:official\s+(?:music\s+)?(?:video|audio)|music\s+video|lyric(?:s)?(?:\s+video)?|visualizer|audio\s+track|full\s+song|album\s+version|hd|4k|hq|exclusive|stream|clip\s+officiel)[^\)\]]*[\)\]]").unwrap(),
            Regex::new(r"(?i)\s*\|\s*(?:official\s+(?:music\s+)?video|visualizer|lyric\s+video|audio).*$").unwrap(),
            Regex::new(r"(?i)\s*-\s*(?:official\s+(?:music\s+)?video|visualizer|lyric\s+video|audio).*$").unwrap(),
        ]
    })
}

pub fn clean_search_title(raw_title: &str) -> String {
    if raw_title.is_empty() {
        return String::new();
    }
    let mut name = raw_title
        .replace(['♪', '♫', '★', '☆', '【', '】', '《', '》', '「', '」', '~', '_'], " ");
    for re in credits_regexes() {
        name = re.replace_all(&name, " ").to_string();
    }
    for re in packaging_regexes() {
        name = re.replace_all(&name, " ").to_string();
    }
    let ws_re = Regex::new(r"\s+").unwrap();
    let cleaned = ws_re.replace_all(&name, " ");
    let trimmed = cleaned.trim().trim_end_matches([',', '–', '—', '-']).trim();
    if !trimmed.is_empty() {
        trimmed.to_string()
    } else {
        raw_title.trim().to_string()
    }
}

pub fn clean_search_artist(raw_artist: &str) -> String {
    if raw_artist.is_empty() {
        return String::new();
    }
    let topic_re = Regex::new(r"(?i)\s*-\s*Topic\s*$").unwrap();
    let stripped = topic_re.replace(raw_artist, "");
    let primary = stripped.split('•').next().unwrap_or("").split('·').next().unwrap_or("");
    let trimmed = primary.trim();
    if !trimmed.is_empty() {
        trimmed.to_string()
    } else {
        raw_artist.trim().to_string()
    }
}

fn url_encode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

// ─── 1. lrc.red ──────────────────────────────────────────────────────────────

const LRC_RED: &str = "https://lrc.red/";

static ISRC_RE: OnceLock<Regex> = OnceLock::new();

fn is_valid_isrc(s: &str) -> bool {
    ISRC_RE
        .get_or_init(|| Regex::new(r"^[A-Z]{2}[A-Z0-9]{3}\d{7}$").unwrap())
        .is_match(s.trim())
}

#[derive(Deserialize)]
struct LrcRedSearchHit {
    isrc: Option<String>,
}

#[derive(Deserialize)]
struct LrcRedSearchResponse {
    hits: Option<Vec<LrcRedSearchHit>>,
}

pub async fn fetch_lrc_red(
    client: &Client,
    title: &str,
    artist: &str,
    _duration_ms: Option<i64>,
    isrc: Option<&str>,
) -> Option<LyricResult> {
    // 1. Direct ISRC hit
    if let Some(code) = isrc {
        let upper = code.trim().to_uppercase();
        if is_valid_isrc(&upper) {
            let isrc_url = format!("{}s/{}.ttml", LRC_RED, upper);
            if let Ok(res) = client.get(&isrc_url).timeout(Duration::from_secs(8)).send().await {
                if res.status().is_success() {
                    if let Ok(text) = res.text().await {
                        let lines = parse_ttml_lyrics(&text);
                        if !lines.is_empty() {
                            let is_word_synced = lines.iter().any(|l| !l.words.is_empty());
                            return Some(LyricResult {
                                source: "lrc.red".to_string(),
                                label: "lrc.red (Apple Music)".to_string(),
                                isrc: Some(upper),
                                is_word_synced,
                                lines,
                                raw_ttml: Some(text),
                            });
                        }
                    }
                }
            }
        }
    }

    // 2. Search query
    let query = format!("{} {}", title, artist).trim().to_string();
    let search_url = format!("{}search.json?q={}", LRC_RED, url_encode(&query));
    let s_res = client
        .get(&search_url)
        .timeout(Duration::from_secs(8))
        .send()
        .await
        .ok()?;

    if !s_res.status().is_success() {
        return None;
    }

    let search_data: LrcRedSearchResponse = s_res.json().await.ok()?;
    let hits = search_data.hits?;
    let hit_isrc = hits.first()?.isrc.as_ref()?;

    let ttml_url = format!("{}s/{}.ttml", LRC_RED, hit_isrc);
    let ttml_res = client
        .get(&ttml_url)
        .timeout(Duration::from_secs(8))
        .send()
        .await
        .ok()?;

    if !ttml_res.status().is_success() {
        return None;
    }

    let ttml = ttml_res.text().await.ok()?;
    let lines = parse_ttml_lyrics(&ttml);
    if lines.is_empty() {
        return None;
    }

    let is_word_synced = lines.iter().any(|l| !l.words.is_empty());
    Some(LyricResult {
        source: "lrc.red".to_string(),
        label: "lrc.red (Apple Music)".to_string(),
        isrc: Some(hit_isrc.clone()),
        is_word_synced,
        lines,
        raw_ttml: Some(ttml),
    })
}

// ─── 2. BiniLyrics ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct BiniResult {
    lyrics_url: Option<String>,
    isrc: Option<String>,
}

#[derive(Deserialize)]
struct BiniResponse {
    results: Option<Vec<BiniResult>>,
}

pub async fn fetch_bini_lyrics(
    client: &Client,
    title: &str,
    artist: &str,
    duration_ms: Option<i64>,
    isrc: Option<&str>,
) -> Option<LyricResult> {
    let mut url = url::Url::parse("https://lyrics-api.binimum.org/").ok()?;
    if let Some(code) = isrc {
        url.query_pairs_mut().append_pair("isrc", code);
    } else {
        url.query_pairs_mut().append_pair("track", title);
        url.query_pairs_mut().append_pair("artist", artist);
        if let Some(d) = duration_ms {
            if d > 0 {
                url.query_pairs_mut()
                    .append_pair("duration", &format!("{}", (d as f64 / 1000.0).round() as i64));
            }
        }
    }

    let res = client
        .get(url)
        .timeout(Duration::from_secs(8))
        .send()
        .await
        .ok()?;

    if !res.status().is_success() {
        return None;
    }

    let data: BiniResponse = res.json().await.ok()?;
    let hit = data.results?.into_iter().next()?;
    let lyrics_url = hit.lyrics_url?;

    let ttml_res = client
        .get(&lyrics_url)
        .timeout(Duration::from_secs(8))
        .send()
        .await
        .ok()?;

    if !ttml_res.status().is_success() {
        return None;
    }

    let ttml = ttml_res.text().await.ok()?;
    let lines = parse_ttml_lyrics(&ttml);
    if lines.is_empty() {
        return None;
    }

    let is_word_synced = lines.iter().any(|l| !l.words.is_empty());
    Some(LyricResult {
        source: "BiniLyrics".to_string(),
        label: "BiniLyrics (Apple Music)".to_string(),
        isrc: hit.isrc,
        is_word_synced,
        lines,
        raw_ttml: Some(ttml),
    })
}

// ─── 3. BetterLyrics ─────────────────────────────────────────────────────────

pub async fn fetch_better_lyrics(
    client: &Client,
    title: &str,
    artist: &str,
    duration_ms: Option<i64>,
) -> Option<LyricResult> {
    let mut url = url::Url::parse("https://lyrics-api.boidu.dev/getLyrics").ok()?;
    url.query_pairs_mut().append_pair("s", title);
    url.query_pairs_mut().append_pair("a", artist);
    if let Some(d) = duration_ms {
        if d > 0 {
            url.query_pairs_mut()
                .append_pair("d", &format!("{}", (d as f64 / 1000.0).round() as i64));
        }
    }

    let res = client
        .get(url)
        .timeout(Duration::from_secs(8))
        .send()
        .await
        .ok()?;

    if !res.status().is_success() {
        return None;
    }

    let text = res.text().await.ok()?.trim().to_string();
    if text.is_empty() {
        return None;
    }

    let mut content = text.clone();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
        if let Some(s) = v.get("ttml").or_else(|| v.get("lyrics")).or_else(|| v.get("content")).and_then(|x| x.as_str()) {
            content = s.to_string();
        }
    }

    let lines = if content.contains("<tt") || content.contains("http://www.w3.org/ns/ttml") {
        parse_ttml_lyrics(&content)
    } else {
        parse_any_lrc(&content)
    };

    if lines.is_empty() {
        return None;
    }

    let is_word_synced = lines.iter().any(|l| !l.words.is_empty());
    Some(LyricResult {
        source: "BetterLyrics".to_string(),
        label: "BetterLyrics (Apple Music)".to_string(),
        isrc: None,
        is_word_synced,
        lines,
        raw_ttml: if content.contains("<tt") { Some(content) } else { None },
    })
}

// ─── 4. SimpMusic (YouTube videoId) ──────────────────────────────────────────

#[derive(Deserialize)]
struct SimpSyllable {
    #[serde(alias = "startTimeMs", alias = "start")]
    start_time_ms: Option<i64>,
    #[serde(alias = "endTimeMs", alias = "end")]
    end_time_ms: Option<i64>,
    text: Option<String>,
}

#[derive(Deserialize)]
struct SimpItem {
    #[serde(alias = "startTimeMs", alias = "start", alias = "time")]
    start_time_ms: Option<i64>,
    #[serde(alias = "endTimeMs", alias = "end")]
    end_time_ms: Option<i64>,
    #[serde(alias = "text", alias = "words", alias = "line")]
    text: Option<String>,
    syllables: Option<Vec<SimpSyllable>>,
}

pub async fn fetch_simp_music_lyrics(client: &Client, video_id: &str) -> Option<LyricResult> {
    if video_id.is_empty() {
        return None;
    }

    let url = format!("https://api-lyrics.simpmusic.org/v1/{}", url_encode(video_id));
    let res = client
        .get(&url)
        .timeout(Duration::from_secs(8))
        .send()
        .await
        .ok()?;

    if !res.status().is_success() {
        return None;
    }

    let val: serde_json::Value = res.json().await.ok()?;
    let items: Vec<SimpItem> = if val.is_array() {
        serde_json::from_value(val).ok()?
    } else if let Some(arr) = val.get("lyrics").or_else(|| val.get("result")) {
        serde_json::from_value(arr.clone()).ok()?
    } else {
        return None;
    };

    if items.is_empty() {
        return None;
    }

    let mut raw_lines = Vec::new();
    for it in items {
        let time_ms = match it.start_time_ms {
            Some(t) => t,
            None => continue,
        };
        let txt = it.text.unwrap_or_default().trim().to_string();
        if txt.is_empty() {
            continue;
        }

        let mut words = Vec::new();
        if let Some(syllables) = it.syllables {
            for s in syllables {
                if let (Some(st), Some(et), Some(stxt)) = (s.start_time_ms, s.end_time_ms, s.text) {
                    words.push(LyricWord::new(st, et, stxt));
                }
            }
        }

        let mut line = LyricLine::new(time_ms, txt);
        line.words = words;
        line.sung_until_ms = it.end_time_ms;
        raw_lines.push(line);
    }

    if raw_lines.is_empty() {
        return None;
    }

    raw_lines.sort_by_key(|l| l.time_ms);
    let lines = with_instrumental_gaps(with_background_vocals(raw_lines));
    let is_word_synced = lines.iter().any(|l| !l.words.is_empty());

    Some(LyricResult {
        source: "SimpMusic".to_string(),
        label: "SimpMusic (YouTube Sync)".to_string(),
        isrc: None,
        is_word_synced,
        lines,
        raw_ttml: None,
    })
}

// ─── 5. LRCLIB ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct LrcLibHit {
    #[serde(alias = "syncedLyrics")]
    synced_lyrics: Option<String>,
    duration: Option<f64>,
}

pub async fn fetch_lrc_lib(
    client: &Client,
    title: &str,
    artist: &str,
    duration_ms: Option<i64>,
) -> Option<LyricResult> {
    let sec = duration_ms.filter(|&d| d > 0).map(|d| (d as f64 / 1000.0).round() as i64).unwrap_or(0);

    // 1. Exact match
    let mut get_url = url::Url::parse("https://lrclib.net/api/get").ok()?;
    get_url.query_pairs_mut().append_pair("track_name", title);
    get_url.query_pairs_mut().append_pair("artist_name", artist);
    if sec > 0 {
        get_url.query_pairs_mut().append_pair("duration", &sec.to_string());
    }

    if let Ok(res) = client
        .get(get_url)
        .header("Lrclib-Client", "AfterToneNative/1.0")
        .timeout(Duration::from_secs(8))
        .send()
        .await
    {
        if res.status().is_success() {
            if let Ok(hit) = res.json::<LrcLibHit>().await {
                if let Some(lrc) = hit.synced_lyrics {
                    let lines = parse_any_lrc(&lrc);
                    if !lines.is_empty() {
                        return Some(LyricResult {
                            source: "LRCLIB".to_string(),
                            label: "LRCLIB (Synced)".to_string(),
                            isrc: None,
                            is_word_synced: false,
                            lines,
                            raw_ttml: None,
                        });
                    }
                }
            }
        }
    }

    // 2. Fuzzy search fallback
    let mut search_url = url::Url::parse("https://lrclib.net/api/search").ok()?;
    search_url.query_pairs_mut().append_pair("track_name", title);
    search_url.query_pairs_mut().append_pair("artist_name", artist);

    if let Ok(res) = client
        .get(search_url)
        .header("Lrclib-Client", "AfterToneNative/1.0")
        .timeout(Duration::from_secs(8))
        .send()
        .await
    {
        if res.status().is_success() {
            if let Ok(hits) = res.json::<Vec<LrcLibHit>>().await {
                let mut with_sync: Vec<LrcLibHit> = hits.into_iter().filter(|h| h.synced_lyrics.is_some()).collect();
                if !with_sync.is_empty() {
                    if sec > 0 {
                        with_sync.sort_by(|a, b| {
                            let da = (a.duration.unwrap_or(0.0) - sec as f64).abs();
                            let db = (b.duration.unwrap_or(0.0) - sec as f64).abs();
                            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                        });
                    }
                    if let Some(first) = with_sync.into_iter().next() {
                        if let Some(lrc) = first.synced_lyrics {
                            let lines = parse_any_lrc(&lrc);
                            if !lines.is_empty() {
                                return Some(LyricResult {
                                    source: "LRCLIB".to_string(),
                                    label: "LRCLIB (Synced)".to_string(),
                                    isrc: None,
                                    is_word_synced: false,
                                    lines,
                                    raw_ttml: None,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    None
}

// ─── 6. KuGou ────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct KuGouSongInfo {
    hash: Option<String>,
    duration: Option<i64>,
}

#[derive(Deserialize)]
struct KuGouSearchResponse {
    data: Option<KuGouSearchData>,
}

#[derive(Deserialize)]
struct KuGouSearchData {
    info: Option<Vec<KuGouSongInfo>>,
}

#[derive(Deserialize)]
struct KuGouCandidate {
    id: Option<String>,
    accesskey: Option<String>,
}

#[derive(Deserialize)]
struct KuGouLyricsSearchResponse {
    candidates: Option<Vec<KuGouCandidate>>,
}

#[derive(Deserialize)]
struct KuGouDownloadResponse {
    content: Option<String>,
}

pub async fn fetch_kugou_lyrics(
    client: &Client,
    title: &str,
    artist: &str,
    duration_ms: Option<i64>,
) -> Option<LyricResult> {
    let keyword = format!("{} - {}", title, artist);
    let sec = duration_ms.unwrap_or(0) / 1000;

    let mut search_url = url::Url::parse("https://mobileservice.kugou.com/api/v3/search/song").ok()?;
    search_url.query_pairs_mut()
        .append_pair("version", "9108")
        .append_pair("plat", "0")
        .append_pair("pagesize", "8")
        .append_pair("showtype", "0")
        .append_pair("keyword", &keyword);

    let res = client
        .get(search_url)
        .timeout(Duration::from_secs(8))
        .send()
        .await
        .ok()?;

    if !res.status().is_success() {
        return None;
    }

    let search_data: KuGouSearchResponse = res.json().await.ok()?;
    let songs = search_data.data?.info?;

    let hashes: Vec<String> = songs
        .into_iter()
        .filter(|s| {
            if sec > 0 {
                if let Some(dur) = s.duration {
                    return (dur - sec).abs() <= 8;
                }
            }
            true
        })
        .filter_map(|s| s.hash)
        .collect();

    if hashes.is_empty() {
        return None;
    }

    let mut candidate = None;
    for h in hashes.iter().take(3) {
        let mut lrc_search_url = url::Url::parse("https://lyrics.kugou.com/search").ok()?;
        lrc_search_url.query_pairs_mut()
            .append_pair("ver", "1")
            .append_pair("man", "yes")
            .append_pair("client", "pc")
            .append_pair("hash", h);

        if let Ok(lrc_res) = client.get(lrc_search_url).timeout(Duration::from_secs(5)).send().await {
            if lrc_res.status().is_success() {
                if let Ok(lrc_data) = lrc_res.json::<KuGouLyricsSearchResponse>().await {
                    if let Some(mut cands) = lrc_data.candidates {
                        if !cands.is_empty() {
                            candidate = Some(cands.remove(0));
                            break;
                        }
                    }
                }
            }
        }
    }

    let cand = candidate?;
    let cand_id = cand.id?;
    let cand_key = cand.accesskey?;

    let mut dl_url = url::Url::parse("https://lyrics.kugou.com/download").ok()?;
    dl_url.query_pairs_mut()
        .append_pair("fmt", "lrc")
        .append_pair("charset", "utf8")
        .append_pair("client", "pc")
        .append_pair("ver", "1")
        .append_pair("id", &cand_id)
        .append_pair("accesskey", &cand_key);

    let dl_res = client.get(dl_url).timeout(Duration::from_secs(6)).send().await.ok()?;
    if !dl_res.status().is_success() {
        return None;
    }

    let dl_data: KuGouDownloadResponse = dl_res.json().await.ok()?;
    let b64 = dl_data.content?;
    use base64::Engine;
    let decoded_bytes = base64::engine::general_purpose::STANDARD.decode(b64.trim()).ok()?;
    let lrc_str = String::from_utf8_lossy(&decoded_bytes).to_string();

    let lines = parse_any_lrc(&lrc_str);
    if lines.is_empty() {
        return None;
    }

    Some(LyricResult {
        source: "KuGou".to_string(),
        label: "KuGou (Synced)".to_string(),
        isrc: None,
        is_word_synced: false,
        lines,
        raw_ttml: None,
    })
}

