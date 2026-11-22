//! Apple Music TTML parser for word-timed and syllable-timed lyrics.

use regex::Regex;
use std::collections::HashSet;
use std::sync::OnceLock;

use super::models::{with_instrumental_gaps, LyricAlignment, LyricLine, LyricWord};

static P_REGEX: OnceLock<Regex> = OnceLock::new();
static SPAN_REGEX: OnceLock<Regex> = OnceLock::new();
static TAG_STRIP_REGEX: OnceLock<Regex> = OnceLock::new();

fn p_regex() -> &'static Regex {
    P_REGEX.get_or_init(|| Regex::new(r"(?is)<p\b([^>]*)>([\s\S]*?)</p>").unwrap())
}

fn span_regex() -> &'static Regex {
    SPAN_REGEX.get_or_init(|| Regex::new(r"(?is)<span\b([^>]*)>([\s\S]*?)</span>").unwrap())
}

fn tag_strip_regex() -> &'static Regex {
    TAG_STRIP_REGEX.get_or_init(|| Regex::new(r"<[^>]+>").unwrap())
}

pub fn strip_tags(html: &str) -> String {
    tag_strip_regex().replace_all(html, "").trim().to_string()
}

pub fn parse_ttml_time(val: &str) -> Option<i64> {
    let raw = val.trim();
    if raw.is_empty() {
        return None;
    }

    if let Some(ms_str) = raw.strip_suffix("ms") {
        return ms_str.trim().parse::<f64>().ok().map(|n| n.round() as i64);
    }

    let clean = raw.strip_suffix('s').unwrap_or(raw).trim();
    let parts: Vec<&str> = clean.split(':').collect();

    let seconds = match parts.len() {
        1 => parts[0].parse::<f64>().ok()?,
        2 => {
            let m = parts[0].parse::<f64>().ok()?;
            let s = parts[1].parse::<f64>().ok()?;
            m * 60.0 + s
        }
        3 => {
            let h = parts[0].parse::<f64>().ok()?;
            let m = parts[1].parse::<f64>().ok()?;
            let s = parts[2].parse::<f64>().ok()?;
            h * 3600.0 + m * 60.0 + s
        }
        _ => return None,
    };

    Some((seconds * 1000.0).round() as i64)
}

pub fn extract_attr(tag_str: &str, attr_name: &str) -> Option<String> {
    let stripped_attr = attr_name.split(':').last().unwrap_or(attr_name);
    let pattern = format!(
        r#"(?i)(?:{}|{})\s*=\s*["']([^"']*)["']"#,
        regex::escape(attr_name),
        regex::escape(stripped_attr)
    );
    let re = Regex::new(&pattern).ok()?;
    let caps = re.captures(tag_str)?;
    Some(caps[1].to_string())
}

struct RawSungLine {
    line: LyricLine,
    agent: Option<String>,
}

/// Parses Apple Music TTML string into an array of LyricLine objects.
pub fn parse_ttml_lyrics(ttml_text: &str) -> Vec<LyricLine> {
    if ttml_text.trim().is_empty() {
        return Vec::new();
    }

    let mut raw_sung: Vec<RawSungLine> = Vec::new();
    let p_re = p_regex();
    let span_re = span_regex();

    for p_cap in p_re.captures_iter(ttml_text) {
        let p_attrs = &p_cap[1];
        let p_body = &p_cap[2];

        let p_begin = extract_attr(p_attrs, "begin").and_then(|s| parse_ttml_time(&s));
        let p_end = extract_attr(p_attrs, "end").and_then(|s| parse_ttml_time(&s));
        let agent = extract_attr(p_attrs, "ttm:agent").or_else(|| extract_attr(p_attrs, "agent"));

        let mut lead_words: Vec<LyricWord> = Vec::new();
        let mut bg_words: Vec<LyricWord> = Vec::new();
        let mut roman_text = String::new();
        let mut trans_text = String::new();

        for s_cap in span_re.captures_iter(p_body) {
            let s_attrs = &s_cap[1];
            let s_content = strip_tags(&s_cap[2]);
            let role = extract_attr(s_attrs, "ttm:role").or_else(|| extract_attr(s_attrs, "role"));

            if let Some(r) = role.as_deref() {
                if r == "x-translation" {
                    if !trans_text.is_empty() {
                        trans_text.push(' ');
                    }
                    trans_text.push_str(&s_content);
                    continue;
                }
                if r == "x-roman" {
                    if !roman_text.is_empty() {
                        roman_text.push(' ');
                    }
                    roman_text.push_str(&s_content);
                    continue;
                }
            }

            let is_bg = role.as_deref() == Some("x-bg");
            let s_begin = extract_attr(s_attrs, "begin")
                .and_then(|s| parse_ttml_time(&s))
                .or(p_begin);
            let s_end = extract_attr(s_attrs, "end")
                .and_then(|s| parse_ttml_time(&s))
                .or(p_end);

            if let (Some(b), Some(e)) = (s_begin, s_end) {
                if !s_content.is_empty() {
                    let word = LyricWord::new(b, e, s_content);
                    if is_bg {
                        bg_words.push(word);
                    } else {
                        lead_words.push(word);
                    }
                }
            }
        }

        let bg_line = if !bg_words.is_empty() {
            let bg_text = bg_words
                .iter()
                .map(|w| w.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            let start = bg_words[0].start_ms;
            let mut l = LyricLine::new(start, bg_text);
            l.words = bg_words;
            Some(Box::new(l))
        } else {
            None
        };

        if lead_words.is_empty() {
            // Line-level paragraph
            let text = strip_tags(p_body);
            if text.is_empty() || p_begin.is_none() {
                continue;
            }
            let begin = p_begin.unwrap();
            let mut line = LyricLine::new(begin, text);
            line.sung_until_ms = p_end.filter(|&e| e > begin);
            line.background = bg_line;
            line.roman = roman_text;
            line.translation = trans_text;
            raw_sung.push(RawSungLine { line, agent });
        } else {
            // Word/syllable-timed paragraph
            let start_time = match p_begin {
                Some(b) => b.min(lead_words[0].start_ms),
                None => lead_words[0].start_ms,
            };
            let text = lead_words
                .iter()
                .map(|w| w.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            let mut line = LyricLine::new(start_time, text);
            line.words = lead_words;
            line.sung_until_ms = p_end;
            line.background = bg_line;
            line.roman = roman_text;
            line.translation = trans_text;
            raw_sung.push(RawSungLine { line, agent });
        }
    }

    if raw_sung.is_empty() {
        return Vec::new();
    }

    // Sort by start timestamp
    raw_sung.sort_by_key(|r| r.line.time_ms);

    // Duet alignments
    let mut unique_agents_order: Vec<String> = Vec::new();
    let mut seen_agents: HashSet<String> = HashSet::new();
    for r in &raw_sung {
        if let Some(ref a) = r.agent {
            if seen_agents.insert(a.clone()) {
                unique_agents_order.push(a.clone());
            }
        }
    }

    let lines: Vec<LyricLine> = raw_sung
        .into_iter()
        .map(|r| {
            let mut line = r.line;
            let alignment = if unique_agents_order.len() > 1 {
                if let Some(ref a) = r.agent {
                    if a == &unique_agents_order[0] {
                        LyricAlignment::Start
                    } else {
                        LyricAlignment::End
                    }
                } else {
                    LyricAlignment::Start
                }
            } else {
                LyricAlignment::Start
            };
            line.alignment = alignment;
            line
        })
        .collect();

    with_instrumental_gaps(lines)
}
