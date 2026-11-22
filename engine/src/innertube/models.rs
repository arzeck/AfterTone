use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InnertubeTrack {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub duration: i64,
    #[serde(rename = "durationFormatted")]
    pub duration_formatted: String,
    pub thumbnail: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub views: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlbumItem {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub thumbnail: String,
    #[serde(rename = "browseId", default, skip_serializing_if = "Option::is_none")]
    pub browse_id: Option<String>,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub album_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HomeShelf {
    pub title: String,
    pub items: Vec<InnertubeTrack>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HomeFeed {
    pub recents: Vec<InnertubeTrack>,
    #[serde(rename = "quickPicks")]
    pub quick_picks: Vec<InnertubeTrack>,
    #[serde(rename = "albumsAndSingles")]
    pub albums_and_singles: Vec<AlbumItem>,
    pub shelves: Vec<HomeShelf>,
    #[serde(rename = "isPersonalized")]
    pub is_personalized: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub tracks: Vec<InnertubeTrack>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowseResponse {
    #[serde(rename = "pageType")]
    pub page_type: String, // "album" | "artist" | "playlist" | "generic"
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(rename = "secondSubtitle", default, skip_serializing_if = "Option::is_none")]
    pub second_subtitle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<String>,
    pub tracks: Vec<InnertubeTrack>,
    pub shelves: Vec<HomeShelf>,
}

