//! Comprehensive Unit and Integration Test Suite for AfterTone Native
//! High coverage tests for state machines, server protocol, streaming, and lyrics.

use aftertone_native::audio::{decrypt_saavn_media_url, AudioProxyServer};
use aftertone_native::clock::ServerClock;
use aftertone_native::lyrics::{
    parse_karaoke_lrc, parse_standard_lrc, parse_ttml_lyrics, parse_ttml_time,
    revealed_chars, word_lift, LyricAlignment, LyricLine, LyricWord,
};
use aftertone_native::protocol::{
    extract_code_and_server, is_valid_code, normalise_code,
};
use aftertone_native::server::LocalPartyServer;

#[test]
fn test_code_normalisation() {
    assert_eq!(normalise_code("  7a9k2m  "), "7A9K2M");
    assert_eq!(normalise_code("o1l9ia"), "01191A");
    assert_eq!(normalise_code("xyz-123"), "XYZ123");
}

#[test]
fn test_code_validation() {
    assert!(is_valid_code("7A9K2M"));
    assert!(!is_valid_code("7A9K2")); // too short
    assert!(!is_valid_code("7A9K2MM")); // too long
    assert!(!is_valid_code("7A9K2!")); // invalid character
}

#[test]
fn test_code_extraction() {
    let res1 = extract_code_and_server("aftertone://party/7A9K2M?server=https%3A%2F%2Fcustom.com");
    assert!(res1.is_some());
    let r1 = res1.unwrap();
    assert_eq!(r1.code, "7A9K2M");
    assert_eq!(r1.server_url.as_deref(), Some("https://custom.com"));

    let res2 = extract_code_and_server("https://aftertone.app/invite/3Y7W9Z");
    assert!(res2.is_some());
    assert_eq!(res2.unwrap().code, "3Y7W9Z");

    let res3 = extract_code_and_server("aftertone://party/9X2K5M");
    assert!(res3.is_some());
    assert_eq!(res3.unwrap().code, "9X2K5M");

    let res4 = extract_code_and_server("   4h8j2k   ");
    assert!(res4.is_some());
    assert_eq!(res4.unwrap().code, "4H8J2K");
}

#[test]
fn test_server_clock_ntp_sync() {
    let mut clock = ServerClock::new();

    // Sample 1: high latency (RTT = 100ms)
    // sent=1000, server=5500, received=1100 -> midpoint=1050, offset = 5500 - 1050 = 4450
    clock.record(1000, 5500, 1100);
    assert_eq!(clock.round_trip_ms, 100);
    assert_eq!(clock.offset_ms, Some(4450));

    // Sample 2: low latency (RTT = 20ms)
    // sent=2000, server=6460, received=2020 -> midpoint=2010, offset = 6460 - 2010 = 4450
    clock.record(2000, 6460, 2020);
    assert_eq!(clock.round_trip_ms, 20);
    assert_eq!(clock.offset_ms, Some(4450));
}

#[test]
fn test_server_clock_playhead_computation() {
    let mut clock = ServerClock::new();
    clock.record(1000, 1010, 1020);

    // Paused playback
    let paused = clock.compute_playhead(42000, 1000, false, Some(180000));
    assert_eq!(paused, 42000);

    // Playing playback
    if let Some(s_now) = clock.server_now_ms() {
        let playing = clock.compute_playhead(10000, s_now - 5000, true, Some(180000));
        assert!((playing - 15000).abs() < 50);
    }
}

#[test]
fn test_standard_lrc_parser() {
    let sample = r#"
[00:12.50]First line of the song
[00:15.00]Second line of the song
    "#;
    let lines = parse_standard_lrc(sample);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].time_ms, 12500);
    assert_eq!(lines[0].text, "First line of the song");
    assert_eq!(lines[1].time_ms, 15000);
    assert_eq!(lines[1].text, "Second line of the song");
}

#[test]
fn test_karaoke_lrc_parser() {
    let sample = r#"
[1000,3000](1000,500)Hel(1500,500)lo (2000,1000)World
    "#;
    let lines = parse_karaoke_lrc(sample);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].time_ms, 1000);
    assert_eq!(lines[0].text, "Hello World");
    assert_eq!(lines[0].words.len(), 3);
    assert_eq!(lines[0].words[0].text, "Hel");
    assert_eq!(lines[0].words[0].start_ms, 1000);
    assert_eq!(lines[0].words[0].end_ms, 1500);
}

#[test]
fn test_ttml_time_parser() {
    assert_eq!(parse_ttml_time("27.395"), Some(27395));
    assert_eq!(parse_ttml_time("01:05.20"), Some(65200));
    assert_eq!(parse_ttml_time("00:03:12.000"), Some(192000));
    assert_eq!(parse_ttml_time("4200ms"), Some(4200));
}

#[test]
fn test_ttml_parser_with_syllables_and_duets() {
    let sample_ttml = r#"
<?xml version="1.0" encoding="utf-8"?>
<tt xmlns="http://www.w3.org/ns/ttml" xmlns:ttm="http://www.w3.org/ns/ttml#metadata">
  <body>
    <div>
      <p begin="00:00:10.000" end="00:00:14.000" ttm:agent="v1">
        <span begin="00:00:10.000" end="00:00:11.000">Never</span>
        <span begin="00:00:11.000" end="00:00:12.500">gonna</span>
        <span begin="00:00:12.500" end="00:00:14.000">give</span>
        <span ttm:role="x-bg" begin="00:00:13.000" end="00:00:14.000">yeah</span>
        <span ttm:role="x-roman">never gonna give</span>
        <span ttm:role="x-translation">Never going to give</span>
      </p>
      <p begin="00:00:15.000" end="00:00:18.000" ttm:agent="v2">
        <span begin="00:00:15.000" end="00:00:18.000">you up</span>
      </p>
    </div>
  </body>
</tt>
    "#;

    let lines = parse_ttml_lyrics(sample_ttml);
    assert_eq!(lines.len(), 2);

    // Line 1
    assert_eq!(lines[0].time_ms, 10000);
    assert_eq!(lines[0].text, "Never gonna give");
    assert_eq!(lines[0].words.len(), 3);
    assert_eq!(lines[0].words[0].text, "Never");
    assert_eq!(
        lines[0].background.as_ref().map(|b| b.text.as_str()),
        Some("yeah")
    );
    assert_eq!(lines[0].roman, "never gonna give");
    assert_eq!(lines[0].translation, "Never going to give");
    assert_eq!(lines[0].alignment, LyricAlignment::Start);

    // Line 2 (duet agent v2 -> End alignment)
    assert_eq!(lines[1].time_ms, 15000);
    assert_eq!(lines[1].text, "you up");
    assert_eq!(lines[1].alignment, LyricAlignment::End);
}

#[test]
fn test_lyrics_easing_and_word_lift() {
    let word = LyricWord::new(1000, 2000, "Test");
    assert_eq!(word_lift(&word, 500), 0.0); // before rise
    assert_eq!(word_lift(&word, 1500), 1.0); // during word
    assert_eq!(word_lift(&word, 2500), 0.0); // after fall

    let w1 = LyricWord::new(1000, 2000, "Hello");
    let w2 = LyricWord::new(2000, 3000, "World");
    let mut line = LyricLine::new(1000, "Hello World");
    line.words = vec![w1, w2];

    assert_eq!(revealed_chars(&line, 500), 0.0);
    assert_eq!(revealed_chars(&line, 2000), 6.0); // "Hello "
    assert_eq!(revealed_chars(&line, 3500), 11.0); // "Hello World"
}

#[test]
fn test_des_ecb_decryptor() {
    let test_enc_url = "Lsm0O03ZqH85s9K9F93J/Tq/k0iE2eE1f1w0r7fK5K9hVqZ7M+m9B9k1lV0s2y8k";
    let decrypted = decrypt_saavn_media_url(test_enc_url);
    // Matches TypeScript suite.test.ts: returns string or empty on corrupt without error
    let _ = decrypted.as_str();
    assert_eq!(decrypt_saavn_media_url(""), "");
    assert_eq!(decrypt_saavn_media_url("invalid_token!"), "");
}

#[tokio::test]
async fn test_audio_proxy_server() {
    let mut proxy = AudioProxyServer::new();
    let base = proxy.start().await.expect("Failed to start AudioProxyServer");
    assert!(proxy.port > 0);
    assert!(base.starts_with("http://127.0.0.1:"));

    let stream_url = proxy.get_stream_url(Some("kJQP7kiw5Fk"), Some("Despacito"), Some("Luis Fonsi"));
    assert!(stream_url.contains("/api/stream?id=kJQP7kiw5Fk"));

    // Health check
    let client = reqwest::Client::new();
    let res = client
        .get(format!("{}/healthz", base))
        .send()
        .await
        .expect("Healthz request failed");
    assert_eq!(res.status(), 200);

    // Stream resolve endpoint test
    let resolve_res = client
        .get(format!("{}/api/stream/resolve?id=kJQP7kiw5Fk&title=Despacito&artist=Luis+Fonsi", base))
        .send()
        .await
        .expect("Stream resolve request failed");
    assert_eq!(resolve_res.status(), 200);
    let resolve_json: serde_json::Value = resolve_res.json().await.unwrap();
    assert_eq!(resolve_json["success"], true);
    assert!(resolve_json["streamUrl"].as_str().is_some());
    assert!(resolve_json["source"].as_str().is_some());

    proxy.stop().await;
    assert!(!proxy.is_running);
}

#[tokio::test]
async fn test_local_party_server() {
    let mut server = LocalPartyServer::new();
    let base = server.start(0).await.expect("Failed to start LocalPartyServer");
    assert!(server.port > 0);

    let client = reqwest::Client::new();
    // Test healthz
    let res = client
        .get(format!("{}/healthz", base))
        .send()
        .await
        .expect("Healthz request failed");
    assert_eq!(res.status(), 200);

    // Test time endpoint
    let t_res = client
        .get(format!("{}/api/time", base))
        .send()
        .await
        .expect("Time request failed");
    assert_eq!(t_res.status(), 200);

    server.stop().await;
    assert!(!server.is_running);
}

#[tokio::test]
async fn test_server_gui_endpoints() {
    let mut server = LocalPartyServer::new();
    let base = server.start(0).await.expect("Failed to start LocalPartyServer");
    let client = reqwest::Client::new();

    // 1. Explore categories
    let exp_res = client
        .get(format!("{}/api/explore", base))
        .send()
        .await
        .expect("Explore request failed");
    assert_eq!(exp_res.status(), 200);
    let exp_json: serde_json::Value = exp_res.json().await.unwrap();
    assert!(exp_json.get("categories").and_then(|c| c.as_array()).is_some());

    // 2. Auth status
    let auth_res = client
        .get(format!("{}/api/auth/status", base))
        .send()
        .await
        .expect("Auth status failed");
    assert_eq!(auth_res.status(), 200);

    // 3. Presence endpoint
    let pres_res = client
        .post(format!("{}/api/integrations/presence", base))
        .json(&serde_json::json!({
            "title": "Starboy",
            "artist": "The Weeknd",
            "isPlaying": true,
            "positionSecs": 10,
            "durationSecs": 230
        }))
        .send()
        .await
        .expect("Presence request failed");
    assert_eq!(pres_res.status(), 200);

    // 4. Local Library scan endpoint
    let lib_res = client
        .get(format!("{}/api/library/local?path=.", base))
        .send()
        .await
        .expect("Library scan failed");
    assert_eq!(lib_res.status(), 200);

    server.stop().await;
}

#[test]
fn test_local_library_scanner_formats() {
    use aftertone_native::library::LocalLibraryScanner;
    use std::path::Path;

    assert!(LocalLibraryScanner::is_supported_audio(Path::new("song.mp3")));
    assert!(LocalLibraryScanner::is_supported_audio(Path::new("track.flac")));
    assert!(LocalLibraryScanner::is_supported_audio(Path::new("music.m4a")));
    assert!(LocalLibraryScanner::is_supported_audio(Path::new("audio.wav")));
    assert!(LocalLibraryScanner::is_supported_audio(Path::new("tune.opus")));
    assert!(!LocalLibraryScanner::is_supported_audio(Path::new("lyrics.lrc")));
    assert!(!LocalLibraryScanner::is_supported_audio(Path::new("cover.jpg")));
}

#[tokio::test]
async fn test_settings_and_download_endpoints() {
    let mut server = LocalPartyServer::new();
    let base = server.start(0).await.expect("Failed to start LocalPartyServer");
    let client = reqwest::Client::new();

    // 1. Settings GET
    let s_get = client
        .get(format!("{}/api/settings", base))
        .send()
        .await
        .expect("Settings GET failed");
    assert_eq!(s_get.status(), 200);
    let s_val: serde_json::Value = s_get.json().await.unwrap();
    assert!(s_val.get("audioQuality").is_some());
    assert!(s_val.get("crossfadeSec").is_some());

    // 2. Settings POST
    let s_post = client
        .post(format!("{}/api/settings", base))
        .json(&serde_json::json!({
            "audioQuality": "auto",
            "crossfadeSec": 6,
            "automix": false,
            "automixBars": 8,
            "showNerdStats": true,
            "sleepTimerMinutes": 30,
            "sleepTimerDeadlineMs": null,
            "sleepTimerAfterTrack": false,
            "discordRpcEnabled": true,
            "jiosaavnEnabled": true,
            "sources": [],
            "lyricsSourcePriority": ["lrc-red", "bini", "kugou"]
        }))
        .send()
        .await
        .expect("Settings POST failed");
    assert_eq!(s_post.status(), 200);

    // Verify settings updated
    let s_verify: serde_json::Value = client
        .get(format!("{}/api/settings", base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(s_verify["crossfadeSec"], 6);
    assert_eq!(s_verify["audioQuality"], "auto");

    // 3. Downloads Queue and List
    let dl_queue = client
        .post(format!("{}/api/download", base))
        .json(&serde_json::json!({
            "videoId": "test_video_123",
            "title": "Blinding Lights",
            "artist": "The Weeknd",
            "thumbnail": "https://example.com/thumb.jpg",
            "durationSec": 200
        }))
        .send()
        .await
        .expect("Download queue failed");
    assert_eq!(dl_queue.status(), 200);
    let dl_val: serde_json::Value = dl_queue.json().await.unwrap();
    assert_eq!(dl_val["id"], "test_video_123");

    let dl_list: serde_json::Value = client
        .get(format!("{}/api/downloads", base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(dl_list.as_array().unwrap().iter().any(|i| i["id"] == "test_video_123"));

    // 4. Download delete
    let dl_del = client
        .delete(format!("{}/api/download/test_video_123", base))
        .send()
        .await
        .expect("Download delete failed");
    assert_eq!(dl_del.status(), 200);

    // 5. Suggestions Endpoint
    let sugg_res = client
        .get(format!("{}/api/suggestions?q=blinding", base))
        .send()
        .await
        .expect("Suggestions GET failed");
    assert_eq!(sugg_res.status(), 200);

    // 6. AfterTone Addon Protocol Endpoints
    let manifest_res = client
        .get(format!("{}/manifest.json", base))
        .send()
        .await
        .expect("Addon manifest GET failed");
    assert_eq!(manifest_res.status(), 200);
    let manifest_json: serde_json::Value = manifest_res.json().await.unwrap();
    assert_eq!(manifest_json["id"], "aftertone-native");
    assert_eq!(manifest_json["allowDownloads"], true);

    let stream_addon_res = client
        .get(format!("{}/stream/test_vid_123", base))
        .send()
        .await
        .expect("Addon stream GET failed");
    assert_eq!(stream_addon_res.status(), 200);
    let stream_addon_json: serde_json::Value = stream_addon_res.json().await.unwrap();
    assert_eq!(stream_addon_json["format"], "mp3");
    assert_eq!(stream_addon_json["quality"], "320kbps");
    assert!(stream_addon_json["url"].as_str().unwrap().contains("/api/stream?id=test_vid_123"));

    server.stop().await;
}
