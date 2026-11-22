//! AfterTone Audio Proxy Server
//! Local loopback HTTP streaming server with native HTTP 206 Byte-Range scrubbing.

use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use futures_util::TryStreamExt;
use reqwest::Client;
use serde::Deserialize;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tower_http::cors::CorsLayer;

use super::stream_resolver::StreamResolver;

#[derive(Deserialize)]
pub struct StreamQuery {
    pub id: Option<String>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub url: Option<String>,
    pub source: Option<String>,
}

#[derive(Clone)]
struct AppState {
    resolver: Arc<StreamResolver>,
    http_client: Client,
    port: u16,
}

pub struct AudioProxyServer {
    pub port: u16,
    pub is_running: bool,
    resolver: Arc<StreamResolver>,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl Default for AudioProxyServer {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioProxyServer {
    pub fn new() -> Self {
        Self {
            port: 0,
            is_running: false,
            resolver: Arc::new(StreamResolver::new()),
            shutdown_tx: None,
        }
    }

    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub fn get_stream_url(&self, video_id: Option<&str>, title: Option<&str>, artist: Option<&str>) -> String {
        let mut params = Vec::new();
        if let Some(v) = video_id {
            params.push(format!("id={}", url::form_urlencoded::byte_serialize(v.as_bytes()).collect::<String>()));
        }
        if let Some(t) = title {
            params.push(format!("title={}", url::form_urlencoded::byte_serialize(t.as_bytes()).collect::<String>()));
        }
        if let Some(a) = artist {
            params.push(format!("artist={}", url::form_urlencoded::byte_serialize(a.as_bytes()).collect::<String>()));
        }
        format!("{}/api/stream?{}", self.base_url(), params.join("&"))
    }

    pub async fn start(&mut self) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        if self.is_running && self.port > 0 {
            return Ok(self.base_url());
        }

        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let port = addr.port();
        self.port = port;

        let state = AppState {
            resolver: Arc::clone(&self.resolver),
            http_client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()?,
            port,
        };

        let app = Router::new()
            .route("/healthz", get(healthz_handler))
            .route("/api/stream", get(stream_handler))
            .route("/api/stream/resolve", get(stream_resolve_handler))
            .route("/audio", get(audio_handler))
            .layer(CorsLayer::permissive())
            .with_state(state);

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
        self.is_running = false;
    }
}

async fn healthz_handler(State(state): State<AppState>) -> impl IntoResponse {
    let body = serde_json::json!({
        "ok": true,
        "port": state.port,
    });
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        body.to_string(),
    )
}

async fn audio_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<StreamQuery>,
) -> Response {
    if let Some(ref cdn_url) = query.url {
        if cdn_url.starts_with("http") {
            return proxy_upstream_stream(&state.http_client, cdn_url, &headers).await;
        }
    }
    (StatusCode::BAD_REQUEST, "Missing or invalid cdn url parameter").into_response()
}

async fn stream_resolve_handler(
    State(state): State<AppState>,
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

    // Race both simultaneously
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
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<StreamQuery>,
) -> Response {
    if let Some(ref cdn_url) = query.url {
        if cdn_url.starts_with("http") {
            return proxy_upstream_stream(&state.http_client, cdn_url, &headers).await;
        }
    }

    let title = query.title.as_deref().unwrap_or("");
    let artist = query.artist.as_deref().unwrap_or("");
    let vid = query.id.as_deref();

    // Check if client explicitly requested a specific source
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

pub async fn proxy_upstream_stream(http_client: &Client, upstream_url: &str, client_headers: &HeaderMap) -> Response {
    let mut req = http_client
        .get(upstream_url)
        .header(
            header::USER_AGENT,
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/134.0.0.0 Safari/537.36",
        )
        .header(header::ACCEPT, "audio/*,*/*")
        .header(header::ACCEPT_ENCODING, "identity");

    if let Some(range_val) = client_headers.get(header::RANGE) {
        req = req.header(header::RANGE, range_val.clone());
    }

    let upstream_res = match req.send().await {
        Ok(r) => r,
        Err(err) => {
            return (
                StatusCode::BAD_GATEWAY,
                format!("Failed to connect to upstream audio: {}", err),
            )
                .into_response();
        }
    };

    let status = StatusCode::from_u16(upstream_res.status().as_u16()).unwrap_or(StatusCode::OK);
    let mut res_headers = HeaderMap::new();

    res_headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    res_headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    res_headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("public, max-age=3600"));

    if let Some(ct) = upstream_res.headers().get(header::CONTENT_TYPE) {
        res_headers.insert(header::CONTENT_TYPE, ct.clone());
    } else {
        res_headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("audio/mp4"));
    }

    if let Some(cl) = upstream_res.headers().get(header::CONTENT_LENGTH) {
        res_headers.insert(header::CONTENT_LENGTH, cl.clone());
    }
    if let Some(cr) = upstream_res.headers().get(header::CONTENT_RANGE) {
        res_headers.insert(header::CONTENT_RANGE, cr.clone());
    }

    let stream = upstream_res
        .bytes_stream()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e));
    let body = Body::from_stream(stream);

    (status, res_headers, body).into_response()
}
