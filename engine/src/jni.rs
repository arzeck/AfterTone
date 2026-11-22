//! Android JNI Bindings for AfterTone Native Engine
//!
//! Exposes the high-performance Rust core (Listen Together party server,
//! 320kbps pure DES decryption, and TTML lyrics parsing) to the Android Kotlin runtime.

use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jint, jstring};
use jni::JNIEnv;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::Mutex;
use std::thread;

static SERVER_RUNNING: AtomicBool = AtomicBool::new(false);
static BOUND_PORT: AtomicU16 = AtomicU16::new(0);
static SHUTDOWN_TX: Mutex<Option<tokio::sync::oneshot::Sender<()>>> = Mutex::new(None);

#[no_mangle]
pub extern "system" fn Java_com_arzeck_aftertone_engine_AfterToneNative_startServer(
    _env: JNIEnv,
    _class: JClass,
    port: jint,
) -> jint {
    if SERVER_RUNNING.load(Ordering::SeqCst) {
        return BOUND_PORT.load(Ordering::SeqCst) as jint;
    }

    let target_port = if port > 0 { port as u16 } else { 7070 };
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    {
        let mut guard = SHUTDOWN_TX.lock().unwrap();
        *guard = Some(shutdown_tx);
    }

    thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(_) => {
                let _ = ready_tx.send(Err(()));
                return;
            }
        };

        rt.block_on(async move {
            let mut server = crate::server::LocalPartyServer::new();
            match server.start(target_port).await {
                Ok(_url) => {
                    let actual_port = target_port;
                    BOUND_PORT.store(actual_port, Ordering::SeqCst);
                    SERVER_RUNNING.store(true, Ordering::SeqCst);
                    let _ = ready_tx.send(Ok(actual_port));

                    // Wait for shutdown signal
                    tokio::select! {
                        _ = &mut shutdown_rx => {
                            server.stop().await;
                        }
                    }
                    SERVER_RUNNING.store(false, Ordering::SeqCst);
                    BOUND_PORT.store(0, Ordering::SeqCst);
                }
                Err(_) => {
                    let _ = ready_tx.send(Err(()));
                }
            }
        });
    });

    match ready_rx.recv() {
        Ok(Ok(p)) => p as jint,
        _ => -1,
    }
}

#[no_mangle]
pub extern "system" fn Java_com_arzeck_aftertone_engine_AfterToneNative_isServerRunning(
    _env: JNIEnv,
    _class: JClass,
) -> jboolean {
    if SERVER_RUNNING.load(Ordering::SeqCst) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "system" fn Java_com_arzeck_aftertone_engine_AfterToneNative_stopServer(
    _env: JNIEnv,
    _class: JClass,
) {
    let mut guard = SHUTDOWN_TX.lock().unwrap();
    if let Some(tx) = guard.take() {
        let _ = tx.send(());
    }
    SERVER_RUNNING.store(false, Ordering::SeqCst);
    BOUND_PORT.store(0, Ordering::SeqCst);
}

#[no_mangle]
pub extern "system" fn Java_com_arzeck_aftertone_engine_AfterToneNative_decryptSaavn(
    mut env: JNIEnv,
    _class: JClass,
    url_input: JString,
) -> jstring {
    let input: String = match env.get_string(&url_input) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };

    let decrypted = crate::audio::des_cipher::decrypt_saavn_media_url(&input);
    match env.new_string(decrypted) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "system" fn Java_com_arzeck_aftertone_engine_AfterToneNative_parseTtml(
    mut env: JNIEnv,
    _class: JClass,
    ttml_input: JString,
) -> jstring {
    let input: String = match env.get_string(&ttml_input) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };

    let lines = crate::lyrics::ttml_parser::parse_ttml_lyrics(&input);
    let json = serde_json::to_string(&lines).unwrap_or_else(|_| "[]".to_string());
    match env.new_string(json) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "system" fn Java_com_arzeck_aftertone_engine_AfterToneNative_getVersion(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    let ver = crate::version();
    match env.new_string(ver) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}
