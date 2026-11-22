//! # AfterTone Native
//!
//! A high-performance native Rust implementation of the Listen Together party protocol,
//! monotonic NTP server clock synchronization, multi-source concurrent karaoke lyrics racing engine,
//! pure-DES audio stream decryptor, local Wi-Fi party server, and interactive desktop player.

pub mod audio;
pub mod client;
pub mod clock;
pub mod download;
pub mod gui;
pub mod innertube;
pub mod integrations;
pub mod library;
pub mod lyrics;
pub mod protocol;
pub mod server;
pub mod settings;
pub mod ui;

pub use audio::*;
pub use client::*;
pub use clock::*;
pub use download::*;
pub use gui::*;
pub use innertube::{BrowseResponse, HomeFeed, HomeShelf, InnerTubeClient, InnertubeTrack, SearchResult};
pub use integrations::*;
pub use library::*;
pub use lyrics::{LyricLine, LyricResult, LyricWord, LyricsRepository};
pub use protocol::*;
pub use server::*;
pub use settings::*;
pub use ui::*;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
