#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
use anyhow::{Context, Result, ensure};
use clap::Parser;
use orvexa_node::desktop::Controller;
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};
include!(concat!(env!("OUT_DIR"), "/assets.rs"));

#[derive(Parser)]
#[command(
    about = "ORVEXA desktop — transparent testnet, verifying full node",
    version
)]
struct Args {
    #[arg(long)]
    data_dir: Option<PathBuf>,
    #[arg(long)]
    web_dir: Option<PathBuf>,
    #[arg(long, default_value = "127.0.0.1:4252")]
    listen: SocketAddr,
    /// Test/server mode. Normal Windows launch opens the native application.
    #[arg(long)]
    headless: bool,
    /// Explicit local WebView automation only. Never enabled in normal launch.
    #[arg(long)]
    webview_debug_port: Option<u16>,
}
fn default_data() -> Result<PathBuf> {
    #[cfg(windows)]
    {
        Ok(
            PathBuf::from(std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is unavailable")?)
                .join("ORVEXA"),
        )
    }
    #[cfg(not(windows))]
    {
        let base = std::env::var_os("XDG_DATA_HOME")
            .map_or_else(
                || std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")),
                |p| Some(PathBuf::from(p)),
            )
            .context("data directory is unavailable")?;
        Ok(base.join("ORVEXA"))
    }
}
fn assets(data: &Path, explicit: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(path);
    }
    if ASSETS.is_empty() {
        return Ok(PathBuf::from("web/dist"));
    }
    let mut manifest = Vec::new();
    for (name, bytes) in ASSETS {
        manifest.extend(name.as_bytes());
        manifest.extend(orvexa_crypto::hash(b"ORVEXA/desktop-asset/v1", bytes).0);
    }
    let root = data.join(format!(
        "assets-{}",
        orvexa_crypto::hash(b"ORVEXA/desktop-assets/v1", &manifest)
    ));
    std::fs::create_dir_all(&root)?;
    for (name, bytes) in ASSETS {
        ensure!(
            !name.contains("..") && !name.starts_with('/'),
            "invalid embedded asset path"
        );
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().context("asset directory")?)?;
        if std::fs::read(&path).ok().as_deref() != Some(*bytes) {
            std::fs::write(path, bytes)?;
        }
    }
    Ok(root)
}
fn main() {
    if let Err(error) = run() {
        #[cfg(windows)]
        if !std::env::args().any(|a| a == "--headless") {
            native::error(&error.to_string());
        }
        eprintln!("ORVEXA: {error}");
        std::process::exit(1);
    }
}
fn run() -> Result<()> {
    let args = Args::parse();
    ensure!(
        args.listen.ip().is_loopback(),
        "desktop host must be loopback"
    );
    let data = args.data_dir.unwrap_or(default_data()?);
    std::fs::create_dir_all(&data)?;
    let control = Controller::open(&data)?;
    let web = assets(&data, args.web_dir)?;
    let runtime = tokio::runtime::Runtime::new()?;
    let (origin, server) = runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind(args.listen)
            .await
            .context("application port is occupied; use another --listen port")?;
        let origin = format!("http://{}", listener.local_addr()?);
        let app = control.router(&web, &origin)?;
        let server = tokio::spawn(async move { axum::serve(listener, app).await });
        control.resume().await;
        Ok::<_, anyhow::Error>((origin, server))
    })?;
    if args.headless {
        println!("ORVEXA_DESKTOP_ORIGIN={origin}");
        runtime.block_on(async {
            tokio::select! {result=server=>{result??;},result=tokio::signal::ctrl_c()=>{result?;}}
            control.stop().await?;
            Ok::<_, anyhow::Error>(())
        })?;
    } else {
        #[cfg(windows)]
        native::window(&origin, &data, args.webview_debug_port, control, runtime)?;
        #[cfg(not(windows))]
        anyhow::bail!(
            "native GUI is currently packaged for Windows; use --headless or the node CLI on this OS"
        );
    }
    Ok(())
}

#[cfg(windows)]
mod native {
    use super::*;
    use std::sync::Arc;
    use tao::{
        dpi::LogicalSize,
        event::{Event, WindowEvent},
        event_loop::{ControlFlow, EventLoop},
        window::{Theme, WindowBuilder},
    };
    use wry::{WebContext, WebViewBuilder, WebViewBuilderExtWindows};
    pub fn error(message: &str) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
        let body:Vec<u16>=format!("ORVEXA could not start.\n\n{message}\n\nKeep your data. Check the app port, data directory and installed Microsoft WebView2 runtime.").encode_utf16().chain(Some(0)).collect();
        let title: Vec<u16> = "ORVEXA".encode_utf16().chain(Some(0)).collect();
        // Valid NUL-terminated buffers live for the complete synchronous call.
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                body.as_ptr(),
                title.as_ptr(),
                MB_OK | MB_ICONERROR,
            );
        }
    }
    pub fn window(
        origin: &str,
        data: &Path,
        debug: Option<u16>,
        control: Arc<Controller>,
        runtime: tokio::runtime::Runtime,
    ) -> Result<()> {
        let event_loop = EventLoop::new();
        let window = WindowBuilder::new()
            .with_title("ORVEXA")
            .with_theme(Some(Theme::Dark))
            .with_inner_size(LogicalSize::new(1360, 900))
            .with_min_inner_size(LogicalSize::new(420, 640))
            .build(&event_loop)?;
        let mut context = WebContext::new(Some(data.join("webview")));
        let allowed = format!("{origin}/");
        let mut builder = WebViewBuilder::new_with_web_context(&mut context)
            .with_url(format!("{origin}/#/node"))
            .with_devtools(false)
            .with_navigation_handler(move |url| url.starts_with(&allowed));
        if let Some(port) = debug {
            ensure!(port >= 1024, "invalid debug port");
            builder = builder.with_additional_browser_args(format!(
                "--remote-debugging-port={port} --remote-debugging-address=127.0.0.1"
            ));
        }
        let _view = builder
            .build(&window)
            .context("create native window (Windows WebView2 runtime required)")?;
        event_loop.run(move |event, _, flow| {
            // Keep native view, profile and Tokio runtime alive for this window.
            let _ = (&_view, &context);
            *flow = ControlFlow::Wait;
            if matches!(
                event,
                Event::WindowEvent {
                    event: WindowEvent::CloseRequested,
                    ..
                }
            ) {
                let _ = runtime.block_on(control.stop());
                *flow = ControlFlow::Exit;
            }
        });
    }
}
