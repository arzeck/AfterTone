use anyhow::Result;
use clap::{Parser, Subcommand};
use std::sync::Arc;

use aftertone_native::audio::{AudioProxyServer, StreamResolver};
use aftertone_native::client::AfterTonePartyClient;
use aftertone_native::gui::{DesktopWindow, NativeDesktopApp};
use aftertone_native::integrations::{SmtcAction, SmtcService};
use aftertone_native::lyrics::LyricsRepository;
use aftertone_native::protocol::extract_code_and_server;
use aftertone_native::server::LocalPartyServer;
use aftertone_native::ui::TerminalUi;

#[derive(Parser)]
#[command(name = "aftertone")]
#[command(about = "AfterTone Desktop Music Player & Listen Together (Native Rust)", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Launch the native Desktop GUI (default when no command is specified)
    Gui {
        #[arg(short, long, default_value_t = 0)]
        port: u16,
        /// Force WebView2 fallback mode instead of native Compose Desktop
        #[arg(long)]
        webview: bool,
    },
    /// Start the built-in local Wi-Fi party server
    Serve {
        #[arg(short, long, default_value_t = 8000)]
        port: u16,
    },
    /// Join an existing AfterTone party
    Join {
        /// 6-character room code or invite link
        code: String,
        /// Member display name
        #[arg(short, long, default_value = "Native Player")]
        name: String,
        /// Custom party server URL
        #[arg(short, long)]
        server: Option<String>,
        /// Open interactive terminal karaoke UI after joining
        #[arg(short, long)]
        tui: bool,
    },
    /// Create a new party room
    Create {
        /// Member display name
        #[arg(short, long, default_value = "Native Host")]
        name: String,
        /// Custom party server URL
        #[arg(short, long)]
        server: Option<String>,
        /// Maximum connected devices (default: 5)
        #[arg(short, long, default_value_t = 5)]
        max: usize,
        /// Open interactive terminal karaoke UI after creating
        #[arg(short, long)]
        tui: bool,
    },
    /// Query and race lyrics across all 5 providers
    Lyrics {
        #[arg(short, long)]
        title: String,
        #[arg(short, long)]
        artist: String,
        #[arg(short, long)]
        video_id: Option<String>,
    },
    /// Resolve 320kbps audio stream URL for a track
    Stream {
        #[arg(short, long)]
        title: String,
        #[arg(short, long)]
        artist: String,
        #[arg(short, long)]
        video_id: Option<String>,
    },
    /// Start loopback HTTP audio range proxy server
    Proxy,
}

async fn run_gui(port: u16, force_webview: bool) -> Result<()> {
    let mut server = LocalPartyServer::new();

    // Connect Discord RPC
    let discord = server.discord();
    tokio::spawn(async move {
        let _ = discord.connect().await;
    });

    let smtc_handle = server.smtc_handle();

    // Initialize Windows SMTC Media Controls immediately so Windows native flyout and keys work
    let (smtc_tx, mut smtc_rx) = tokio::sync::mpsc::unbounded_channel::<SmtcAction>();
    let smtc = SmtcService::new(None, smtc_tx);
    {
        let mut lock = smtc_handle.lock().await;
        *lock = Some(smtc);
    }

    tokio::spawn(async move {
        while let Some(action) = smtc_rx.recv().await {
            println!("[SMTC Hardware Key] Action: {:?}", action);
        }
    });

    let (hwnd_tx, hwnd_rx) = tokio::sync::oneshot::channel::<Option<isize>>();
    let smtc_handle_clone = server.smtc_handle();
    tokio::spawn(async move {
        if let Ok(Some(hwnd)) = hwnd_rx.await {
            // Re-bind to real HWND if WebView window was created
            let (smtc_tx2, _) = tokio::sync::mpsc::unbounded_channel::<SmtcAction>();
            let smtc = SmtcService::new(Some(hwnd), smtc_tx2);
            let mut lock = smtc_handle_clone.lock().await;
            *lock = Some(smtc);
        }
    });

    let base_url = server.start(port).await.map_err(|e| anyhow::anyhow!("{}", e))?;
    println!("🎵 AfterTone Native Engine running at: {}", base_url);

    if !force_webview {
        println!("🚀 Starting 100% Native Compose Desktop GUI (Skiko / DirectX)…");
        if let Err(e) = NativeDesktopApp::run(server.port).await {
            eprintln!("⚠️ Native GUI encountered an issue ({}), falling back to WebView…", e);
            DesktopWindow::run(base_url, move |hwnd| {
                let _ = hwnd_tx.send(hwnd);
            })
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        }
    } else {
        println!("🌐 Starting Desktop Window (WebView2 mode)…");
        DesktopWindow::run(base_url, move |hwnd| {
            let _ = hwnd_tx.send(hwnd);
        })
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    }

    server.stop().await;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        None => run_gui(0, false).await?,
        Some(Commands::Gui { port, webview }) => run_gui(port, webview).await?,

        Some(Commands::Serve { port }) => {
            let mut server = LocalPartyServer::new();
            let base_url = server.start(port).await.map_err(|e| anyhow::anyhow!("{}", e))?;
            println!("🎵 AfterTone Local Party Server running at: {}", base_url);
            println!("   Health endpoint: {}/healthz", base_url);
            println!("   Time endpoint:   {}/api/time", base_url);
            println!("Press Ctrl+C to terminate server.");
            tokio::signal::ctrl_c().await?;
            server.stop().await;
            println!("Server stopped.");
        }

        Some(Commands::Join {
            code,
            name,
            server,
            tui,
        }) => {
            let parsed = extract_code_and_server(&code)
                .ok_or_else(|| anyhow::anyhow!("Invalid party code or invite link"))?;

            let target_server = parsed.server_url.or(server);
            let client = Arc::new(AfterTonePartyClient::new(target_server.as_deref()));

            println!("Connecting to AfterTone party {}…", parsed.code);
            client
                .join_party(&parsed.code, Some(&name), target_server.as_deref())
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            println!("✅ Successfully joined room {}", parsed.code);

            if tui {
                let repo = Arc::new(LyricsRepository::new());
                let ui = TerminalUi::new(client, repo);
                ui.run().await.map_err(|e| anyhow::anyhow!("{}", e))?;
            } else {
                println!("Listening for party events. Press Ctrl+C to leave.");
                tokio::signal::ctrl_c().await?;
                client.leave_party().await;
            }
        }

        Some(Commands::Create {
            name,
            server,
            max,
            tui,
        }) => {
            let client = Arc::new(AfterTonePartyClient::new(server.as_deref()));
            println!("Creating new AfterTone party…");
            let code = client
                .create_party(Some(&name), server.as_deref(), Some(max))
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            println!("🎉 Party created with code: {}", code);
            let s_url = client.server_url.read().await.clone();
            let invite_url = format!("https://aftertone.app/invite/{}?server={}", code, url::form_urlencoded::byte_serialize(s_url.as_bytes()).collect::<String>());
            println!("Share this invite link: {}", invite_url);

            if tui {
                let repo = Arc::new(LyricsRepository::new());
                let ui = TerminalUi::new(client, repo);
                ui.run().await.map_err(|e| anyhow::anyhow!("{}", e))?;
            } else {
                println!("Party is active. Press Ctrl+C to leave.");
                tokio::signal::ctrl_c().await?;
                client.leave_party().await;
            }
        }

        Some(Commands::Lyrics {
            title,
            artist,
            video_id,
        }) => {
            println!("Racing lyrics for \"{}\" by \"{}\"…", title, artist);
            let repo = LyricsRepository::new();
            let (best, all) = repo
                .fetch_lyrics(&title, &artist, None, video_id.as_deref(), None)
                .await;

            if let Some(r) = best {
                println!("\n🏆 Best Result: {} ({} lines, word-synced: {})", r.label, r.lines.len(), r.is_word_synced);
                println!("Discovered providers:");
                for (src, res) in all {
                    println!("  • {:<15} (lines: {}, word-synced: {})", src, res.lines.len(), res.is_word_synced);
                }
                println!("\nPreview of first 5 lines:");
                for line in r.lines.iter().take(5) {
                    if line.is_gap {
                        println!("  [{:>6}ms] (Instrumental gap)", line.time_ms);
                    } else if !line.words.is_empty() {
                        let words_summary: Vec<String> = line
                            .words
                            .iter()
                            .map(|w| format!("{}({}-{}ms)", w.text, w.start_ms, w.end_ms))
                            .collect();
                        println!("  [{:>6}ms] {}", line.time_ms, words_summary.join(" "));
                    } else {
                        println!("  [{:>6}ms] {}", line.time_ms, line.text);
                    }
                }
            } else {
                println!("No lyrics found across any provider.");
            }
        }

        Some(Commands::Stream {
            title,
            artist,
            video_id,
        }) => {
            println!("Resolving audio stream for \"{}\" by \"{}\"…", title, artist);
            let resolver = StreamResolver::new();
            if let Some(url) = resolver
                .resolve_audio_stream(&title, &artist, video_id.as_deref())
                .await
            {
                println!("✅ Resolved Stream URL:\n{}", url);
            } else if let Some(vid) = video_id {
                println!("JioSaavn CDN failed, attempting YouTube extraction via yt-dlp…");
                if let Some(yt_url) = StreamResolver::resolve_youtube_stream_url(&vid).await {
                    println!("✅ Resolved YouTube Stream URL:\n{}", yt_url);
                } else {
                    println!("❌ Could not resolve audio stream.");
                }
            } else {
                println!("❌ Could not resolve audio stream.");
            }
        }

        Some(Commands::Proxy) => {
            let mut proxy = AudioProxyServer::new();
            let base_url = proxy.start().await.map_err(|e| anyhow::anyhow!("{}", e))?;
            println!("🎧 Loopback Audio Range Proxy running at: {}", base_url);
            println!("   Example stream route: {}/api/stream?id=kJQP7kiw5Fk", base_url);
            println!("Press Ctrl+C to stop.");
            tokio::signal::ctrl_c().await?;
            proxy.stop().await;
            println!("Proxy stopped.");
        }
    }

    Ok(())
}
