//! AfterTone Listen Together Protocol Types
//! High-performance real-time party playback and membership synchronization models.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartyTrack {
    pub video_id: String,
    pub title: String,
    pub artist: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_autoplay: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartyMember {
    pub member_id: String,
    pub user_id: String,
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    pub is_host: bool,
    pub connected: bool,
    pub joined_at_ms: i64,
    pub last_seen_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartyPlayback {
    pub seq: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track: Option<PartyTrack>,
    pub queue_seq: i64,
    pub queue_length: i64,
    pub queue_index: i64,
    pub is_playing: bool,
    pub position_ms: i64,
    pub anchor_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_position_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autoplay_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at_ms: Option<i64>,
}

impl Default for PartyPlayback {
    fn default() -> Self {
        Self {
            seq: 0,
            track: None,
            queue_seq: 0,
            queue_length: 0,
            queue_index: -1,
            is_playing: false,
            position_ms: 0,
            anchor_ms: 0,
            effective_position_ms: None,
            updated_by: None,
            started_by: None,
            started_name: None,
            autoplay_enabled: None,
            updated_at_ms: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PartyQueue {
    pub seq: i64,
    pub index: i64,
    pub items: Vec<PartyTrack>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartySnapshot {
    pub code: String,
    pub created_at_ms: i64,
    pub max_members: usize,
    pub host_only_control: bool,
    pub members: Vec<PartyMember>,
    pub playback: PartyPlayback,
    pub queue: PartyQueue,
    pub server_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinRequest {
    pub user_id: String,
    pub device_id: String,
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_members: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinResponse {
    pub code: String,
    pub token: String,
    pub you: PartyMember,
    pub party: PartySnapshot,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartyPreview {
    pub code: String,
    pub host_name: String,
    pub is_full: bool,
    pub max_members: usize,
    pub member_count: usize,
    pub members: Vec<PartyPreviewMember>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartyPreviewMember {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    pub display_name: String,
    pub is_host: bool,
}

/// Client to Server WebSocket message frames
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ClientFrame {
    Ping {
        client_ms: i64,
    },
    Sync,
    SyncQueue,
    Report {
        position_ms: i64,
        is_playing: bool,
    },
    Control(ClientControlFrame),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum ClientControlFrame {
    Play {
        #[serde(skip_serializing_if = "Option::is_none")]
        position_ms: Option<i64>,
    },
    Pause {
        #[serde(skip_serializing_if = "Option::is_none")]
        position_ms: Option<i64>,
    },
    Seek {
        position_ms: i64,
    },
    Next,
    Previous,
    SetTrack {
        track: PartyTrack,
        position_ms: i64,
        is_playing: bool,
    },
    QueueAdd {
        tracks: Vec<PartyTrack>,
        #[serde(default)]
        play_next: bool,
    },
    QueueRemove {
        video_id: String,
    },
    QueueClear,
    QueueMove {
        from_index: usize,
        to_index: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        video_id: Option<String>,
    },
    SetHostOnlyControl {
        enabled: bool,
    },
    Kick {
        member_id: String,
    },
}

/// Server to Client WebSocket message frames
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ServerFrame {
    Welcome {
        you: PartyMember,
        party: PartySnapshot,
        server_ms: i64,
    },
    Pong {
        client_ms: i64,
        server_ms: i64,
    },
    State {
        playback: PartyPlayback,
        server_ms: i64,
    },
    Queue {
        queue: PartyQueue,
        server_ms: i64,
    },
    Members {
        members: Vec<PartyMember>,
        max_members: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        host_only_control: Option<bool>,
        server_ms: i64,
    },
    Activity {
        action: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        actor: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        actor_name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        timestamp: Option<i64>,
    },
    Error {
        error: String,
        message: String,
    },
    Bye {
        reason: String,
    },
}
