//! Local Music Library Scanner
//! Recursively scans local folders for audio files and extracts tags using `lofty`.

use lofty::file::{AudioFile, TaggedFileExt};
use lofty::probe::Probe;
use lofty::tag::Accessor;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalTrack {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: i64,
    #[serde(rename = "durationFormatted")]
    pub duration_formatted: String,
    pub path: String,
    pub format: String,
    #[serde(rename = "bitrateKbps")]
    pub bitrate_kbps: Option<u32>,
}

pub struct LocalLibraryScanner;

impl LocalLibraryScanner {
    pub fn is_supported_audio(path: &Path) -> bool {
        match path.extension().and_then(|e| e.to_str()).map(|s| s.to_lowercase()) {
            Some(ext) => matches!(ext.as_str(), "mp3" | "flac" | "m4a" | "wav" | "aac" | "ogg" | "opus" | "wma"),
            None => false,
        }
    }

    pub fn scan_directory<P: AsRef<Path>>(root: P) -> Vec<LocalTrack> {
        let mut tracks = Vec::new();
        let walker = WalkDir::new(root).follow_links(true).into_iter().filter_map(|e| e.ok());

        for entry in walker {
            let path = entry.path();
            if path.is_file() && Self::is_supported_audio(path) {
                if let Some(track) = Self::read_track(path) {
                    tracks.push(track);
                }
            }
        }

        tracks
    }

    pub fn read_track(path: &Path) -> Option<LocalTrack> {
        let tagged_file = Probe::open(path).ok()?.read().ok()?;
        let tag = tagged_file.primary_tag().or_else(|| tagged_file.first_tag());

        let title = tag
            .and_then(|t| t.title())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Unknown Title")
                    .to_string()
            });

        let artist = tag
            .and_then(|t| t.artist())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Unknown Artist".to_string());

        let album = tag
            .and_then(|t| t.album())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Unknown Album".to_string());

        let properties = tagged_file.properties();
        let duration_secs = properties.duration().as_secs() as i64;
        let bitrate_kbps = properties.audio_bitrate();

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("audio")
            .to_uppercase();

        let m = duration_secs / 60;
        let s = duration_secs % 60;
        let duration_formatted = format!("{}:{:02}", m, s);

        let id = format!("local:{}", path.to_string_lossy());

        Some(LocalTrack {
            id,
            title,
            artist,
            album,
            duration: duration_secs,
            duration_formatted,
            path: path.to_string_lossy().to_string(),
            format: ext,
            bitrate_kbps,
        })
    }

    pub fn get_default_music_dir() -> Option<PathBuf> {
        if let Some(profile) = std::env::var_os("USERPROFILE") {
            let music = PathBuf::from(profile).join("Music");
            if music.exists() {
                return Some(music);
            }
        }
        None
    }
}

