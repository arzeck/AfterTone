//! Interactive Terminal Karaoke Player & Controller
//! Renders real-time word-by-word synced karaoke lyrics directly in the terminal with live syllable glow.

use crossterm::{
    cursor,
    event::{self, Event, KeyCode},
    execute,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{self, Clear, ClearType},
};
use std::io::{stdout, Write};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use crate::client::{AfterTonePartyClient, ClientEvent};
use crate::lyrics::{LyricAlignment, LyricResult, LyricsRepository};

fn format_duration(ms: i64) -> String {
    if ms <= 0 {
        return "00:00".to_string();
    }
    let total_sec = ms / 1000;
    let min = total_sec / 60;
    let sec = total_sec % 60;
    format!("{:02}:{:02}", min, sec)
}

pub struct TerminalUi {
    client: Arc<AfterTonePartyClient>,
    lyrics_repo: Arc<LyricsRepository>,
    current_lyrics: Arc<RwLock<Option<LyricResult>>>,
}

impl TerminalUi {
    pub fn new(client: Arc<AfterTonePartyClient>, lyrics_repo: Arc<LyricsRepository>) -> Self {
        Self {
            client,
            lyrics_repo,
            current_lyrics: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        terminal::enable_raw_mode()?;
        let mut out = stdout();
        execute!(out, terminal::EnterAlternateScreen, cursor::Hide)?;

        let mut event_rx = self.client.subscribe();
        let repo_clone = Arc::clone(&self.lyrics_repo);
        let lyrics_ref = Arc::clone(&self.current_lyrics);

        // Track change listener to fetch lyrics
        tokio::spawn(async move {
            while let Ok(evt) = event_rx.recv().await {
                if let ClientEvent::TrackChanged(Some(track)) = evt {
                    *lyrics_ref.write().await = None;
                    let (best, _) = repo_clone
                        .fetch_lyrics(
                            &track.title,
                            &track.artist,
                            track.duration_ms,
                            Some(&track.video_id),
                            None,
                        )
                        .await;
                    *lyrics_ref.write().await = best;
                }
            }
        });

        let mut ticker = tokio::time::interval(Duration::from_millis(50));
        let mut running = true;

        while running {
            tokio::select! {
                _ = ticker.tick() => {
                    self.render(&mut out).await?;
                }
                _ = async {
                    if event::poll(Duration::from_millis(10)).unwrap_or(false) {
                        if let Ok(Event::Key(key)) = event::read() {
                            match key.code {
                                KeyCode::Char('q') | KeyCode::Esc => {
                                    running = false;
                                }
                                KeyCode::Char(' ') => {
                                    self.client.toggle_play_pause().await;
                                }
                                KeyCode::Char('n') => {
                                    self.client.next().await;
                                }
                                KeyCode::Char('p') => {
                                    self.client.previous().await;
                                }
                                KeyCode::Right => {
                                    let cur = self.client.get_current_playhead().await;
                                    self.client.seek(cur + 10_000).await;
                                }
                                KeyCode::Left => {
                                    let cur = self.client.get_current_playhead().await;
                                    self.client.seek(cur - 10_000).await;
                                }
                                _ => {}
                            }
                        }
                    }
                } => {}
            }
        }

        execute!(out, cursor::Show, terminal::LeaveAlternateScreen)?;
        terminal::disable_raw_mode()?;
        Ok(())
    }

    async fn render(&self, out: &mut std::io::Stdout) -> Result<(), Box<dyn std::error::Error>> {
        let (_cols, rows) = terminal::size().unwrap_or((80, 24));
        execute!(out, cursor::MoveTo(0, 0), Clear(ClearType::All))?;

        let code_guard = self.client.code.read().await;
        let room_code = code_guard.as_deref().unwrap_or("------");
        let rtt = self.client.clock.read().await.round_trip_ms;

        let playback = self.client.playback.read().await.clone();
        let playhead = self.client.get_current_playhead().await;
        let duration = playback.track.as_ref().and_then(|t| t.duration_ms).unwrap_or(0);

        // Header
        execute!(
            out,
            SetForegroundColor(Color::Cyan),
            Print("╔══════════════════════════════════════════════════════════════════════════════╗\n"),
            Print(format!("║  🎵 AfterTone Listen Together (Native Rust)     Room: {:<6}  Ping: {:>3}ms  ║\n", room_code, rtt)),
            Print("╚══════════════════════════════════════════════════════════════════════════════╝\n"),
            ResetColor
        )?;

        // Track info
        if let Some(ref track) = playback.track {
            let icon = if playback.is_playing { "▶ " } else { "⏸ " };
            let progress_str = format!("{} / {}", format_duration(playhead), format_duration(duration));

            execute!(
                out,
                SetForegroundColor(Color::Yellow),
                Print(format!("  {}Now Playing: ", icon)),
                SetForegroundColor(Color::White),
                Print(format!("{} — {}\n", track.title, track.artist)),
                SetForegroundColor(Color::DarkGrey),
                Print(format!("  Time: {}\n\n", progress_str)),
                ResetColor
            )?;
        } else {
            execute!(
                out,
                SetForegroundColor(Color::DarkGrey),
                Print("  No track currently playing in the party.\n\n"),
                ResetColor
            )?;
        }

        // Lyrics Area
        let lyrics_guard = self.current_lyrics.read().await;
        if let Some(ref lyric_res) = *lyrics_guard {
            execute!(
                out,
                SetForegroundColor(Color::Magenta),
                Print(format!("  ─── Lyrics ({}) ───────────────────────────────────────\n", lyric_res.label)),
                ResetColor
            )?;

            let lines = &lyric_res.lines;
            let cur_idx = lines
                .iter()
                .rposition(|l| playhead >= l.time_ms)
                .unwrap_or(0);

            let start_idx = cur_idx.saturating_sub(2);
            let end_idx = (cur_idx + 4).min(lines.len());

            for (i, line) in lines[start_idx..end_idx].iter().enumerate() {
                let actual_idx = start_idx + i;
                let is_current = actual_idx == cur_idx;

                if line.is_gap {
                    if is_current {
                        execute!(
                            out,
                            SetForegroundColor(Color::Yellow),
                            Print("      ●   ●   ●   (Instrumental)\n"),
                            ResetColor
                        )?;
                    }
                    continue;
                }

                let align_prefix = match line.alignment {
                    LyricAlignment::Start => "  ",
                    LyricAlignment::End => "        [Duet] ",
                };

                if is_current {
                    if line.words.is_empty() {
                        execute!(
                            out,
                            SetForegroundColor(Color::Green),
                            Print(format!("  ➜ {}{}\n", align_prefix, line.text)),
                            ResetColor
                        )?;
                    } else {
                        // Word-by-word karaoke sweep
                        execute!(out, SetForegroundColor(Color::Green), Print("  ➜ "), ResetColor)?;
                        for word in &line.words {
                            if playhead >= word.end_ms {
                                execute!(out, SetForegroundColor(Color::White), Print(format!("{} ", word.text)))?;
                            } else if playhead >= word.start_ms {
                                execute!(out, SetForegroundColor(Color::Cyan), Print(format!("★{}★ ", word.text)))?;
                            } else {
                                execute!(out, SetForegroundColor(Color::DarkGrey), Print(format!("{} ", word.text)))?;
                            }
                        }
                        execute!(out, Print("\n"), ResetColor)?;
                    }

                    if let Some(ref bg) = line.background {
                        execute!(
                            out,
                            SetForegroundColor(Color::DarkCyan),
                            Print(format!("       (answering: {})\n", bg.text)),
                            ResetColor
                        )?;
                    }
                } else {
                    execute!(
                        out,
                        SetForegroundColor(Color::DarkGrey),
                        Print(format!("    {}{}\n", align_prefix, line.text)),
                        ResetColor
                    )?;
                }
            }
        } else {
            execute!(
                out,
                SetForegroundColor(Color::DarkGrey),
                Print("  Fetching real-time synchronized lyrics across 5 providers…\n"),
                ResetColor
            )?;
        }

        // Footer shortcuts
        execute!(
            out,
            cursor::MoveTo(0, rows.saturating_sub(2)),
            SetForegroundColor(Color::DarkGrey),
            Print("────────────────────────────────────────────────────────────────────────────────\n"),
            SetForegroundColor(Color::Cyan),
            Print(" [Space] Play/Pause   [Left/Right] ±10s   [N] Next   [P] Prev   [Q] Quit\n"),
            ResetColor
        )?;

        out.flush()?;
        Ok(())
    }
}
