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
                let javaw = parent.join("runtime").join("bin").join("javaw.exe");
                if javaw.exists() {
                    return Ok(javaw);
                }
                let java = parent.join("runtime").join("bin").join("java.exe");
                if java.exists() {
                    return Ok(java);
                }
            }
        }

        // 2. Check current working directory: ./runtime/bin/javaw.exe or java.exe
        let local_javaw = PathBuf::from("runtime").join("bin").join("javaw.exe");
        if local_javaw.exists() {
            return Ok(local_javaw);
        }
        let local_candidate = PathBuf::from("runtime").join("bin").join("java.exe");
        if local_candidate.exists() {
            return Ok(local_candidate);
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
        // 1. Check relative to current exe: <exe_dir>/app
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                let candidate = parent.join("app");
                if candidate.exists() && candidate.join("skiko-windows-x64.dll").exists() {
                    return Ok(candidate);
                }
            }
        }

        // 2. Check current working directory: ./app
        let local_candidate = PathBuf::from("app");
        if local_candidate.exists() && local_candidate.join("skiko-windows-x64.dll").exists() {
            return Ok(local_candidate);
        }

        Err(anyhow!("Could not locate AfterTone desktop app directory with Skiko libraries (checked <exe_dir>/app and ./app)"))
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
