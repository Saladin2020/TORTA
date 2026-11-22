//! Tray command types + legacy non-Windows tray loop.

use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub enum TrayCmd {
    SyncNow,
    ToggleEnabled,
    OpenSettings,
    OpenPortal,
    OpenGui,
    ReloadConfig,
    Quit,
}

pub struct TrayHandles {
    pub cmd_rx: mpsc::Receiver<TrayCmd>,
    pub enabled: Arc<AtomicBool>,
    pub status: Arc<Mutex<String>>,
    pub server_url: Arc<Mutex<String>>,
    pub agent_token: Arc<Mutex<String>>,
}

pub struct TrayOptions {
    pub enabled_initial: bool,
    pub server_url: String,
    pub agent_token: String,
    pub config_path: Option<PathBuf>,
}

/// Legacy tao-based tray (Linux / fallback). Windows uses `gui::run_tray_host`.
#[cfg(not(windows))]
pub fn spawn_tray(opts: TrayOptions) -> anyhow::Result<TrayHandles> {
    use tao::{
        event::Event,
        event_loop::{ControlFlow, EventLoop},
    };
    use tray_icon::{
        menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
        Icon, TrayIconBuilder,
    };

    let (std_tx, std_rx) = std_mpsc::channel::<TrayCmd>();
    let (tx, rx) = mpsc::channel::<TrayCmd>(32);
    let enabled = Arc::new(AtomicBool::new(opts.enabled_initial));
    let status = Arc::new(Mutex::new("พร้อม".into()));
    let server_url = Arc::new(Mutex::new(opts.server_url.clone()));
    let agent_token = Arc::new(Mutex::new(opts.agent_token.clone()));

    let enabled_t = enabled.clone();
    let status_t = status.clone();
    let server = opts.server_url.clone();

    std::thread::Builder::new()
        .name("dice-tray".into())
        .spawn(move || {
            let event_loop = EventLoop::new();
            let icon = {
                let size = 32u32;
                let mut rgba = vec![0u8; (size * size * 4) as usize];
                for px in rgba.chunks_exact_mut(4) {
                    px[3] = 255;
                }
                Icon::from_rgba(rgba, size, size).unwrap()
            };
            let menu = Menu::new();
            let sync = MenuItem::new("บังคับซิงค์เลย", true, None);
            let quit = MenuItem::new("ออก", true, None);
            let _ = menu.append(&sync);
            let _ = menu.append(&PredefinedMenuItem::separator());
            let _ = menu.append(&quit);
            let mut tray = TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_tooltip("TORTA Dice")
                .with_icon(icon)
                .build()
                .ok();
            let sync_id = sync.id().clone();
            let quit_id = quit.id().clone();
            let menu_rx = MenuEvent::receiver();
            event_loop.run(move |event, _, cf| {
                *cf = ControlFlow::Wait;
                if let Ok(ev) = menu_rx.try_recv() {
                    if ev.id == sync_id {
                        let _ = std_tx.send(TrayCmd::SyncNow);
                    } else if ev.id == quit_id {
                        let _ = std_tx.send(TrayCmd::Quit);
                        tray.take();
                        *cf = ControlFlow::Exit;
                    }
                }
                let _ = (&enabled_t, &status_t, &server);
                if let Event::LoopDestroyed = event {
                    tray.take();
                }
            });
        })?;

    tokio::spawn(async move {
        while let Ok(cmd) = std_rx.recv() {
            let quit = matches!(cmd, TrayCmd::Quit);
            if tx.send(cmd).await.is_err() || quit {
                break;
            }
        }
    });

    Ok(TrayHandles {
        cmd_rx: rx,
        enabled,
        status,
        server_url,
        agent_token,
    })
}

#[cfg(windows)]
pub fn spawn_tray(_opts: TrayOptions) -> anyhow::Result<TrayHandles> {
    anyhow::bail!("Windows tray uses gui::run_tray_host — call that from main instead")
}

pub async fn forward_tray_to_push(
    mut tray_rx: mpsc::Receiver<TrayCmd>,
    push_tx: mpsc::Sender<crate::ws::PushCmd>,
    enabled: Arc<AtomicBool>,
    config_path: Option<PathBuf>,
    on_quit: tokio::sync::oneshot::Sender<()>,
    on_reload: mpsc::Sender<()>,
) {
    let mut quit_sent = Some(on_quit);
    while let Some(cmd) = tray_rx.recv().await {
        match cmd {
            TrayCmd::SyncNow => {
                let _ = push_tx.send(crate::ws::PushCmd::RunNow).await;
            }
            TrayCmd::ToggleEnabled => {
                let on = enabled.load(Ordering::Relaxed);
                tracing::info!(enabled = on, "tray pause/resume");
                if let Some(path) = &config_path {
                    if let Ok(mut cfg) = crate::config::AgentConfig::load(path) {
                        cfg.enabled = on;
                        let _ = cfg.save(path);
                    }
                }
            }
            TrayCmd::ReloadConfig => {
                let _ = on_reload.send(()).await;
            }
            TrayCmd::OpenSettings | TrayCmd::OpenPortal | TrayCmd::OpenGui => {}
            TrayCmd::Quit => {
                if let Some(q) = quit_sent.take() {
                    let _ = q.send(());
                }
                break;
            }
        }
    }
}
