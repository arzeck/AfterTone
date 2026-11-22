//! Enhanced LRC, Karaoke LRC, and Standard LRC Parser.

use regex::Regex;
use std::sync::OnceLock;

use super::models::{
    with_background_vocals, with_instrumental_gaps, LyricLine, LyricWord,
};

static LINE_RE: OnceLock<Regex> = OnceLock::new();
static WORD_RE: OnceLock<Regex> = OnceLock::new();
static META_RE: OnceLock<Regex> = OnceLock::new();
static KARAOKE_LINE_RE: OnceLock<Regex> = OnceLock::new();
static KARAOKE_WORD_RE: OnceLock<Regex> = OnceLock::new();

fn line_re() -> &'static Regex {
    LINE_RE.get_or_init(|| Regex::new(r"^\[(\d{1,3}):(\d{2})[.:](\d{2,3})](.*)$").unwrap())
}

fn word_re() -> &'static Regex {
    WORD_RE.get_or_init(|| Regex::new(r"<(\d{1,3}):(\d{2})[.:](\d{2,3})>([^<]*)").unwrap())
}

fn meta_re() -> &'static Regex {
    META_RE.get_or_init(|| Regex::new(r"(?i)^\[(?:ti|ar|al|by|offset|length|re|ve):").unwrap())
}

fn karaoke_line_re() -> &'static Regex {
    KARAOKE_LINE_RE.get_or_init(|| Regex::new(r"^\[(\d{1,8}),(\d{1,8})](.*)$").unwrap())
}

fn karaoke_word_re() -> &'static Regex {
    KARAOKE_WORD_RE.get_or_init(|| Regex::new(r"\((\d{1,8}),(\d{1,8})(?:,\d{1,8})?\)([^()]*)").unwrap())
}

fn stamp(min_s: &str, sec_s: &str, frac_s: &str) -> i64 {
    let min: i64 = min_s.parse().unwrap_or(0);
    let sec: i64 = sec_s.parse().unwrap_or(0);
    let frac_raw: i64 = frac_s.parse().unwrap_or(0);
    let frac_ms = if frac_s.len() == 3 {
        frac_raw
    } else {
        frac_raw * 10
    };
    min * 60_000 + sec * 1_000 + frac_ms
}

pub fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

struct EnhancedRow {
    time_ms: i64,
    word_matches: Vec<(i64, String)>,
    plain: String,
}

/// Parses Enhanced LRC (Musixmatch rich sync, SimpMusic rich sync).
pub fn parse_enhanced_lrc(lrc: &str) -> Vec<LyricLine> {
    let mut rows: Vec<EnhancedRow> = Vec::new();
    let l_re = line_re();
    let w_re = word_re();
    let m_re = meta_re();

    for raw_line in lrc.lines() {
        let line = raw_line.trim();
        if line.is_empty() || m_re.is_match(line) {
            continue;
        }

        if let Some(caps) = l_re.captures(line) {
            let line_ms = stamp(&caps[1], &caps[2], &caps[3]);
            let content = &caps[4];

            let mut word_matches = Vec::new();
            for w_cap in w_re.captures_iter(content) {
                let w_time = stamp(&w_cap[1], &w_cap[2], &w_cap[3]);
                let w_text = w_cap[4].to_string();
                word_matches.push((w_time, w_text));
            }

            let plain_raw = w_re.replace_all(content, "").trim().to_string();
            let plain = decode_entities(&plain_raw);
            rows.push(EnhancedRow {
                time_ms: line_ms,
                word_matches,
                plain,
            });
        }
    }

    if !rows.iter().any(|r| !r.word_matches.is_empty()) {
        return Vec::new();
    }

    let mut parsed_lines = Vec::with_capacity(rows.len());
    for i in 0..rows.len() {
        let row = &rows[i];
        let next_line_ms = rows.get(i + 1).map(|r| r.time_ms);

        if row.word_matches.is_empty() {
            let mut l = LyricLine::new(row.time_ms, &row.plain);
            l.sung_until_ms = next_line_ms;
            parsed_lines.push(l);
            continue;
        }

        let mut words = Vec::with_capacity(row.word_matches.len());
        for (j, (w_start, w_text)) in row.word_matches.iter().enumerate() {
            let next_start = if j + 1 < row.word_matches.len() {
                row.word_matches[j + 1].0
            } else {
                next_line_ms.unwrap_or(w_start + 2000)
            };
            let cleaned = decode_entities(w_text.trim());
            if !cleaned.is_empty() {
                words.push(LyricWord::new(*w_start, next_start, cleaned));
            }
        }

        let mut line = LyricLine::new(row.time_ms, &row.plain);
        line.words = words;
        parsed_lines.push(line);
    }

    with_instrumental_gaps(with_background_vocals(parsed_lines))
}

/// Parses Karaoke LRC (QQ Music / NetEase format: [startMs, durMs](startMs, durMs)text).
pub fn parse_karaoke_lrc(lrc: &str) -> Vec<LyricLine> {
    let mut parsed_lines = Vec::new();
    let kl_re = karaoke_line_re();
    let kw_re = karaoke_word_re();
    let m_re = meta_re();

    for raw_line in lrc.lines() {
        let line = raw_line.trim();
        if line.is_empty() || m_re.is_match(line) {
            continue;
        }

        if let Some(caps) = kl_re.captures(line) {
            let line_start: i64 = caps[1].parse().unwrap_or(0);
            let line_dur: i64 = caps[2].parse().unwrap_or(0);
            let content = &caps[3];

            let mut words = Vec::new();
            for w_cap in kw_re.captures_iter(content) {
                let w_start: i64 = w_cap[1].parse().unwrap_or(0);
                let w_dur: i64 = w_cap[2].parse().unwrap_or(0);
                let w_text = decode_entities(w_cap[3].trim());
                if !w_text.is_empty() {
                    words.push(LyricWord::new(w_start, w_start + w_dur, w_text));
                }
            }

            let tag_re = Regex::new(r"\(\d{1,8},\d{1,8}(?:,\d{1,8})?\)").unwrap();
            let plain_raw = tag_re.replace_all(content, "").trim().to_string();
            let plain = decode_entities(&plain_raw);

            if !plain.is_empty() || !words.is_empty() {
                let mut line = LyricLine::new(line_start, plain);
                line.words = words;
                line.sung_until_ms = Some(line_start + line_dur);
                parsed_lines.push(line);
            }
        }
    }

    with_instrumental_gaps(with_background_vocals(parsed_lines))
}

/// Parses Standard line-synced LRC [mm:ss.xx]text.
pub fn parse_standard_lrc(lrc: &str) -> Vec<LyricLine> {
    let mut entries: Vec<(i64, String)> = Vec::new();
    let l_re = line_re();
    let m_re = meta_re();

    for raw_line in lrc.lines() {
        let line = raw_line.trim();
        if line.is_empty() || m_re.is_match(line) {
            continue;
        }

        if let Some(caps) = l_re.captures(line) {
            let time_ms = stamp(&caps[1], &caps[2], &caps[3]);
            let text = decode_entities(caps[4].trim());
            if !text.is_empty() {
                entries.push((time_ms, text));
            }
        }
    }

    entries.sort_by_key(|e| e.0);

    let mut parsed_lines = Vec::with_capacity(entries.len());
    for i in 0..entries.len() {
        let (time_ms, ref text) = entries[i];
        let next_time = entries.get(i + 1).map(|e| e.0);
        let mut line = LyricLine::new(time_ms, text.clone());
        line.sung_until_ms = next_time;
        parsed_lines.push(line);
    }

    with_instrumental_gaps(with_background_vocals(parsed_lines))
}

/// Auto-detects and parses any LRC variant.
pub fn parse_any_lrc(lrc: &str) -> Vec<LyricLine> {
    if lrc.trim().is_empty() {
        return Vec::new();
    }

    if lrc.contains('(') && karaoke_line_re().is_match(lrc) {
        let res = parse_karaoke_lrc(lrc);
        if !res.is_empty() {
            return res;
        }
    }

    if lrc.contains('<') && lrc.contains('>') {
        let res = parse_enhanced_lrc(lrc);
        if !res.is_empty() {
            return res;
        }
    }

    parse_standard_lrc(lrc)
}
