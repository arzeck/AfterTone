//! AfterTone Party Client
//! Handles room creation, joining, WebSockets, NTP heartbeat, sequence guard, and playback controls.

use futures_util::{SinkExt, StreamExt};
use reqwest::Client as HttpClient;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, Mutex, RwLock};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::AUTHORIZATION;
use tokio_tungstenite::tungstenite::Message;

use crate::clock::ServerClock;
use crate::protocol::{
    normalise_code, ClientControlFrame, ClientFrame, JoinRequest, JoinResponse, PartyMember,
    PartyPlayback, PartyQueue, PartySnapshot, PartyTrack, ServerFrame,
};

pub const KNOWN_SERVERS: &[&str] = &[
    "http://127.0.0.1:8000",
    "https://party.aftertone.app",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
}

#[derive(Debug, Clone)]
pub enum ClientEvent {
    ConnectionStateChanged(ConnectionState, i64 /* rtt */),
    LatencyChanged(i64),
    StateChanged(PartyPlayback, i64 /* playheadMs */),
    QueueChanged(PartyQueue),
    MembersChanged(Vec<PartyMember>, bool /* hostOnly */),
    TrackChanged(Option<PartyTrack>),
    Error(String),
}

pub struct AfterTonePartyClient {
    pub code: Arc<RwLock<Option<String>>>,
    pub token: Arc<RwLock<Option<String>>>,
    pub you: Arc<RwLock<Option<PartyMember>>>,
    pub members: Arc<RwLock<Vec<PartyMember>>>,
    pub host_only_control: Arc<RwLock<bool>>,
    pub playback: Arc<RwLock<PartyPlayback>>,
    pub queue: Arc<RwLock<PartyQueue>>,
    pub connection_state: Arc<RwLock<ConnectionState>>,
    pub server_url: Arc<RwLock<String>>,
    pub clock: Arc<RwLock<ServerClock>>,

    device_id: String,
    user_id: String,
    http_client: HttpClient,
    event_tx: broadcast::Sender<ClientEvent>,
    outbound_tx: Arc<Mutex<Option<mpsc::Sender<ClientFrame>>>>,
    last_applied_seq: Arc<RwLock<i64>>,
}

impl Default for AfterTonePartyClient {
    fn default() -> Self {
        Self::new(None)
    }
}

impl AfterTonePartyClient {
    pub fn new(server_url: Option<&str>) -> Self {
        let (event_tx, _) = broadcast::channel(256);
        let default_server = server_url
            .unwrap_or("http://127.0.0.1:8000")
            .trim()
            .trim_end_matches('/')
            .to_string();

        let device_id = format!("native-{}", &format!("{:x}", rand::random::<u64>())[..8]);
        let user_id = format!("user-{}", &format!("{:x}", rand::random::<u64>())[..8]);

        Self {
            code: Arc::new(RwLock::new(None)),
            token: Arc::new(RwLock::new(None)),
            you: Arc::new(RwLock::new(None)),
            members: Arc::new(RwLock::new(Vec::new())),
            host_only_control: Arc::new(RwLock::new(false)),
            playback: Arc::new(RwLock::new(PartyPlayback::default())),
            queue: Arc::new(RwLock::new(PartyQueue::default())),
            connection_state: Arc::new(RwLock::new(ConnectionState::Disconnected)),
            server_url: Arc::new(RwLock::new(default_server)),
            clock: Arc::new(RwLock::new(ServerClock::new())),
            device_id,
            user_id,
            http_client: HttpClient::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
            event_tx,
            outbound_tx: Arc::new(Mutex::new(None)),
            last_applied_seq: Arc::new(RwLock::new(0)),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ClientEvent> {
        self.event_tx.subscribe()
    }

    pub async fn get_current_playhead(&self) -> i64 {
        let pb = self.playback.read().await;
        let clock = self.clock.read().await;
        clock.compute_playhead(
            pb.position_ms,
            pb.anchor_ms,
            pb.is_playing,
            pb.track.as_ref().and_then(|t| t.duration_ms),
        )
    }

    pub async fn join_party(
        &self,
        code: &str,
        display_name: Option<&str>,
        server_url: Option<&str>,
    ) -> Result<(), String> {
        let norm_code = normalise_code(code);
        if norm_code.is_empty() {
            return Err("Invalid party code. Must be 6 alphanumeric characters.".to_string());
        }

        if let Some(s) = server_url {
            *self.server_url.write().await = s.trim().trim_end_matches('/').to_string();
        }

        self.set_connection_state(ConnectionState::Connecting).await;

        let cur_server = self.server_url.read().await.clone();
        let mut servers_to_try = vec![cur_server];
        for &known in KNOWN_SERVERS {
            let norm = known.trim().trim_end_matches('/').to_string();
            if !servers_to_try.contains(&norm) {
                servers_to_try.push(norm);
            }
        }

        let body = JoinRequest {
            user_id: self.user_id.clone(),
            device_id: self.device_id.clone(),
            display_name: display_name.unwrap_or("Native Player").to_string(),
            avatar_url: None,
            max_members: None,
        };

        for server in &servers_to_try {
            let join_url = format!("{}/api/parties/{}/join", server, norm_code);
            let res = match self.http_client.post(&join_url).json(&body).send().await {
                Ok(r) => r,
                Err(_) => continue,
            };

            if res.status() == reqwest::StatusCode::NOT_FOUND {
                continue;
            }

            if !res.status().is_success() {
                let err_txt = res.text().await.unwrap_or_default();
                let msg = if err_txt.contains("party_full") {
                    format!("Party \"{}\" is full.", norm_code)
                } else {
                    format!("Failed to join party: {}", err_txt)
                };
                self.set_connection_state(ConnectionState::Disconnected).await;
                let _ = self.event_tx.send(ClientEvent::Error(msg.clone()));
                return Err(msg);
            }

            let data: JoinResponse = match res.json().await {
                Ok(d) => d,
                Err(e) => return Err(format!("Invalid response from party server: {}", e)),
            };

            *self.server_url.write().await = server.clone();
            *self.code.write().await = Some(data.code.clone());
            *self.token.write().await = Some(data.token.clone());
            *self.you.write().await = Some(data.you.clone());
            self.apply_snapshot(data.party).await;

            self.connect_ws(server, &data.code, &data.token).await?;
            return Ok(());
        }

        self.set_connection_state(ConnectionState::Disconnected).await;
        let msg = format!(
            "Party \"{}\" was not found on any server.\nServers tried:\n{}",
            norm_code,
            servers_to_try
                .iter()
                .map(|s| format!("  • {}", s))
                .collect::<Vec<_>>()
                .join("\n")
        );
        let _ = self.event_tx.send(ClientEvent::Error(msg.clone()));
        Err(msg)
    }

    pub async fn create_party(
        &self,
        display_name: Option<&str>,
        server_url: Option<&str>,
        max_members: Option<usize>,
    ) -> Result<String, String> {
        if let Some(s) = server_url {
            *self.server_url.write().await = s.trim().trim_end_matches('/').to_string();
        }

        self.set_connection_state(ConnectionState::Connecting).await;
        let cur_server = self.server_url.read().await.clone();

        let body = JoinRequest {
            user_id: self.user_id.clone(),
            device_id: self.device_id.clone(),
            display_name: display_name.unwrap_or("Native Player").to_string(),
            avatar_url: None,
            max_members,
        };

        let create_url = format!("{}/api/parties", cur_server);
        let res = match self.http_client.post(&create_url).json(&body).send().await {
            Ok(r) => r,
            Err(e) => {
                let msg = format!("Cannot reach party server at {}: {}", cur_server, e);
                self.set_connection_state(ConnectionState::Disconnected).await;
                let _ = self.event_tx.send(ClientEvent::Error(msg.clone()));
                return Err(msg);
            }
        };

        if !res.status().is_success() {
            let err_txt = res.text().await.unwrap_or_default();
            self.set_connection_state(ConnectionState::Disconnected).await;
            return Err(format!("Failed to create party: {}", err_txt));
        }

        let data: JoinResponse = match res.json().await {
            Ok(d) => d,
            Err(e) => return Err(format!("Invalid response JSON: {}", e)),
        };

        let code = data.code.clone();
        *self.code.write().await = Some(data.code.clone());
        *self.token.write().await = Some(data.token.clone());
        *self.you.write().await = Some(data.you.clone());
        self.apply_snapshot(data.party).await;

        self.connect_ws(&cur_server, &data.code, &data.token).await?;
        Ok(code)
    }

    pub async fn leave_party(&self) {
        let code = self.code.write().await.take();
        let token = self.token.write().await.take();
        let server = self.server_url.read().await.clone();

        self.outbound_tx.lock().await.take();

        if let (Some(c), Some(t)) = (code, token) {
            let leave_url = format!("{}/api/parties/{}/leave", server, c);
            let _ = self
                .http_client
                .post(&leave_url)
                .bearer_auth(t)
                .send()
                .await;
        }

        *self.you.write().await = None;
        self.members.write().await.clear();
        *self.playback.write().await = PartyPlayback::default();
        *self.queue.write().await = PartyQueue::default();
        self.clock.write().await.reset();
        self.set_connection_state(ConnectionState::Disconnected).await;
        let _ = self.event_tx.send(ClientEvent::TrackChanged(None));
    }

    // ─── Playback Controls ───────────────────────────────────────────────────

    pub async fn play(&self, position_ms: Option<i64>) {
        let pos = match position_ms {
            Some(p) => p,
            None => self.get_current_playhead().await,
        };
        self.send_control(ClientControlFrame::Play {
            position_ms: Some(pos),
        })
        .await;
    }

    pub async fn pause(&self, position_ms: Option<i64>) {
        let pos = match position_ms {
            Some(p) => p,
            None => self.get_current_playhead().await,
        };
        self.send_control(ClientControlFrame::Pause {
            position_ms: Some(pos),
        })
        .await;
    }

    pub async fn toggle_play_pause(&self) {
        let is_playing = self.playback.read().await.is_playing;
        if is_playing {
            self.pause(None).await;
        } else {
            self.play(None).await;
        }
    }

    pub async fn seek(&self, position_ms: i64) {
        self.send_control(ClientControlFrame::Seek {
            position_ms: position_ms.max(0),
        })
        .await;
    }

    pub async fn next(&self) {
        self.send_control(ClientControlFrame::Next).await;
    }

    pub async fn previous(&self) {
        self.send_control(ClientControlFrame::Previous).await;
    }

    pub async fn set_track(&self, track: PartyTrack, position_ms: i64, is_playing: bool) {
        self.send_control(ClientControlFrame::SetTrack {
            track,
            position_ms: position_ms.max(0),
            is_playing,
        })
        .await;
    }

    pub async fn queue_add(&self, tracks: Vec<PartyTrack>, play_next: bool) {
        self.send_control(ClientControlFrame::QueueAdd { tracks, play_next })
            .await;
    }

    pub async fn queue_remove(&self, video_id: &str) {
        self.send_control(ClientControlFrame::QueueRemove {
            video_id: video_id.to_string(),
        })
        .await;
    }

    pub async fn queue_clear(&self) {
        self.send_control(ClientControlFrame::QueueClear).await;
    }

    pub async fn queue_move(&self, from_index: usize, to_index: usize, video_id: Option<&str>) {
        self.send_control(ClientControlFrame::QueueMove {
            from_index,
            to_index,
            video_id: video_id.map(|v| v.to_string()),
        })
        .await;
    }

    pub async fn set_host_only_control(&self, enabled: bool) {
        self.send_control(ClientControlFrame::SetHostOnlyControl { enabled })
            .await;
    }

    pub async fn kick_member(&self, member_id: &str) {
        self.send_control(ClientControlFrame::Kick {
            member_id: member_id.to_string(),
        })
        .await;
    }

    async fn send_control(&self, frame: ClientControlFrame) {
        if let Some(ref tx) = *self.outbound_tx.lock().await {
            let _ = tx.send(ClientFrame::Control(frame)).await;
        }
    }

    // ─── WebSocket Connection & State ────────────────────────────────────────

    async fn connect_ws(&self, server: &str, code: &str, token: &str) -> Result<(), String> {
        let ws_scheme = if server.starts_with("https") { "wss" } else { "ws" };
        let base_no_scheme = server
            .strip_prefix("https://")
            .or_else(|| server.strip_prefix("http://"))
            .unwrap_or(server);
        let ws_url = format!("{}://{}/ws/parties/{}", ws_scheme, base_no_scheme, code);

        let mut req = ws_url
            .into_client_request()
            .map_err(|e| format!("Failed to create WS request: {}", e))?;
        req.headers_mut().insert(
            AUTHORIZATION,
            format!("Bearer {}", token)
                .parse()
                .map_err(|e| format!("Invalid auth header: {}", e))?,
        );

        let (ws_stream, _) = connect_async(req)
            .await
            .map_err(|e| format!("WebSocket handshake failed: {}", e))?;

        let (mut ws_sink, mut ws_stream) = ws_stream.split();
        let (outbound_tx, mut outbound_rx) = mpsc::channel::<ClientFrame>(64);
        *self.outbound_tx.lock().await = Some(outbound_tx.clone());

        self.set_connection_state(ConnectionState::Connected).await;

        // Ping loop
        let pinger_tx = outbound_tx.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(12));
            loop {
                interval.tick().await;
                let client_ms = ServerClock::local_now_ms();
                if pinger_tx.send(ClientFrame::Ping { client_ms }).await.is_err() {
                    break;
                }
            }
        });

        // Sink forwarding task
        tokio::spawn(async move {
            while let Some(frame) = outbound_rx.recv().await {
                if let Ok(json) = serde_json::to_string(&frame) {
                    if ws_sink.send(Message::Text(json)).await.is_err() {
                        break;
                    }
                }
            }
        });

        // Reader task
        let you_ref = Arc::clone(&self.you);
        let members_ref = Arc::clone(&self.members);
        let host_only_ref = Arc::clone(&self.host_only_control);
        let playback_ref = Arc::clone(&self.playback);
        let queue_ref = Arc::clone(&self.queue);
        let clock_ref = Arc::clone(&self.clock);
        let event_tx_clone = self.event_tx.clone();
        let last_seq_ref = Arc::clone(&self.last_applied_seq);

        tokio::spawn(async move {
            while let Some(Ok(msg)) = ws_stream.next().await {
                if let Message::Text(txt) = msg {
                    if let Ok(frame) = serde_json::from_str::<ServerFrame>(&txt) {
                        match frame {
                            ServerFrame::Welcome { you, party, server_ms } => {
                                *you_ref.write().await = Some(you);
                                if server_ms > 0 {
                                    let local_now = ServerClock::local_now_ms();
                                    clock_ref.write().await.record(local_now - 50, server_ms, local_now);
                                }
                                *members_ref.write().await = party.members.clone();
                                *host_only_ref.write().await = party.host_only_control;
                                *playback_ref.write().await = party.playback.clone();
                                *queue_ref.write().await = party.queue.clone();
                                *last_seq_ref.write().await = party.playback.seq;

                                let _ = event_tx_clone.send(ClientEvent::MembersChanged(party.members, party.host_only_control));
                                let _ = event_tx_clone.send(ClientEvent::QueueChanged(party.queue));
                                let playhead = clock_ref.read().await.compute_playhead(
                                    party.playback.position_ms,
                                    party.playback.anchor_ms,
                                    party.playback.is_playing,
                                    party.playback.track.as_ref().and_then(|t| t.duration_ms),
                                );
                                let _ = event_tx_clone.send(ClientEvent::StateChanged(party.playback.clone(), playhead));
                                let _ = event_tx_clone.send(ClientEvent::TrackChanged(party.playback.track));
                            }
                            ServerFrame::Pong { client_ms, server_ms } => {
                                let local_now = ServerClock::local_now_ms();
                                let mut clk = clock_ref.write().await;
                                clk.record(client_ms, server_ms, local_now);
                                let rtt = clk.round_trip_ms;
                                drop(clk);
                                let _ = event_tx_clone.send(ClientEvent::LatencyChanged(rtt));
                            }
                            ServerFrame::State { playback, server_ms } => {
                                if server_ms > 0 && clock_ref.read().await.offset_ms.is_none() {
                                    let local_now = ServerClock::local_now_ms();
                                    clock_ref.write().await.record(local_now - 50, server_ms, local_now);
                                }

                                let mut cur_seq = last_seq_ref.write().await;
                                if playback.seq <= *cur_seq {
                                    continue;
                                }
                                *cur_seq = playback.seq;

                                let old_track = playback_ref.read().await.track.clone();
                                *playback_ref.write().await = playback.clone();

                                if old_track != playback.track {
                                    let _ = event_tx_clone.send(ClientEvent::TrackChanged(playback.track.clone()));
                                }

                                let playhead = clock_ref.read().await.compute_playhead(
                                    playback.position_ms,
                                    playback.anchor_ms,
                                    playback.is_playing,
                                    playback.track.as_ref().and_then(|t| t.duration_ms),
                                );
                                let _ = event_tx_clone.send(ClientEvent::StateChanged(playback, playhead));
                            }
                            ServerFrame::Queue { queue, .. } => {
                                *queue_ref.write().await = queue.clone();
                                let _ = event_tx_clone.send(ClientEvent::QueueChanged(queue));
                            }
                            ServerFrame::Members { members, host_only_control, .. } => {
                                *members_ref.write().await = members.clone();
                                if let Some(h) = host_only_control {
                                    *host_only_ref.write().await = h;
                                }
                                let h = *host_only_ref.read().await;
                                let _ = event_tx_clone.send(ClientEvent::MembersChanged(members, h));
                            }
                            ServerFrame::Error { message, .. } => {
                                let _ = event_tx_clone.send(ClientEvent::Error(message));
                            }
                            ServerFrame::Bye { .. } => {
                                break;
                            }
                            _ => {}
                        }
                    }
                }
            }
        });

        Ok(())
    }

    async fn set_connection_state(&self, state: ConnectionState) {
        *self.connection_state.write().await = state;
        let rtt = self.clock.read().await.round_trip_ms;
        let _ = self.event_tx.send(ClientEvent::ConnectionStateChanged(state, rtt));
    }

    async fn apply_snapshot(&self, snapshot: PartySnapshot) {
        if snapshot.server_ms > 0 && self.clock.read().await.offset_ms.is_none() {
            let local_now = ServerClock::local_now_ms();
            self.clock.write().await.record(local_now - 50, snapshot.server_ms, local_now);
        }
        *self.members.write().await = snapshot.members.clone();
        *self.host_only_control.write().await = snapshot.host_only_control;
        *self.playback.write().await = snapshot.playback.clone();
        *self.queue.write().await = snapshot.queue.clone();
        *self.last_applied_seq.write().await = snapshot.playback.seq;

        let _ = self.event_tx.send(ClientEvent::MembersChanged(
            snapshot.members,
            snapshot.host_only_control,
        ));
        let _ = self.event_tx.send(ClientEvent::QueueChanged(snapshot.queue));
        let playhead = self.get_current_playhead().await;
        let _ = self.event_tx.send(ClientEvent::StateChanged(
            snapshot.playback.clone(),
            playhead,
        ));
        let _ = self.event_tx.send(ClientEvent::TrackChanged(snapshot.playback.track));
    }
}
