//! System Media Transport Controls (SMTC) Integration
//! Supports Windows media keys (Play/Pause, Next, Previous) and taskbar media overlay.

use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, PlatformConfig};
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub enum SmtcAction {
    Play,
    Pause,
    TogglePlayPause,
    Next,
    Previous,
}

#[cfg(target_os = "windows")]
mod dummy_win {
    use std::mem;
    use std::ptr;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetAncestor,
        IsDialogMessageW, PeekMessageW, RegisterClassExW, TranslateMessage, GA_ROOT, MSG,
        PM_REMOVE, WM_QUIT, WNDCLASSEXW,
    };

    pub struct DummyWindow {
        pub handle: HWND,
    }

    unsafe impl Send for DummyWindow {}
    unsafe impl Sync for DummyWindow {}

    impl DummyWindow {
        pub fn new() -> Result<Self, String> {
            let class_name: Vec<u16> = "AfterToneSmtcMessageWindow\0".encode_utf16().collect();

            let handle = unsafe {
                let instance = GetModuleHandleW(ptr::null());

                let mut wnd_class: WNDCLASSEXW = mem::zeroed();
                wnd_class.cbSize = mem::size_of::<WNDCLASSEXW>() as u32;
                wnd_class.hInstance = instance;
                wnd_class.lpszClassName = class_name.as_ptr();
                wnd_class.lpfnWndProc = Some(Self::wnd_proc);

                let _ = RegisterClassExW(&wnd_class);

                CreateWindowExW(
                    0,
                    class_name.as_ptr(),
                    ptr::null(),
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    instance,
                    ptr::null(),
                )
            };

            if handle == 0 {
                return Err("CreateWindowExW returned NULL".to_string());
            }

            Ok(DummyWindow { handle })
        }

        extern "system" fn wnd_proc(
            hwnd: HWND,
            msg: u32,
            wparam: WPARAM,
            lparam: LPARAM,
        ) -> LRESULT {
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
    }

    impl Drop for DummyWindow {
        fn drop(&mut self) {
            unsafe {
                let _ = DestroyWindow(self.handle);
            }
        }
    }

    pub fn start_message_pump() {
        std::thread::spawn(|| {
            unsafe {
                let mut msg: MSG = std::mem::zeroed();
                loop {
                    let mut has_message = PeekMessageW(&mut msg, 0, 0, 0, PM_REMOVE) != 0;
                    while msg.message != WM_QUIT && has_message {
                        if IsDialogMessageW(GetAncestor(msg.hwnd, GA_ROOT), &msg) == 0 {
                            let _ = TranslateMessage(&msg);
                            DispatchMessageW(&msg);
                        }
                        has_message = PeekMessageW(&mut msg, 0, 0, 0, PM_REMOVE) != 0;
                    }
                    if msg.message == WM_QUIT {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
        });
    }
}

pub struct SmtcService {
    _controls: Option<Arc<std::sync::Mutex<MediaControls>>>,
    #[cfg(target_os = "windows")]
    _dummy_window: Option<Arc<dummy_win::DummyWindow>>,
}

impl SmtcService {
    pub fn new(hwnd: Option<isize>, action_tx: mpsc::UnboundedSender<SmtcAction>) -> Self {
        #[cfg(target_os = "windows")]
        let (hwnd_ptr, dummy_win_opt) = {
            if let Some(h) = hwnd {
                if h != 0 {
                    (Some(h as *mut std::ffi::c_void), None)
                } else {
                    match dummy_win::DummyWindow::new() {
                        Ok(dw) => {
                            let raw_hwnd = dw.handle as *mut std::ffi::c_void;
                            dummy_win::start_message_pump();
                            (Some(raw_hwnd), Some(Arc::new(dw)))
                        }
                        Err(e) => {
                            eprintln!("⚠️ [SMTC] Failed to create dummy message window: {}", e);
                            (None, None)
                        }
                    }
                }
            } else {
                match dummy_win::DummyWindow::new() {
                    Ok(dw) => {
                        let raw_hwnd = dw.handle as *mut std::ffi::c_void;
                        dummy_win::start_message_pump();
                        (Some(raw_hwnd), Some(Arc::new(dw)))
                    }
                    Err(e) => {
                        eprintln!("⚠️ [SMTC] Failed to create dummy message window: {}", e);
                        (None, None)
                    }
                }
            }
        };

        #[cfg(not(target_os = "windows"))]
        let (hwnd_ptr, dummy_win_opt) = (None, None);

        let config = PlatformConfig {
            dbus_name: "aftertone",
            display_name: "AfterTone",
            hwnd: hwnd_ptr,
        };

        match MediaControls::new(config) {
            Ok(mut controls) => {
                println!("✅ [SMTC] Windows System Media Transport Controls initialized successfully!");
                controls
                    .attach(move |event: MediaControlEvent| match event {
                        MediaControlEvent::Play => {
                            let _ = action_tx.send(SmtcAction::Play);
                        }
                        MediaControlEvent::Pause => {
                            let _ = action_tx.send(SmtcAction::Pause);
                        }
                        MediaControlEvent::Toggle => {
                            let _ = action_tx.send(SmtcAction::TogglePlayPause);
                        }
                        MediaControlEvent::Next => {
                            let _ = action_tx.send(SmtcAction::Next);
                        }
                        MediaControlEvent::Previous => {
                            let _ = action_tx.send(SmtcAction::Previous);
                        }
                        _ => {}
                    })
                    .ok();

                Self {
                    _controls: Some(Arc::new(std::sync::Mutex::new(controls))),
                    #[cfg(target_os = "windows")]
                    _dummy_window: dummy_win_opt,
                }
            }
            Err(e) => {
                eprintln!("⚠️ [SMTC] Failed to initialize Windows MediaControls: {:?}", e);
                Self {
                    _controls: None,
                    #[cfg(target_os = "windows")]
                    _dummy_window: None,
                }
            }
        }
    }

    pub fn update_metadata(&self, title: &str, artist: &str, album: Option<&str>, duration: Option<std::time::Duration>) {
        if let Some(ref controls) = self._controls {
            if let Ok(mut c) = controls.lock() {
                let _ = c.set_metadata(MediaMetadata {
                    title: Some(title),
                    artist: Some(artist),
                    album,
                    duration,
                    cover_url: None,
                });
            }
        }
    }

    pub fn set_playback(&self, is_playing: bool) {
        if let Some(ref controls) = self._controls {
            if let Ok(mut c) = controls.lock() {
                let playback = if is_playing {
                    MediaPlayback::Playing { progress: None }
                } else {
                    MediaPlayback::Paused { progress: None }
                };
                let _ = c.set_playback(playback);
            }
        }
    }
}
