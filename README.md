# AfterTone Native 🎵

A high-performance, native desktop music application and engine built in **Rust** featuring Listen Together synchronization, multi-provider concurrent karaoke lyrics racing, pure DES-ECB stream decryption, loopback HTTP 206 Partial Content range proxy, local Wi-Fi party server, and fluid Apple Music-style lyrics.

---

## ✨ Features & Architecture

### 1. 🔗 Protocol & Synchronization (`crate::protocol`, `crate::clock`)
- **Listen Together Wire Protocol**: Complete `serde`-compatible definitions for real-time collaborative listening (`PartyTrack`, `PartyMember`, `PartyPlayback`, `PartyQueue`, `PartySnapshot`, `ClientFrame`, `ServerFrame`).
- **Code Generation & Normalization**: 6-character party codes with ambiguity mapping (`I`/`L` &rarr; `1`, `O` &rarr; `0`), validation against base32 alphabet, and parsing from `aftertone://party/...`, invite links, query parameters, and plain strings.
- **Monotonic Server Clock (NTP)**: Zero-drift playhead arithmetic (`positionMs + (serverNow - anchorMs)`), sliding-window jitter filtering (best RTT selection), and immunity to local clock adjustments or DST shifts.

### 2. 🎤 Synced Karaoke Lyrics Engine (`crate::lyrics` & `am-lyrics`)
- **Multi-Provider Concurrent Racing**: Races `lrc.red`, `BiniLyrics`, `BetterLyrics`, `SimpMusic`, and `LRCLIB` in parallel, automatically promoting the highest-fidelity word/syllable-timed document.
- **Apple Music TTML Parser**: Parses XML timestamps (`s`, `ms`, `hh:mm:ss.mmm`), syllable `<span>` tags, background vocals (`ttm:role="x-bg"`), romanization (`x-roman`), translation (`x-translation`), and multi-vocalist duet alignments (`ttm:agent`).
- **Multi-Format LRC Parser**: Parses Enhanced LRC (`<mm:ss.xx>`), Karaoke LRC (`[start,dur](start,dur)`), and Standard line-synced LRC (`[mm:ss.xx]`).
- **Apple Music Fluid Visuals**: Word-by-word progressive karaoke reveal, syllable easing, fluid spring scrolling, blurred inactive lines, and clickable seek jumps powered by `am-lyrics`.

### 3. 🎧 Audio Stream Resolver & HTTP Range Proxy (`crate::audio`)
- **Pure Rust DES-ECB Cipher**: Decrypts high-fidelity CDN tokens using key `'38346591'` with bit permutations, S-boxes, Feistel rounds, and PKCS#5/7 unpadding (zero external crypto dependencies).
- **320kbps Audio Upgrade**: Automatically resolves 320kbps high-bitrate media files with in-memory TTL caching.
- **YouTube Extraction**: Integrated `yt-dlp` stream resolver using `visionos,android_creator,web_creator` player clients.
- **HTTP 206 Partial Content Proxy**: Loopback HTTP streaming server with native `Range: bytes=start-end` support for instant seeking and scrubbing.

### 4. 🌐 Built-in Local Wi-Fi Party Server (`crate::server`)
- **Axum / Tokio HTTP & WebSocket Server**:
  - `GET /`: Server status dashboard.
  - `GET /healthz`: Health status and server timestamp.
  - `GET /api/time`: Monotonic server timestamp.
  - `POST /api/parties`: Create a party room and issue token.
  - `GET /api/parties/:code/preview`: Room preview (host, member count, fullness).
  - `POST /api/parties/:code/join`: Join party room.
  - `POST /api/parties/:code/leave`: Leave party room.
  - `GET /ws/parties/:code`: Authenticated WebSocket connection.
- **Physical LAN IP Detection**: Automatic detection of primary physical Wi-Fi or Ethernet IPv4 address, filtering out virtual adapters (WSL, Docker, Tailscale) and CGNAT.

### 5. 👥 AfterTone Party Client (`crate::client`)
- Async WebSocket party client with automatic reconnection, heartbeat pings every 12s, sequence guard to prevent oscillation, and event broadcasting channel.
- Controls: `play`, `pause`, `seek`, `next`, `previous`, `set_track`, `queue_add`, `queue_remove`, `queue_clear`, `queue_move`, `set_host_only_control`, `kick_member`.

### 6. 💻 Interactive Terminal Karaoke UI (`crate::ui`)
- Rich `crossterm` terminal UI rendering live word-by-word glowing syllables, lead/duet alignments, instrumental breathing dots, progress bar, ping latency, and keyboard controls.

---

## 🚀 CLI Usage

```bash
# 1. Start native desktop GUI (Compose Desktop or WebView)
aftertone

# 2. Start local party server on port 8000
aftertone serve --port 8000

# 3. Join a party room from terminal with live karaoke UI
aftertone join 7A9K2M --tui

# 4. Create a new party room
aftertone create --name "My Party Host" --tui

# 5. Race lyrics across all providers
aftertone lyrics --title "Blinding Lights" --artist "The Weeknd"

# 6. Resolve 320kbps audio stream URL
aftertone stream --title "Shape of You" --artist "Ed Sheeran"

# 7. Start audio range proxy server
aftertone proxy
```

---

## 🧪 Testing

Run the comprehensive unit and integration test suite:

```bash
cargo test
```

---

## 🙏 Credits & Upstream Attribution

- **BitChord** ([kushagrasinghx/BitChord](https://github.com/kushagrasinghx/BitChord)): Credit to Kushagra Singh and the BitChord project for the Listen Together synchronization protocol inspiration, jam invite link schemas, and desktop player architecture.
- **am-lyrics** ([binimum/am-lyrics](https://github.com/binimum/am-lyrics)): Credit to Max Bai (`binimum`) for the Apple Music-fidelity web component and word-by-word synchronized lyrics rendering engine.
- **yt-music-better**: Credit to the open-source community for timing models and provider racing concepts.
