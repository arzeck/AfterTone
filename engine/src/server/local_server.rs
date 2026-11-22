//! Built-in AfterTone Listen Together Party Server (Rust)
//! Implements AfterTone's backend protocol and Party API for local Wi-Fi party hosting,
//! YouTube Music InnerTube feed/search, 320kbps audio proxy, and synchronized karaoke lyrics.

use axum::body::Body;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use reqwest::Client as HttpClient;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, oneshot, RwLock};
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

use super::network::NetworkInfo;
use crate::audio::{proxy_upstream_stream, StreamQuery, StreamResolver};
use crate::download::{DownloadItem, DownloadManager, DownloadRequest};
use crate::innertube::{BrowseResponse, InnerTubeClient};
use crate::integrations::{DiscordRpcService, SmtcService};
use crate::library::LocalLibraryScanner;
use crate::lyrics::LyricsRepository;
use crate::protocol::{
    generate_code, normalise_code, ClientControlFrame, ClientFrame, JoinRequest, JoinResponse,
    PartyMember, PartyPlayback, PartyPreview, PartyPreviewMember, PartyQueue, PartySnapshot,
    ServerFrame,
};
use crate::settings::{AppSettings, SettingsManager};

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

pub struct InternalMember {
    pub member: PartyMember,
    pub token: String,
}

pub struct InternalParty {
    pub code: String,
    pub created_at_ms: i64,
    pub max_members: usize,
    pub host_only_control: bool,
    pub members: HashMap<String, InternalMember>,
    pub playback: PartyPlayback,
    pub queue: PartyQueue,
    pub tx: broadcast::Sender<ServerFrame>,
}

impl InternalParty {
    pub fn to_snapshot(&self) -> PartySnapshot {
        PartySnapshot {
            code: self.code.clone(),
            created_at_ms: self.created_at_ms,
            max_members: self.max_members,
            host_only_control: self.host_only_control,
            members: self.members.values().map(|m| m.member.clone()).collect(),
            playback: self.playback.clone(),
            queue: self.queue.clone(),
            server_ms: now_ms(),
        }
    }
}

#[derive(Clone)]
pub struct ServerState {
    pub parties: Arc<RwLock<HashMap<String, InternalParty>>>,
    pub port: u16,
    pub innertube: Arc<InnerTubeClient>,
    pub lyrics: Arc<LyricsRepository>,
    pub resolver: Arc<StreamResolver>,
    pub downloads: Arc<DownloadManager>,
    pub settings: Arc<SettingsManager>,
    pub discord: Arc<DiscordRpcService>,
    pub smtc: Arc<tokio::sync::Mutex<Option<SmtcService>>>,
    pub http_client: HttpClient,
}

pub struct LocalPartyServer {
    pub port: u16,
    pub is_running: bool,
    parties: Arc<RwLock<HashMap<String, InternalParty>>>,
    innertube: Arc<InnerTubeClient>,
    lyrics: Arc<LyricsRepository>,
    resolver: Arc<StreamResolver>,
    downloads: Arc<DownloadManager>,
    settings: Arc<SettingsManager>,
    discord: Arc<DiscordRpcService>,
    smtc: Arc<tokio::sync::Mutex<Option<SmtcService>>>,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl Default for LocalPartyServer {
    fn default() -> Self {
        Self::new()
    }
}

fn find_client_dir() -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let candidates = [
                parent.join("web"),
                parent.join("client"),
                parent.join("..").join("web"),
                parent.join("..").join("client"),
            ];
            for cand in &candidates {
                if cand.exists() && cand.join("index.html").exists() {
                    return Some(cand.clone());
                }
            }
        }
    }
    let local_candidates = [
        PathBuf::from("web"),
        PathBuf::from("..").join("web"),
        PathBuf::from("client"),
        PathBuf::from("..").join("client"),
    ];
    for cand in &local_candidates {
        if cand.exists() && cand.join("index.html").exists() {
            return Some(cand.clone());
        }
    }
    None
}

impl LocalPartyServer {
    pub fn new() -> Self {
        let resolver = Arc::new(StreamResolver::new());
        let downloads = DownloadManager::new(Arc::clone(&resolver));
        let settings = SettingsManager::new();

        Self {
            port: 8000,
            is_running: false,
            parties: Arc::new(RwLock::new(HashMap::new())),
            innertube: Arc::new(InnerTubeClient::new()),
            lyrics: Arc::new(LyricsRepository::new()),
            resolver,
            downloads,
            settings,
            discord: Arc::new(DiscordRpcService::new()),
            smtc: Arc::new(tokio::sync::Mutex::new(None)),
            shutdown_tx: None,
        }
    }

    pub fn set_smtc(&self, smtc: SmtcService) {
        let smtc_lock = Arc::clone(&self.smtc);
        tokio::spawn(async move {
            let mut lock = smtc_lock.lock().await;
            *lock = Some(smtc);
        });
    }

    pub fn smtc_handle(&self) -> Arc<tokio::sync::Mutex<Option<SmtcService>>> {
        Arc::clone(&self.smtc)
    }

    pub fn discord(&self) -> Arc<DiscordRpcService> {
        Arc::clone(&self.discord)
    }

    pub fn get_local_ip(&self) -> String {
        NetworkInfo::get_local_ip()
    }

    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub async fn start(&mut self, port: u16) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        if self.is_running {
            return Ok(self.base_url());
        }

        let listener = if port == 0 {
            TcpListener::bind("127.0.0.1:0")
                .await
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?
        } else {
            let mut bind_port = port;
            loop {
                match TcpListener::bind(format!("127.0.0.1:{}", bind_port)).await {
                    Ok(l) => break l,
                    Err(_err) if bind_port < port + 10 => {
                        bind_port += 1;
                    }
                    Err(err) => return Err(Box::new(err)),
                }
            }
        };

        let local_addr = listener
            .local_addr()
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
        self.port = local_addr.port();

        let state = ServerState {
            parties: Arc::clone(&self.parties),
            port: self.port,
            innertube: Arc::clone(&self.innertube),
            lyrics: Arc::clone(&self.lyrics),
            resolver: Arc::clone(&self.resolver),
            downloads: Arc::clone(&self.downloads),
            settings: Arc::clone(&self.settings),
            discord: Arc::clone(&self.discord),
            smtc: Arc::clone(&self.smtc),
            http_client: HttpClient::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()?,
        };

        let mut app = Router::new()
            .route("/manifest.json", get(addon_manifest_handler))
            .route("/search", get(addon_search_handler))
            .route("/stream/:id", get(addon_stream_handler))
            .route("/healthz", get(healthz_handler))
            .route("/api/time", get(time_handler))
            .route("/api/parties", post(create_party_handler))
            .route("/api/parties/:code/preview", get(preview_party_handler))
            .route("/api/parties/:code/join", post(join_party_handler))
            .route("/api/parties/:code/leave", post(leave_party_handler))
            .route("/ws/parties/:code", get(ws_handler))
            .route("/api/home", get(innertube_home_handler))
            .route("/api/explore", get(innertube_explore_handler))
            .route("/api/search", get(innertube_search_handler))
            .route("/api/suggestions", get(innertube_suggestions_handler))
            .route("/api/browse", get(innertube_browse_handler))
            .route("/api/next", get(innertube_next_handler))
            .route("/api/stream", get(stream_handler))
            .route("/api/stream/resolve", get(stream_resolve_handler))
            .route("/audio", get(stream_handler))
            .route("/api/lyrics", get(lyrics_handler))
            .route("/api/download", post(download_queue_handler))
            .route("/api/downloads", get(download_list_handler))
            .route("/api/download/:id", delete(download_delete_handler))
            .route("/api/download/file/:id", get(download_file_handler))
            .route("/api/settings", get(settings_get_handler).post(settings_update_handler))
            .route("/api/sources/test", post(sources_test_handler))
            .route("/api/library/local", get(local_library_handler))
            .route("/api/library/liked", get(library_empty_tracks_handler))
            .route("/api/library/history", get(library_empty_tracks_handler))
            .route("/api/library/playlists", get(library_playlists_handler))
            .route("/api/auth/status", get(auth_status_handler))
            .route("/api/auth/login", post(auth_ok_handler))
            .route("/api/auth/logout", post(auth_ok_handler))
            .route("/api/auth/switch-account", post(auth_ok_handler))
            .route("/api/integrations/presence", post(presence_handler));

        if let Some(client_dir) = find_client_dir() {
            let index_file = client_dir.join("index.html");
            let serve_dir = ServeDir::new(&client_dir).fallback(ServeFile::new(index_file));
            app = app.fallback_service(serve_dir);
        } else {
            app = app.route("/", get(dashboard_handler));
        }

        let app = app.layer(CorsLayer::permissive()).with_state(state);

        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        self.shutdown_tx = Some(shutdown_tx);
        self.is_running = true;

        let base = self.base_url();
        tokio::spawn(async move {
            let server = axum::serve(listener, app);
            let _ = server
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.await;
                })
                .await;
        });

        Ok(base)
    }

    pub async fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        self.parties.write().await.clear();
        self.is_running = false;
    }
}

// ─── Handlers ────────────────────────────────────────────────────────────────

async fn dashboard_handler(State(state): State<ServerState>) -> Html<String> {
    let party_count = state.parties.read().await.len();
    let local_ip = NetworkInfo::get_local_ip();
    let html = format!(
        r#"<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <title>AfterTone Party Server (Rust)</title>
  <style>
    body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; background: #121214; color: #f0f0f5; padding: 2rem; max-width: 600px; margin: 0 auto; line-height: 1.6; }}
    .status {{ display: inline-flex; align-items: center; gap: 8px; background: rgba(34, 197, 94, 0.15); color: #22c55e; padding: 6px 14px; border-radius: 9999px; font-weight: 600; font-size: 14px; }}
    .card {{ background: #1c1c22; border: 1px solid #2e2e38; border-radius: 12px; padding: 20px; margin-top: 20px; }}
    code {{ background: #262630; padding: 3px 8px; border-radius: 6px; font-family: monospace; color: #a78bfa; }}
  </style>
</head>
<body>
  <h1>🎵 AfterTone Listen Together Server (Native Rust)</h1>
  <div class="status">🟢 Server is RUNNING on Port {port}</div>
  <div class="card">
    <h3>Server Details</h3>
    <p><strong>Engine:</strong> High-Performance Tokio / Axum in Rust</p>
    <p><strong>Status:</strong> Active</p>
    <p><strong>Active Parties:</strong> {party_count}</p>
    <p><strong>Health Check:</strong> <a href="/healthz" style="color:#60a5fa">/healthz</a> (ok: true)</p>
    <p><strong>Time:</strong> <a href="/api/time" style="color:#60a5fa">/api/time</a></p>
  </div>
  <p style="color: #94a3b8; font-size: 13px; margin-top: 20px;">Use this server in AfterTone: <code>http://{local_ip}:{port}</code></p>
</body>
</html>"#,
        port = state.port,
        party_count = party_count,
        local_ip = local_ip
    );
    Html(html)
}

async fn healthz_handler() -> impl IntoResponse {
    let now = now_ms();
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        serde_json::json!({ "ok": true, "serverMs": now }).to_string(),
    )
}

async fn time_handler() -> impl IntoResponse {
    let now = now_ms();
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        serde_json::json!({ "serverMs": now }).to_string(),
    )
}

async fn create_party_handler(
    State(state): State<ServerState>,
    Json(body): Json<JoinRequest>,
) -> Response {
    if body.display_name.trim().is_empty() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({ "error": "invalid_identity", "message": "displayName required" })),
        )
            .into_response();
    }

    let mut parties = state.parties.write().await;
    let mut code = generate_code();
    while parties.contains_key(&code) {
        code = generate_code();
    }

    let now = now_ms();
    let member_id = format!("{:x}", rand::random::<u32>());
    let token = format!("{:x}{:x}", rand::random::<u64>(), rand::random::<u64>());

    let member = PartyMember {
        member_id: member_id.clone(),
        user_id: if !body.user_id.is_empty() {
            body.user_id
        } else {
            format!("user-{}", member_id)
        },
        display_name: body.display_name,
        avatar_url: body.avatar_url,
        is_host: true,
        connected: false,
        joined_at_ms: now,
        last_seen_ms: now,
    };

    let (tx, _) = broadcast::channel(128);

    let mut party = InternalParty {
        code: code.clone(),
        created_at_ms: now,
        max_members: body.max_members.unwrap_or(5),
        host_only_control: false,
        members: HashMap::new(),
        playback: PartyPlayback {
            seq: 0,
            track: None,
            queue_seq: 0,
            queue_length: 0,
            queue_index: -1,
            is_playing: false,
            position_ms: 0,
            anchor_ms: now,
            effective_position_ms: Some(0),
            updated_by: None,
            started_by: None,
            started_name: None,
            autoplay_enabled: Some(false),
            updated_at_ms: Some(now),
        },
        queue: PartyQueue {
            seq: 0,
            index: -1,
            items: Vec::new(),
        },
        tx,
    };

    party.members.insert(
        member_id.clone(),
        InternalMember {
            member: member.clone(),
            token: token.clone(),
        },
    );

    let snapshot = party.to_snapshot();
    parties.insert(code.clone(), party);

    let resp = JoinResponse {
        code,
        token,
        you: member,
        party: snapshot,
    };

    (StatusCode::CREATED, Json(resp)).into_response()
}

async fn preview_party_handler(
    State(state): State<ServerState>,
    Path(code_param): Path<String>,
) -> Response {
    let norm = normalise_code(&code_param);
    let parties = state.parties.read().await;
    let party = match parties.get(&norm) {
        Some(p) => p,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "code": "not_found", "message": "No party with that code exists." })),
            )
                .into_response();
        }
    };

    let host = party.members.values().find(|m| m.member.is_host);
    let host_name = host.map(|h| h.member.display_name.as_str()).unwrap_or("Host").to_string();

    let preview = PartyPreview {
        code: norm,
        host_name,
        is_full: party.members.len() >= party.max_members,
        max_members: party.max_members,
        member_count: party.members.len(),
        members: party
            .members
            .values()
            .map(|m| PartyPreviewMember {
                avatar_url: m.member.avatar_url.clone(),
                display_name: m.member.display_name.clone(),
                is_host: m.member.is_host,
            })
            .collect(),
    };

    Json(preview).into_response()
}

async fn join_party_handler(
    State(state): State<ServerState>,
    Path(code_param): Path<String>,
    Json(body): Json<JoinRequest>,
) -> Response {
    let norm = normalise_code(&code_param);
    let mut parties = state.parties.write().await;
    let party = match parties.get_mut(&norm) {
        Some(p) => p,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "not_found", "message": "Party not found" })),
            )
                .into_response();
        }
    };

    if body.display_name.trim().is_empty() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({ "error": "invalid_identity", "message": "displayName required" })),
        )
            .into_response();
    }

    if party.members.len() >= party.max_members {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": "party_full", "message": "Party is full" })),
        )
            .into_response();
    }

    let now = now_ms();
    let member_id = format!("{:x}", rand::random::<u32>());
    let token = format!("{:x}{:x}", rand::random::<u64>(), rand::random::<u64>());

    let member = PartyMember {
        member_id: member_id.clone(),
        user_id: if !body.user_id.is_empty() {
            body.user_id
        } else {
            format!("user-{}", member_id)
        },
        display_name: body.display_name,
        avatar_url: body.avatar_url,
        is_host: false,
        connected: false,
        joined_at_ms: now,
        last_seen_ms: now,
    };

    party.members.insert(
        member_id,
        InternalMember {
            member: member.clone(),
            token: token.clone(),
        },
    );

    let snapshot = party.to_snapshot();
    let resp = JoinResponse {
        code: norm,
        token,
        you: member,
        party: snapshot,
    };

    (StatusCode::OK, Json(resp)).into_response()
}

async fn leave_party_handler(
    State(state): State<ServerState>,
    Path(code_param): Path<String>,
    headers: HeaderMap,
) -> Response {
    let norm = normalise_code(&code_param);
    let auth = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .strip_prefix("Bearer ")
        .unwrap_or("")
        .trim();

    let mut parties = state.parties.write().await;
    if let Some(party) = parties.get_mut(&norm) {
        let member_to_remove = party
            .members
            .iter()
            .find(|(_, m)| m.token == auth)
            .map(|(k, _)| k.clone());

        if let Some(m_id) = member_to_remove {
            party.members.remove(&m_id);
            let members_list: Vec<PartyMember> = party.members.values().map(|m| m.member.clone()).collect();
            let _ = party.tx.send(ServerFrame::Members {
                members: members_list,
                max_members: party.max_members,
                host_only_control: Some(party.host_only_control),
                server_ms: now_ms(),
            });
        }
    }

    (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
}

// ─── WebSocket ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct WsQuery {
    token: Option<String>,
}

async fn ws_handler(
    State(state): State<ServerState>,
    Path(code_param): Path<String>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let norm = normalise_code(&code_param);

    let auth_token = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(|s| s.trim().to_string())
        .or(query.token);

    let token = match auth_token {
        Some(t) if !t.is_empty() => t,
        _ => return StatusCode::UNAUTHORIZED.into_response(),
    };

    let parties = state.parties.read().await;
    let party = match parties.get(&norm) {
        Some(p) => p,
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    let authed_member = party.members.values().find(|m| m.token == token);
    let member_id = match authed_member {
        Some(m) => m.member.member_id.clone(),
        None => return StatusCode::UNAUTHORIZED.into_response(),
    };

    drop(parties);

    ws.on_upgrade(move |socket| handle_ws_socket(socket, state, norm, member_id))
}

async fn handle_ws_socket(
    socket: WebSocket,
    state: ServerState,
    code: String,
    member_id: String,
) {
    let (mut ws_sender, mut ws_receiver) = socket.split();

    let (_party_tx, mut rx, welcome_msg, _members_msg) = {
        let mut parties = state.parties.write().await;
        let party = match parties.get_mut(&code) {
            Some(p) => p,
            None => return,
        };

        if let Some(m) = party.members.get_mut(&member_id) {
            m.member.connected = true;
            m.member.last_seen_ms = now_ms();
        }

        let you = match party.members.get(&member_id) {
            Some(m) => m.member.clone(),
            None => return,
        };

        let now = now_ms();
        let welcome = ServerFrame::Welcome {
            you,
            party: party.to_snapshot(),
            server_ms: now,
        };

        let members_list: Vec<PartyMember> = party.members.values().map(|m| m.member.clone()).collect();
        let members = ServerFrame::Members {
            members: members_list,
            max_members: party.max_members,
            host_only_control: Some(party.host_only_control),
            server_ms: now,
        };

        let _ = party.tx.send(members.clone());
        (party.tx.clone(), party.tx.subscribe(), welcome, members)
    };

    // Send Welcome
    if let Ok(json) = serde_json::to_string(&welcome_msg) {
        let _ = ws_sender.send(Message::Text(json)).await;
    }

    // Forward broadcast messages to this client
    let mut send_task = tokio::spawn(async move {
        while let Ok(frame) = rx.recv().await {
            if let Ok(json) = serde_json::to_string(&frame) {
                if ws_sender.send(Message::Text(json)).await.is_err() {
                    break;
                }
            }
        }
    });

    // Handle incoming client messages
    let code_clone = code.clone();
    let member_id_clone = member_id.clone();
    let state_clone = state.clone();

    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_receiver.next().await {
            if let Message::Text(txt) = msg {
                if let Ok(frame) = serde_json::from_str::<ClientFrame>(&txt) {
                    handle_client_frame(&state_clone, &code_clone, &member_id_clone, frame).await;
                }
            }
        }
    });

    tokio::select! {
        _ = &mut send_task => {},
        _ = &mut recv_task => {},
    }

    // On disconnect, mark connected = false
    let mut parties = state.parties.write().await;
    if let Some(party) = parties.get_mut(&code) {
        if let Some(m) = party.members.get_mut(&member_id) {
            m.member.connected = false;
        }
        let members_list: Vec<PartyMember> = party.members.values().map(|m| m.member.clone()).collect();
        let _ = party.tx.send(ServerFrame::Members {
            members: members_list,
            max_members: party.max_members,
            host_only_control: Some(party.host_only_control),
            server_ms: now_ms(),
        });
    }
}

async fn handle_client_frame(
    state: &ServerState,
    code: &str,
    member_id: &str,
    frame: ClientFrame,
) {
    let now = now_ms();
    let mut parties = state.parties.write().await;
    let party = match parties.get_mut(code) {
        Some(p) => p,
        None => return,
    };

    let is_host = party
        .members
        .get(member_id)
        .map(|m| m.member.is_host)
        .unwrap_or(false);

    let display_name = party
        .members
        .get(member_id)
        .map(|m| m.member.display_name.clone())
        .unwrap_or_else(|| "User".to_string());

    match frame {
        ClientFrame::Ping { client_ms } => {
            let _ = party.tx.send(ServerFrame::Pong {
                client_ms,
                server_ms: now,
            });
        }
        ClientFrame::Sync => {
            let _ = party.tx.send(ServerFrame::State {
                playback: party.playback.clone(),
                server_ms: now,
            });
        }
        ClientFrame::SyncQueue => {
            let _ = party.tx.send(ServerFrame::Queue {
                queue: party.queue.clone(),
                server_ms: now,
            });
        }
        ClientFrame::Report { position_ms, is_playing } => {
            if party.playback.is_playing == is_playing {
                party.playback.position_ms = position_ms;
            }
        }
        ClientFrame::Control(ctrl) => {
            if party.host_only_control && !is_host {
                let _ = party.tx.send(ServerFrame::Error {
                    error: "host_only".to_string(),
                    message: "Host has locked party controls.".to_string(),
                });
                return;
            }

            let mut playback_changed = false;
            let mut queue_changed = false;

            match ctrl {
                ClientControlFrame::Play { position_ms } => {
                    party.playback.is_playing = true;
                    if let Some(pos) = position_ms {
                        party.playback.position_ms = pos;
                    }
                    party.playback.anchor_ms = now + 350;
                    playback_changed = true;
                }
                ClientControlFrame::Pause { position_ms } => {
                    party.playback.is_playing = false;
                    if let Some(pos) = position_ms {
                        party.playback.position_ms = pos;
                    }
                    party.playback.anchor_ms = now;
                    playback_changed = true;
                }
                ClientControlFrame::Seek { position_ms } => {
                    party.playback.position_ms = position_ms;
                    party.playback.anchor_ms = now + if party.playback.is_playing { 350 } else { 0 };
                    playback_changed = true;
                }
                ClientControlFrame::SetTrack { track, position_ms, is_playing } => {
                    party.playback.track = Some(track);
                    party.playback.position_ms = position_ms;
                    party.playback.is_playing = is_playing;
                    party.playback.anchor_ms = now + if is_playing { 350 } else { 0 };
                    party.playback.started_by = Some(member_id.to_string());
                    party.playback.started_name = Some(display_name);
                    playback_changed = true;
                }
                ClientControlFrame::Next => {
                    if !party.queue.items.is_empty() && (party.queue.index + 1) < party.queue.items.len() as i64 {
                        party.queue.index += 1;
                        let next_track = party.queue.items[party.queue.index as usize].clone();
                        party.playback.track = Some(next_track);
                        party.playback.position_ms = 0;
                        party.playback.is_playing = true;
                        party.playback.anchor_ms = now + 350;
                        playback_changed = true;
                        queue_changed = true;
                    }
                }
                ClientControlFrame::Previous => {
                    if !party.queue.items.is_empty() && party.queue.index > 0 {
                        party.queue.index -= 1;
                        let prev_track = party.queue.items[party.queue.index as usize].clone();
                        party.playback.track = Some(prev_track);
                        party.playback.position_ms = 0;
                        party.playback.is_playing = true;
                        party.playback.anchor_ms = now + 350;
                        playback_changed = true;
                        queue_changed = true;
                    }
                }
                ClientControlFrame::QueueAdd { tracks, play_next } => {
                    if play_next && party.queue.index >= 0 && (party.queue.index as usize + 1) <= party.queue.items.len() {
                        let insert_idx = party.queue.index as usize + 1;
                        for (offset, track) in tracks.into_iter().enumerate() {
                            party.queue.items.insert(insert_idx + offset, track);
                        }
                    } else {
                        party.queue.items.extend(tracks);
                    }
                    party.queue.seq += 1;
                    party.playback.queue_seq = party.queue.seq;
                    party.playback.queue_length = party.queue.items.len() as i64;
                    queue_changed = true;
                    playback_changed = true;
                }
                ClientControlFrame::QueueRemove { video_id } => {
                    party.queue.items.retain(|t| t.video_id != video_id);
                    party.queue.seq += 1;
                    party.playback.queue_seq = party.queue.seq;
                    party.playback.queue_length = party.queue.items.len() as i64;
                    queue_changed = true;
                    playback_changed = true;
                }
                ClientControlFrame::QueueClear => {
                    party.queue.items.clear();
                    party.queue.index = -1;
                    party.queue.seq += 1;
                    party.playback.queue_seq = party.queue.seq;
                    party.playback.queue_length = 0;
                    queue_changed = true;
                    playback_changed = true;
                }
                ClientControlFrame::QueueMove { from_index, to_index, .. } => {
                    if from_index < party.queue.items.len() && to_index < party.queue.items.len() {
                        let item = party.queue.items.remove(from_index);
                        party.queue.items.insert(to_index, item);
                        party.queue.seq += 1;
                        party.playback.queue_seq = party.queue.seq;
                        queue_changed = true;
                    }
                }
                ClientControlFrame::SetHostOnlyControl { enabled } => {
                    if is_host {
                        party.host_only_control = enabled;
                        let members_list: Vec<PartyMember> = party.members.values().map(|m| m.member.clone()).collect();
                        let _ = party.tx.send(ServerFrame::Members {
                            members: members_list,
                            max_members: party.max_members,
                            host_only_control: Some(party.host_only_control),
                            server_ms: now,
                        });
                    }
                }
                ClientControlFrame::Kick { member_id: target_id } => {
                    if is_host && target_id != member_id {
                        party.members.remove(&target_id);
                        let members_list: Vec<PartyMember> = party.members.values().map(|m| m.member.clone()).collect();
                        let _ = party.tx.send(ServerFrame::Members {
                            members: members_list,
                            max_members: party.max_members,
                            host_only_control: Some(party.host_only_control),
                            server_ms: now,
                        });
                    }
                }
            }

            if playback_changed {
                party.playback.seq += 1;
                party.playback.updated_by = Some(member_id.to_string());
                party.playback.updated_at_ms = Some(now);
            }

            if queue_changed {
                let _ = party.tx.send(ServerFrame::Queue {
                    queue: party.queue.clone(),
                    server_ms: now,
                });
            }

            if playback_changed {
                let _ = party.tx.send(ServerFrame::State {
                    playback: party.playback.clone(),
                    server_ms: now,
                });
            }
        }
    }
}

// ─── Extra AfterTone Full-App Handlers ────────────────────────────────────────

#[derive(Serialize)]
struct CategoryItem {
    id: String,
    title: String,
    icon: String,
    query: String,
    color: String,
}

#[derive(Serialize)]
struct ExploreResponse {
    categories: Vec<CategoryItem>,
}

async fn innertube_home_handler(
    State(state): State<ServerState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    match state.innertube.fetch_home().await {
        Ok(feed) => Ok(Json(serde_json::to_value(feed).unwrap_or_default())),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

async fn innertube_explore_handler() -> Json<ExploreResponse> {
    let categories = vec![
        CategoryItem {
            id: "new_releases".into(),
            title: "New Releases".into(),
            icon: "Sparkles".into(),
            query: "New Releases Music".into(),
            color: "#6366f1".into(),
        },
        CategoryItem {
            id: "charts".into(),
            title: "Charts & Trending".into(),
            icon: "TrendingUp".into(),
            query: "Top 50 Global Hits".into(),
            color: "#ec4899".into(),
        },
        CategoryItem {
            id: "focus".into(),
            title: "Focus & Lofi".into(),
            icon: "Coffee".into(),
            query: "Lofi Hip Hop Chill Study".into(),
            color: "#10b981".into(),
        },
        CategoryItem {
            id: "chill".into(),
            title: "Chill & Relax".into(),
            icon: "Moon".into(),
            query: "Acoustic Chill Vibes".into(),
            color: "#3b82f6".into(),
        },
        CategoryItem {
            id: "workout".into(),
            title: "Workout Energy".into(),
            icon: "Zap".into(),
            query: "High Energy Workout Hits".into(),
            color: "#f59e0b".into(),
        },
        CategoryItem {
            id: "party".into(),
            title: "Party Dance".into(),
            icon: "Music".into(),
            query: "Dance Party Electronic".into(),
            color: "#8b5cf6".into(),
        },
    ];
    Json(ExploreResponse { categories })
}

#[derive(Deserialize)]
pub struct SearchQueryParam {
    pub q: Option<String>,
}

async fn innertube_search_handler(
    State(state): State<ServerState>,
    Query(param): Query<SearchQueryParam>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let q = param.q.unwrap_or_default();
    match state.innertube.search(&q).await {
        Ok(res) => Ok(Json(serde_json::to_value(res).unwrap_or_default())),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

#[derive(Deserialize)]
pub struct NextQueryParam {
    pub id: Option<String>,
}

async fn innertube_next_handler(
    State(state): State<ServerState>,
    Query(param): Query<NextQueryParam>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let id = param.id.unwrap_or_default();
    match state.innertube.fetch_radio(&id).await {
        Ok(tracks) => Ok(Json(serde_json::json!({ "tracks": tracks }))),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

async fn serve_local_audio(file_path: &str, headers: &HeaderMap) -> Response {
    let path = std::path::Path::new(file_path);
    if !path.exists() {
        return (StatusCode::NOT_FOUND, "Local audio file not found").into_response();
    }
    let data = match tokio::fs::read(path).await {
        Ok(d) => d,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    let total_len = data.len();
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("mp3").to_lowercase();
    let mime = match ext.as_str() {
        "mp3" => "audio/mpeg",
        "flac" => "audio/flac",
        "m4a" | "aac" => "audio/mp4",
        "wav" => "audio/wav",
        "ogg" | "opus" => "audio/ogg",
        _ => "audio/mpeg",
    };

    if let Some(range_header) = headers.get(header::RANGE).and_then(|h| h.to_str().ok()) {
        if let Some(spec) = range_header.strip_prefix("bytes=") {
            let parts: Vec<&str> = spec.split('-').collect();
            let start: usize = parts[0].parse().unwrap_or(0);
            let end: usize = if parts.len() > 1 && !parts[1].is_empty() {
                parts[1].parse().unwrap_or(total_len - 1).min(total_len - 1)
            } else {
                total_len - 1
            };

            if start <= end && start < total_len {
                let slice = &data[start..=end];
                let mut res = Response::new(Body::from(slice.to_vec()));
                *res.status_mut() = StatusCode::PARTIAL_CONTENT;
                res.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(mime));
                res.headers_mut().insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
                res.headers_mut().insert(header::CONTENT_LENGTH, HeaderValue::from(slice.len()));
                if let Ok(cr_val) = HeaderValue::from_str(&format!("bytes {}-{}/{}", start, end, total_len)) {
                    res.headers_mut().insert(header::CONTENT_RANGE, cr_val);
                }
                return res;
            }
        }
    }

    let mut res = Response::new(Body::from(data));
    *res.status_mut() = StatusCode::OK;
    res.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(mime));
    res.headers_mut().insert(header::CONTENT_LENGTH, HeaderValue::from(total_len));
    res.headers_mut().insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    res
}

async fn stream_resolve_handler(
    State(state): State<ServerState>,
    Query(query): Query<StreamQuery>,
) -> Response {
    let title = query.title.as_deref().unwrap_or("");
    let artist = query.artist.as_deref().unwrap_or("");
    let vid = query.id.as_deref();

    if let Some(src) = query.source.as_deref() {
        if src.eq_ignore_ascii_case("jiosaavn") || src.eq_ignore_ascii_case("saavn") {
            if let Some(saavn_stream) = state.resolver.resolve_audio_stream(title, artist, vid).await {
                let stream_proxy_url = format!("{}/api/stream?url={}", state.port, url::form_urlencoded::byte_serialize(saavn_stream.as_bytes()).collect::<String>());
                let body = serde_json::json!({
                    "success": true,
                    "source": "jiosaavn",
                    "quality": "320kbps",
                    "streamUrl": saavn_stream,
                    "proxyUrl": format!("http://127.0.0.1:{}", stream_proxy_url),
                });
                return (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "application/json")],
                    body.to_string(),
                ).into_response();
            }
            return (
                StatusCode::NOT_FOUND,
                [(header::CONTENT_TYPE, "application/json")],
                serde_json::json!({ "success": false, "error": "JioSaavn stream not found" }).to_string(),
            ).into_response();
        } else if src.eq_ignore_ascii_case("youtube") || src.eq_ignore_ascii_case("yt") {
            if let Some(v) = vid {
                if let Some(yt_stream) = StreamResolver::resolve_youtube_stream_url(v).await {
                    let stream_proxy_url = format!("{}/api/stream?url={}", state.port, url::form_urlencoded::byte_serialize(yt_stream.as_bytes()).collect::<String>());
                    let body = serde_json::json!({
                        "success": true,
                        "source": "youtube",
                        "quality": "251opus",
                        "streamUrl": yt_stream,
                        "proxyUrl": format!("http://127.0.0.1:{}", stream_proxy_url),
                    });
                    return (
                        StatusCode::OK,
                        [(header::CONTENT_TYPE, "application/json")],
                        body.to_string(),
                    ).into_response();
                }
            }
            return (
                StatusCode::NOT_FOUND,
                [(header::CONTENT_TYPE, "application/json")],
                serde_json::json!({ "success": false, "error": "YouTube stream not found" }).to_string(),
            ).into_response();
        }
    }

    // Default: race both simultaneously
    if let Some((url, source)) = state.resolver.resolve_fastest_stream(title, artist, vid).await {
        let stream_proxy_url = format!("{}/api/stream?url={}", state.port, url::form_urlencoded::byte_serialize(url.as_bytes()).collect::<String>());
        let body = serde_json::json!({
            "success": true,
            "source": source,
            "quality": if source == "jiosaavn" { "320kbps" } else { "251opus" },
            "streamUrl": url,
            "proxyUrl": format!("http://127.0.0.1:{}", stream_proxy_url),
        });
        return (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            body.to_string(),
        ).into_response();
    }

    (
        StatusCode::NOT_FOUND,
        [(header::CONTENT_TYPE, "application/json")],
        serde_json::json!({ "success": false, "error": "Could not resolve stream from any provider" }).to_string(),
    ).into_response()
}

async fn stream_handler(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Query(query): Query<StreamQuery>,
) -> Response {
    if let Some(ref cdn_url) = query.url {
        if cdn_url.starts_with("http") {
            return proxy_upstream_stream(&state.http_client, cdn_url, &headers).await;
        }
    }

    if let Some(ref id) = query.id {
        if id.starts_with("local:") {
            let file_path = id.trim_start_matches("local:");
            return serve_local_audio(file_path, &headers).await;
        }
    }

    let title = query.title.as_deref().unwrap_or("");
    let artist = query.artist.as_deref().unwrap_or("");
    let vid = query.id.as_deref();

    // Specific source requested
    if let Some(src) = query.source.as_deref() {
        if src.eq_ignore_ascii_case("jiosaavn") || src.eq_ignore_ascii_case("saavn") {
            if let Some(saavn_stream) = state.resolver.resolve_audio_stream(title, artist, vid).await {
                let res = proxy_upstream_stream(&state.http_client, &saavn_stream, &headers).await;
                if res.status().is_success() || res.status() == StatusCode::PARTIAL_CONTENT {
                    return res;
                }
            }
            return (StatusCode::NOT_FOUND, "Could not resolve JioSaavn audio stream").into_response();
        } else if src.eq_ignore_ascii_case("youtube") || src.eq_ignore_ascii_case("yt") {
            if let Some(v) = vid {
                if let Some(yt_stream) = StreamResolver::resolve_youtube_stream_url(v).await {
                    let res = proxy_upstream_stream(&state.http_client, &yt_stream, &headers).await;
                    if res.status().is_success() || res.status() == StatusCode::PARTIAL_CONTENT {
                        return res;
                    }
                }
            }
            return (StatusCode::NOT_FOUND, "Could not resolve YouTube audio stream").into_response();
        }
    }

    // Default: Concurrent race between JioSaavn and YouTube (yt-dlp)
    if let Some((stream_url, _source)) = state.resolver.resolve_fastest_stream(title, artist, vid).await {
        let res = proxy_upstream_stream(&state.http_client, &stream_url, &headers).await;
        if res.status().is_success() || res.status() == StatusCode::PARTIAL_CONTENT {
            return res;
        }
    }

    (StatusCode::NOT_FOUND, "Could not resolve audio stream").into_response()
}

#[derive(Deserialize)]
pub struct LyricsQueryParam {
    pub id: Option<String>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub duration: Option<i64>,
}

async fn lyrics_handler(
    State(state): State<ServerState>,
    Query(param): Query<LyricsQueryParam>,
) -> Json<serde_json::Value> {
    let title = param.title.unwrap_or_default();
    let artist = param.artist.unwrap_or_default();
    let duration_ms = param.duration.map(|d| d * 1000);
    let video_id = param.id.as_deref();

    let (best, all) = state
        .lyrics
        .fetch_lyrics(&title, &artist, duration_ms, video_id, None)
        .await;

    let available: Vec<serde_json::Value> = all
        .values()
        .map(|r| {
            serde_json::json!({
                "id": r.source,
                "name": r.label,
                "service": r.source,
                "type": if r.is_word_synced { "syllable" } else { "line" },
                "isWordSynced": r.is_word_synced,
            })
        })
        .collect();

    let current = best.map(|r| {
        serde_json::json!({
            "id": r.source,
            "name": r.label,
            "service": r.source,
            "type": if r.is_word_synced { "syllable" } else { "line" },
            "isWordSynced": r.is_word_synced,
            "lines": r.lines,
            "rawTtml": r.raw_ttml,
            "raw": r.raw_ttml,
        })
    });

    Json(serde_json::json!({
        "current": current,
        "available": available,
    }))
}

#[derive(Deserialize)]
pub struct LibraryQueryParam {
    pub path: Option<String>,
}

async fn local_library_handler(Query(param): Query<LibraryQueryParam>) -> Json<serde_json::Value> {
    let scan_path = param.path.unwrap_or_else(|| {
        if let Some(user_profile) = std::env::var_os("USERPROFILE") {
            let music_dir = std::path::PathBuf::from(user_profile).join("Music");
            if music_dir.exists() {
                return music_dir.to_string_lossy().to_string();
            }
        }
        ".".to_string()
    });

    let tracks = LocalLibraryScanner::scan_directory(&scan_path);
    Json(serde_json::json!({ "tracks": tracks, "path": scan_path }))
}

async fn library_empty_tracks_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "tracks": [] }))
}

async fn library_playlists_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "playlists": [] }))
}

async fn auth_status_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "connected": false,
        "isSessionActive": false
    }))
}

async fn auth_ok_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresencePayload {
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub is_playing: bool,
    pub position_secs: Option<i64>,
    pub duration_secs: Option<i64>,
}

async fn presence_handler(
    State(state): State<ServerState>,
    Json(payload): Json<PresencePayload>,
) -> StatusCode {
    let title = payload.title.clone();
    let artist = payload.artist.clone();
    let album = payload.album.clone();
    let is_playing = payload.is_playing;
    let pos = payload.position_secs.unwrap_or(0);
    let dur = payload.duration_secs.unwrap_or(0);

    // Update Discord RPC
    let discord = Arc::clone(&state.discord);
    let t = title.clone();
    let a = artist.clone();
    let alb = album.clone();
    tokio::spawn(async move {
        discord.update_presence(&t, &a, alb.as_deref(), is_playing, pos, dur).await;
    });

    // Update Windows SMTC
    if let Ok(smtc_opt) = state.smtc.try_lock() {
        if let Some(ref smtc) = *smtc_opt {
            smtc.update_metadata(
                &title,
                &artist,
                album.as_deref(),
                if dur > 0 { Some(std::time::Duration::from_secs(dur as u64)) } else { None },
            );
            smtc.set_playback(is_playing);
        }
    }

    StatusCode::OK
}

// ─── Browse & Suggestions Handlers ──────────────────────────────────────────

#[derive(Deserialize)]
pub struct BrowseQuery {
    pub id: String,
    pub params: Option<String>,
}

async fn innertube_browse_handler(
    State(state): State<ServerState>,
    Query(query): Query<BrowseQuery>,
) -> Result<Json<BrowseResponse>, StatusCode> {
    match state.innertube.fetch_browse(&query.id, query.params.as_deref()).await {
        Ok(browse) => Ok(Json(browse)),
        Err(err) => {
            eprintln!("[InnerTube] Browse error for {}: {}", query.id, err);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[derive(Deserialize)]
pub struct SuggestionsQuery {
    pub q: String,
}

async fn innertube_suggestions_handler(
    State(state): State<ServerState>,
    Query(query): Query<SuggestionsQuery>,
) -> Result<Json<Vec<String>>, StatusCode> {
    match state.innertube.fetch_suggestions(&query.q).await {
        Ok(suggestions) => Ok(Json(suggestions)),
        Err(err) => {
            eprintln!("[InnerTube] Suggestions error for {}: {}", query.q, err);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

// ─── Download Manager Handlers ──────────────────────────────────────────────

async fn download_queue_handler(
    State(state): State<ServerState>,
    Json(req): Json<DownloadRequest>,
) -> Json<DownloadItem> {
    let item = state.downloads.queue(req).await;
    Json(item)
}

async fn download_list_handler(
    State(state): State<ServerState>,
) -> Json<Vec<DownloadItem>> {
    let items = state.downloads.list().await;
    Json(items)
}

async fn download_delete_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    let ok = state.downloads.delete(&id).await;
    Json(serde_json::json!({ "ok": ok }))
}

async fn download_file_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    req: axum::extract::Request,
) -> Response {
    if let Some(item) = state.downloads.get(&id).await {
        if let Some(ref file_path_str) = item.file_path {
            let path = PathBuf::from(file_path_str);
            if path.exists() {
                use tower::ServiceExt;
                let service = tower_http::services::ServeFile::new(path);
                let res = service.oneshot(req).await.unwrap();
                return res.into_response();
            }
        }
    }
    StatusCode::NOT_FOUND.into_response()
}

// ─── Settings Handlers ──────────────────────────────────────────────────────

async fn settings_get_handler(
    State(state): State<ServerState>,
) -> Json<AppSettings> {
    Json(state.settings.get().await)
}

async fn settings_update_handler(
    State(state): State<ServerState>,
    Json(payload): Json<AppSettings>,
) -> Json<serde_json::Value> {
    state.settings.update(payload).await;
    Json(serde_json::json!({ "ok": true }))
}

#[derive(Deserialize)]
struct TestSourcePayload {
    url: String,
}

async fn sources_test_handler(
    State(state): State<ServerState>,
    Json(payload): Json<TestSourcePayload>,
) -> Json<serde_json::Value> {
    let test_url = format!("{}/manifest.json", payload.url.trim().trim_end_matches('/'));
    if let Ok(res) = state.http_client.get(&test_url).send().await {
        if res.status().is_success() {
            if let Ok(manifest) = res.json::<serde_json::Value>().await {
                return Json(serde_json::json!({ "ok": true, "manifest": manifest }));
            }
        }
    }
    Json(serde_json::json!({ "ok": false, "error": "Failed to connect to addon server manifest" }))
}

// ─── AfterTone Addon Protocol Handlers ───────────────────────────────────────

async fn addon_manifest_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "id": "aftertone-native",
        "name": "AfterTone Native (320kbps + Multi-Lyrics)",
        "version": "1.0.0",
        "resources": ["search", "stream"],
        "settings": [],
        "allowDownloads": true
    }))
}

#[derive(Deserialize)]
struct AddonSearchQuery {
    #[serde(default)]
    q: String,
}

async fn addon_search_handler(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Query(query): Query<AddonSearchQuery>,
) -> Json<serde_json::Value> {
    let q = query.q.trim();
    if q.is_empty() {
        return Json(serde_json::json!({ "tracks": [] }));
    }

    let host = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("127.0.0.1:4040");

    let results = state
        .innertube
        .search(q)
        .await
        .map(|r| r.tracks)
        .unwrap_or_default();
    let tracks: Vec<serde_json::Value> = results
        .into_iter()
        .map(|item| {
            serde_json::json!({
                "id": item.id,
                "title": item.title,
                "artist": item.artist,
                "album": item.album.unwrap_or_default(),
                "duration": item.duration as f64,
                "artworkURL": item.thumbnail,
                "format": "mp3",
                "audioQuality": "320kbps",
                "streamURL": format!("http://{}/stream/{}", host, item.id)
            })
        })
        .collect();

    Json(serde_json::json!({ "tracks": tracks }))
}

async fn addon_stream_handler(
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    let host = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("127.0.0.1:4040");

    Json(serde_json::json!({
        "url": format!("http://{}/api/stream?id={}", host, id),
        "format": "mp3",
        "quality": "320kbps",
        "bitrate": 320000.0,
        "sampleRate": 44100.0
    }))
}

