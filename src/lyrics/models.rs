//! Synced Lyrics Models & Timing Mathematics

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LyricAlignment {
    Start,
    End,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricWord {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

impl LyricWord {
    pub fn new(start_ms: i64, end_ms: i64, text: impl Into<String>) -> Self {
        Self {
            start_ms,
            end_ms,
            text: text.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricLine {
    pub time_ms: i64,
    pub text: String,
    pub words: Vec<LyricWord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sung_until_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<Box<LyricLine>>,
    pub alignment: LyricAlignment,
    pub roman: String,
    pub translation: String,
    pub is_gap: bool,
}

impl LyricLine {
    pub fn new(time_ms: i64, text: impl Into<String>) -> Self {
        let t = text.into().trim().to_string();
        let is_gap = t.is_empty();
        Self {
            time_ms,
            text: t,
            words: Vec::new(),
            sung_until_ms: None,
            background: None,
            alignment: LyricAlignment::Start,
            roman: String::new(),
            translation: String::new(),
            is_gap,
        }
    }
}

pub const MIN_GAP_MS: i64 = 4000;
pub const RISE_MS: f64 = 200.0;

/// Hermite interpolation: smoothstep easing 3t^2 - 2t^3
pub fn smooth_hermite(t: f64) -> f64 {
    let c = t.clamp(0.0, 1.0);
    c * c * (3.0 - 2.0 * c)
}

/// Calculates how far (0..1) the word has lifted off the floor at [position_ms].
pub fn word_lift(word: &LyricWord, position_ms: i64) -> f64 {
    let rising = ((position_ms - word.start_ms) as f64 / RISE_MS).clamp(0.0, 1.0);
    let falling = (1.0 - (position_ms - word.end_ms) as f64 / RISE_MS).clamp(0.0, 1.0);
    smooth_hermite(rising.min(falling))
}

/// Fractional character index into [text] revealed by the karaoke sweep at [position_ms].
pub fn revealed_chars(line: &LyricLine, position_ms: i64) -> f64 {
    let total_chars = line.text.chars().count() as f64;
    if line.words.is_empty() {
        return if position_ms >= line.time_ms {
            total_chars
        } else {
            0.0
        };
    }

    let first = &line.words[0];
    let last = &line.words[line.words.len() - 1];

    if position_ms <= first.start_ms {
        return 0.0;
    }
    if position_ms >= last.end_ms {
        return total_chars;
    }

    let mut revealed = 0.0;
    for (i, word) in line.words.iter().enumerate() {
        let word_len = word.text.chars().count() as f64;
        let len = word_len + if i + 1 < line.words.len() { 1.0 } else { 0.0 };

        if position_ms >= word.end_ms {
            revealed += len;
        } else if position_ms >= word.start_ms {
            let dur = (word.end_ms - word.start_ms).max(1) as f64;
            let frac = ((position_ms - word.start_ms) as f64 / dur).clamp(0.0, 1.0);
            revealed += frac * len;
            break;
        } else {
            break;
        }
    }

    revealed.min(total_chars)
}

static BG_PAREN_RE: OnceLock<Regex> = OnceLock::new();

/// Extracts bracketed background vocal answering phrases e.g. "(yeah yeah)"
pub fn with_background_vocals(lines: Vec<LyricLine>) -> Vec<LyricLine> {
    let re = BG_PAREN_RE.get_or_init(|| Regex::new(r"\s*[(\x{ff08}]([^)\x{ff09}]+)[)\x{ff09}]\s*$").unwrap());

    lines
        .into_iter()
        .map(|mut line| {
            if !line.words.is_empty() || line.background.is_some() || line.text.is_empty() {
                return line;
            }

            if let Some(caps) = re.captures(&line.text) {
                if let Some(m) = caps.get(0) {
                    let idx = m.start();
                    if idx > 0 {
                        let main_text = line.text[..idx].trim().to_string();
                        let bg_text = caps.get(1).map(|c| c.as_str().trim().to_string()).unwrap_or_default();
                        if !main_text.is_empty() && !bg_text.is_empty() {
                            let bg_align = match line.alignment {
                                LyricAlignment::Start => LyricAlignment::End,
                                LyricAlignment::End => LyricAlignment::Start,
                            };
                            let mut bg_line = LyricLine::new(line.time_ms, bg_text);
                            bg_line.alignment = bg_align;
                            line.text = main_text;
                            line.background = Some(Box::new(bg_line));
                        }
                    }
                }
            }
            line
        })
        .collect()
}

/// Inserts breathing dot gap lines for instrumental sections >= 4000ms.
pub fn with_instrumental_gaps(lines: Vec<LyricLine>) -> Vec<LyricLine> {
    if lines.is_empty() {
        return Vec::new();
    }
    let mut result = Vec::with_capacity(lines.len() * 2);

    for i in 0..lines.len() {
        if i > 0 {
            let prev = &lines[i - 1];
            let cur = &lines[i];

            let prev_end = if !prev.words.is_empty() {
                prev.words[prev.words.len() - 1].end_ms
            } else {
                prev.sung_until_ms.unwrap_or(prev.time_ms)
            };

            let gap = cur.time_ms - prev_end;
            if gap >= MIN_GAP_MS {
                let mut gap_line = LyricLine::new(prev_end + 500, "");
                gap_line.sung_until_ms = Some(cur.time_ms - 500);
                gap_line.is_gap = true;
                gap_line.alignment = LyricAlignment::Start;
                result.push(gap_line);
            }
        }
        result.push(lines[i].clone());
    }

    result
}

/// Alternates duet voice lines between Start (left) and End (right).
pub fn compute_line_alignments(lines: Vec<LyricLine>) -> Vec<LyricLine> {
    let mut is_left = true;
    lines
        .into_iter()
        .map(|mut line| {
            line.alignment = if is_left {
                LyricAlignment::Start
            } else {
                LyricAlignment::End
            };
            is_left = !is_left;
            line
        })
        .collect()
}
