//! AfterTone Native Desktop GUI Window
//! Embeds Microsoft Edge WebView2 Evergreen inside a native Tao desktop window.

use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop};
use tao::window::WindowBuilder;
use wry::WebViewBuilder;

pub struct DesktopWindow;

impl DesktopWindow {
    pub fn run<F>(server_url: String, on_hwnd: F) -> Result<(), Box<dyn std::error::Error>>
    where
        F: FnOnce(Option<isize>),
    {
        let event_loop = EventLoop::new();

        let window = WindowBuilder::new()
            .with_title("AfterTone")
            .with_inner_size(LogicalSize::new(1280.0, 800.0))
            .with_min_inner_size(LogicalSize::new(960.0, 600.0))
            .build(&event_loop)?;

        #[cfg(target_os = "windows")]
        {
            use tao::platform::windows::WindowExtWindows;
            on_hwnd(Some(window.hwnd() as isize));
        }
        #[cfg(not(target_os = "windows"))]
        {
            on_hwnd(None);
        }

        let devtools = cfg!(debug_assertions);

        let _webview = WebViewBuilder::new()
            .with_url(&server_url)
            .with_devtools(devtools)
            .with_background_color((12, 13, 18, 255))
            .build(&window)?;

        event_loop.run(move |event, _, control_flow| {
            *control_flow = ControlFlow::Wait;

            match event {
                Event::WindowEvent {
                    event: WindowEvent::CloseRequested,
                    ..
                } => {
                    *control_flow = ControlFlow::Exit;
                }
                _ => {}
            }
        });
    }
}

