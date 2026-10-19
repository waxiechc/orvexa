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
    /// Test/server mode. Windows/macOS launch opens the native application.
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
    #[cfg(target_os = "macos")]
    {
        Ok(
            PathBuf::from(std::env::var_os("HOME").context("home directory is unavailable")?)
                .join("Library/Application Support/ORVEXA"),
        )
    }
    #[cfg(not(any(windows, target_os = "macos")))]
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
        #[cfg(any(windows, target_os = "macos"))]
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
    // Per-launch secret for the private v2 wallet API. Only the app window
    // receives it; any other local process lacks it. Tests may supply one.
    let session = match std::env::var("ORVEXA_V2_SESSION") {
        Ok(token) if token.len() >= 32 => token,
        _ => orvexa_crypto::Hash32(orvexa_crypto::random_bytes()?).to_string(),
    };
    let web = assets(&data, args.web_dir)?;
    // The embedded node is I/O driven. Do not allocate a worker per CPU core
    // on high-core-count Macs/PCs while the app is idle.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(4)
        .enable_all()
        .build()?;
    let (origin, server) = runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind(args.listen)
            .await
            .context("application port is occupied; use another --listen port")?;
        let origin = format!("http://{}", listener.local_addr()?);
        // Public networks are listed in a file signed with the release key
        // this build already trusts for updates; fetched only on request.
        let directory = orvexa_shielded::desktop::DirectorySource {
            url: std::env::var("ORVEXA_NETWORK_DIRECTORY_URL")
                .unwrap_or_else(|_| orvexa_shielded::directory::DEFAULT_URL.to_owned()),
            trusted: orvexa_update::trusted_keys()
                .iter()
                .map(|k| k.to_bytes())
                .collect(),
        };
        let private = orvexa_shielded::desktop::DesktopV2::open_with_directory(
            &data.join("v2"),
            session.clone(),
            directory,
        )
        .await
        .map_err(anyhow::Error::msg)?;
        let updates = orvexa_update::Updater::new(&data, session.clone());
        updates.start();
        let app = control.router_with(&web, &origin, private.router().merge(updates.router()))?;
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
        #[cfg(any(windows, target_os = "macos"))]
        native::window(
            &origin,
            &data,
            args.webview_debug_port,
            &session,
            control,
            runtime,
        )?;
        #[cfg(not(any(windows, target_os = "macos")))]
        anyhow::bail!(
            "native GUI is currently packaged for Windows; use --headless or the node CLI on this OS"
        );
    }
    Ok(())
}

#[cfg(any(windows, target_os = "macos"))]
mod native {
    use super::*;
    use std::sync::Arc;
    #[cfg(windows)]
    use tao::event_loop::EventLoop;
    use tao::{
        dpi::LogicalSize,
        event::{Event, WindowEvent},
        event_loop::ControlFlow,
        window::{Theme, WindowBuilder},
    };
    #[cfg(windows)]
    use wry::WebViewBuilderExtWindows;
    use wry::{WebContext, WebViewBuilder};
    #[cfg(windows)]
    pub fn error(message: &str) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
        let body:Vec<u16>=format!("Не удалось запустить ORVEXA.\n\n{message}\n\nСохраните данные. Проверьте порт, каталог данных и наличие Microsoft WebView2 runtime.").encode_utf16().chain(Some(0)).collect();
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
    #[cfg(target_os = "macos")]
    pub fn error(message: &str) {
        let _ = std::process::Command::new("/usr/bin/osascript")
            .args(["-e", "on run argv\ndisplay alert \"ORVEXA\" message (item 1 of argv) as critical\nend run", &format!("Не удалось запустить ORVEXA. Сохраните данные.\n\n{message}")])
            .status();
    }
    #[cfg(target_os = "macos")]
    enum AppEvent {
        Quit,
    }
    #[cfg(target_os = "macos")]
    fn mac_menu(proxy: tao::event_loop::EventLoopProxy<AppEvent>) -> Result<muda::Menu> {
        use muda::{
            Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu,
            accelerator::{Accelerator, Code, Modifiers},
        };
        let menu = Menu::new();
        let quit = MenuItem::with_id(
            "quit",
            "Завершить ORVEXA",
            true,
            Some(Accelerator::new(Some(Modifiers::SUPER), Code::KeyQ)),
        );
        let app = Submenu::with_items(
            "ORVEXA",
            true,
            &[
                &PredefinedMenuItem::hide(Some("Скрыть ORVEXA")),
                &PredefinedMenuItem::hide_others(Some("Скрыть остальные")),
                &PredefinedMenuItem::show_all(Some("Показать все")),
                &PredefinedMenuItem::separator(),
                &quit,
            ],
        )?;
        let edit = Submenu::with_items(
            "Правка",
            true,
            &[
                &PredefinedMenuItem::undo(Some("Отменить")),
                &PredefinedMenuItem::redo(Some("Повторить")),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::cut(Some("Вырезать")),
                &PredefinedMenuItem::copy(Some("Копировать")),
                &PredefinedMenuItem::paste(Some("Вставить")),
                &PredefinedMenuItem::select_all(Some("Выбрать всё")),
            ],
        )?;
        menu.append_items(&[&app, &edit])?;
        menu.init_for_nsapp();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if event.id.as_ref() == "quit" {
                let _ = proxy.send_event(AppEvent::Quit);
            }
        }));
        Ok(menu)
    }
    pub fn window(
        origin: &str,
        data: &Path,
        debug: Option<u16>,
        session: &str,
        control: Arc<Controller>,
        runtime: tokio::runtime::Runtime,
    ) -> Result<()> {
        #[cfg(windows)]
        let event_loop = EventLoop::new();
        #[cfg(target_os = "macos")]
        let event_loop = {
            let version = std::process::Command::new("/usr/bin/sw_vers")
                .arg("-productVersion")
                .output()?;
            let major = String::from_utf8(version.stdout)?
                .split('.')
                .next()
                .unwrap_or("")
                .parse::<u32>()?;
            ensure!(
                major >= 14,
                "ORVEXA native GUI requires macOS 14 or newer for isolated persistent wallet storage"
            );
            tao::event_loop::EventLoopBuilder::<AppEvent>::with_user_event().build()
        };
        #[cfg(target_os = "macos")]
        let menu = mac_menu(event_loop.create_proxy())?;
        let window = WindowBuilder::new()
            .with_title("ORVEXA")
            .with_theme(Some(Theme::Dark))
            .with_inner_size(LogicalSize::new(1360, 900))
            .with_min_inner_size(LogicalSize::new(420, 640))
            .build(&event_loop)?;
        let mut context = WebContext::new(Some(data.join("webview")));
        let allowed = format!("{origin}/");
        let allowed_blob = format!("blob:{origin}/");
        let mut builder = WebViewBuilder::new_with_web_context(&mut context)
            .with_url(format!("{origin}/#/node"))
            .with_background_color((5, 6, 8, 255))
            .with_devtools(false)
            .with_initialization_script(format!(
                "Object.defineProperty(window,'__ORVEXA_V2_SESSION',{{value:{},writable:false}});",
                serde_json::to_string(session)?
            ))
            .with_navigation_handler(move |url| {
                url.starts_with(&allowed) || url.starts_with(&allowed_blob)
            });
        #[cfg(windows)]
        if let Some(port) = debug {
            ensure!(port >= 1024, "invalid debug port");
            builder = builder.with_additional_browser_args(format!(
                "--remote-debugging-port={port} --remote-debugging-address=127.0.0.1"
            ));
        }
        #[cfg(target_os = "macos")]
        {
            use wry::WebViewBuilderExtDarwin;
            ensure!(
                debug.is_none(),
                "remote debug port is only supported on Windows"
            );
            let profile = data.canonicalize()?;
            let hash = orvexa_crypto::hash(
                b"ORVEXA/mac-web-profile/v1",
                profile.to_string_lossy().as_bytes(),
            );
            let mut identifier = [0; 16];
            identifier.copy_from_slice(&hash.0[..16]);
            let download_origin = format!("blob:{origin}/");
            let download_http_origin = format!("{origin}/");
            builder = builder
                .with_data_store_identifier(identifier)
                .with_allow_link_preview(false)
                // WKWebView downloads require an explicit upstream handler.
                // The system Downloads directory and collision handling come
                // from Wry; never send backup bytes through a wallet RPC.
                .with_download_started_handler(move |url, _| {
                    url.starts_with(&download_origin) || url.starts_with(&download_http_origin)
                });
        }
        let _view = builder
            .build(&window)
            .context("create native window (Windows: WebView2, macOS: system WebKit)")?;
        event_loop.run(move |event, _, flow| {
            // Keep native view, profile and Tokio runtime alive for this window.
            let _ = (&_view, &context);
            #[cfg(target_os = "macos")]
            let _ = &menu;
            *flow = ControlFlow::Wait;
            let close = matches!(
                &event,
                Event::WindowEvent {
                    event: WindowEvent::CloseRequested,
                    ..
                }
            );
            #[cfg(target_os = "macos")]
            let close = close || matches!(&event, Event::UserEvent(AppEvent::Quit));
            if close {
                let _ = runtime.block_on(control.stop());
                *flow = ControlFlow::Exit;
            }
        });
    }
}
