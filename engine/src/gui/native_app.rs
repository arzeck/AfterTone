//! AfterTone Native Desktop GUI Application Runner
//! Launches the native Skiko / DirectX Compose Desktop application
//! paired with the AfterTone Native Rust backend engine.

use anyhow::{anyhow, Result};
use std::path::PathBuf;
use tokio::process::Command;

pub struct NativeDesktopApp;

impl NativeDesktopApp {
    /// Resolves the path to the bundled or system Java 21 runtime.
    pub fn find_java_binary() -> Result<PathBuf> {
        // 1. Check relative to current exe: <exe_dir>/runtime/bin/javaw.exe (Windowed GUI) or java.exe
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                let candidates = [
                    parent.join("runtime").join("bin").join("javaw.exe"),
                    parent.join("runtime").join("bin").join("java.exe"),
                    parent.join("windows").join("runtime").join("bin").join("javaw.exe"),
                    parent.join("windows").join("runtime").join("bin").join("java.exe"),
                    parent.join("..").join("windows").join("runtime").join("bin").join("javaw.exe"),
                    parent.join("..").join("windows").join("runtime").join("bin").join("java.exe"),
                ];
                for cand in &candidates {
                    if cand.exists() {
                        return Ok(cand.clone());
                    }
                }
            }
        }

        // 2. Check current working directory: ./windows/runtime, ../windows/runtime, or ./runtime
        let local_candidates = [
            PathBuf::from("windows").join("runtime").join("bin").join("javaw.exe"),
            PathBuf::from("windows").join("runtime").join("bin").join("java.exe"),
            PathBuf::from("..").join("windows").join("runtime").join("bin").join("javaw.exe"),
            PathBuf::from("..").join("windows").join("runtime").join("bin").join("java.exe"),
            PathBuf::from("runtime").join("bin").join("javaw.exe"),
            PathBuf::from("runtime").join("bin").join("java.exe"),
        ];
        for cand in &local_candidates {
            if cand.exists() {
                return Ok(cand.clone());
            }
        }

        // 3. Check JAVA_HOME
        if let Ok(java_home) = std::env::var("JAVA_HOME") {
            let javaw = PathBuf::from(&java_home).join("bin").join("javaw.exe");
            if javaw.exists() {
                return Ok(javaw);
            }
            let candidate = PathBuf::from(java_home).join("bin").join("java.exe");
            if candidate.exists() {
                return Ok(candidate);
            }
        }

        // 4. Fallback to "javaw" on system PATH
        Ok(PathBuf::from("javaw"))
    }

    /// Resolves the path to the bundled app directory containing application JARs and Skiko natives.
    pub fn find_app_dir() -> Result<PathBuf> {
        // 1. Check relative to current exe: <exe_dir>/app, <exe_dir>/windows/app, or ../windows/app
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                let candidates = [
                    parent.join("app"),
                    parent.join("windows").join("app"),
                    parent.join("..").join("windows").join("app"),
                ];
                for cand in &candidates {
                    if cand.exists() && cand.join("skiko-windows-x64.dll").exists() {
                        return Ok(cand.clone());
                    }
                }
            }
        }

        // 2. Check current working directory: ./windows/app, ../windows/app, or ./app
        let local_candidates = [
            PathBuf::from("windows").join("app"),
            PathBuf::from("..").join("windows").join("app"),
            PathBuf::from("app"),
        ];
        for cand in &local_candidates {
            if cand.exists() && cand.join("skiko-windows-x64.dll").exists() {
                return Ok(cand.clone());
            }
        }

        Err(anyhow!("Could not locate AfterTone desktop app directory with Skiko libraries (checked <exe_dir>/app, windows/app, and ./app)"))
    }

    /// Launches and runs the native Compose Desktop GUI.
    pub async fn run(backend_port: u16) -> Result<()> {
        let java_bin = Self::find_java_binary()?;
        let app_dir = Self::find_app_dir()?;

        println!("🚀 Launching AfterTone Native Desktop GUI (Skiko / Compose Desktop)…");
        println!("   Runtime: {}", java_bin.display());
        println!("   App Directory: {}", app_dir.display());

        let classpath = format!("{}\\*", app_dir.to_string_lossy());
        let skiko_path = format!("-Dskiko.library.path={}", app_dir.to_string_lossy());
        let working_dir = app_dir.parent().unwrap_or(&app_dir);

        let mut cmd = Command::new(&java_bin);
        cmd.current_dir(working_dir)
            .arg("-Daftertone.version=1.0")
            .arg(&skiko_path)
            .arg("-Djpackage.app-version=1.8.30")
            .arg("-Dcompose.application.configure.swing.globals=true")
            .arg(format!("-Daftertone.backend.port={}", backend_port))
            .arg("-Xmx512m")
            .arg("-XX:+UseG1GC")
            .arg("-XX:G1PeriodicGCInterval=20000")
            .arg("-XX:G1PeriodicGCSystemLoadThreshold=0")
            .arg("-cp")
            .arg(&classpath)
            .arg("com.music.bitchord.desktop.MainKt")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());

        #[cfg(target_os = "windows")]
        {
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| anyhow!("Failed to spawn native AfterTone GUI: {}", e))?;

        let status = child.wait().await?;
        if !status.success() {
            eprintln!("AfterTone GUI exited with status: {}", status);
        }

        Ok(())
    }
}
