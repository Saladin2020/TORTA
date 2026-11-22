mod client;
mod config;
mod gui;
mod icon;
mod queue;
mod runner;
mod schedule;
mod tray;
mod ws;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc as std_mpsc, Arc, Mutex};
use std::time::Duration;

use clap::{Parser, Subcommand};
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;

use crate::client::ApiClient;
use crate::config::AgentConfig;
use crate::gui::GuiShared;
use crate::queue::WorkQueue;
use crate::tray::TrayCmd;
use crate::ws::PushCmd;

#[derive(Parser, Debug)]
#[command(name = "dice-agent", about = "TORTA Dice backup agent (Windows/Linux)")]
struct Args {
    #[arg(long, short = 'c', default_value = "dice-agent.toml")]
    config: PathBuf,

    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Write a sample config file
    Init,
    /// Run once then exit
    BackupNow,
    /// Poll + websocket forever (headless)
    Run,
    /// System tray icon + settings popup (recommended on desktop)
    Tray,
    /// Toggle local enable flag in config
    SetEnabled { enabled: bool },
}

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Windows tray must own the main thread (winit/eframe)
    if matches!(args.cmd, Some(Cmd::Tray)) {
        init_tracing();
        let cfg = AgentConfig::load(&args.config)?;
        #[cfg(windows)]
        {
            return run_tray_windows(cfg);
        }
        #[cfg(not(windows))]
        {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?;
            return rt.block_on(run_loop(cfg, true));
        }
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(async_main(args))
}

async fn async_main(args: Args) -> anyhow::Result<()> {
    init_tracing();
    match args.cmd.unwrap_or(Cmd::Run) {
        Cmd::Init => {
            let mut cfg = AgentConfig::default();
            cfg.agent_token = "paste-token-from-admin".into();
            cfg.save(&args.config)?;
            println!("wrote {}", args.config.display());
            Ok(())
        }
        Cmd::SetEnabled { enabled } => {
            let mut cfg = AgentConfig::load(&args.config)?;
            cfg.enabled = enabled;
            cfg.save(&args.config)?;
            println!("local enabled = {enabled}");
            Ok(())
        }
        Cmd::BackupNow => {
            let cfg = AgentConfig::load(&args.config)?;
            run_once(&cfg).await
        }
        Cmd::Run => {
            let cfg = AgentConfig::load(&args.config)?;
            run_loop(cfg, false).await
        }
        Cmd::Tray => {
            let cfg = AgentConfig::load(&args.config)?;
            run_loop(cfg, true).await
        }
    }
}

#[cfg(windows)]
fn run_tray_windows(cfg: AgentConfig) -> anyhow::Result<()> {
    let shared = GuiShared {
        status: Arc::new(Mutex::new("พร้อม".into())),
        enabled: Arc::new(AtomicBool::new(cfg.enabled)),
        server_url: Arc::new(Mutex::new(cfg.server_url.clone())),
        agent_token: Arc::new(Mutex::new(cfg.agent_token.clone())),
        config_path: cfg.config_path.clone(),
        running: Arc::new(AtomicBool::new(true)),
    };

    let (cmd_tx, cmd_rx) = std_mpsc::channel::<TrayCmd>();
    let shared_bg = shared.clone();
    let cfg_bg = cfg.clone();

    std::thread::Builder::new()
        .name("dice-agent-bg".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            rt.block_on(agent_background(cfg_bg, shared_bg, cmd_rx));
        })?;

    println!("TORTA Dice Agent");
    println!("  ไอคอนอยู่ในถาดระบบ (คลิก ^ ข้างนาฬิกา)");
    println!("  คลิกซ้ายที่ไอคอน = เปิดหน้าต่างตั้งค่า");
    println!("  คลิกขวา = เมนูลัด");

    gui::run_tray_host(shared, cmd_tx)
}

#[cfg(windows)]
async fn agent_background(
    mut cfg: AgentConfig,
    shared: GuiShared,
    cmd_rx: std_mpsc::Receiver<TrayCmd>,
) {
    let mut client = ApiClient::new(&cfg);
    let queue_path = WorkQueue::path_for_config(cfg.config_path.as_deref());
    let host = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".into());
    let os = std::env::consts::OS.to_string();
    let mut last_run_key = String::new();

    let (push_tx, mut push_rx) = mpsc::channel::<PushCmd>(8);
    tokio::spawn(ws::run_push_loop(cfg.clone(), push_tx.clone()));

    // Bridge std cmd channel → async
    let (async_cmd_tx, mut async_cmd_rx) = mpsc::channel::<TrayCmd>(32);
    let running = shared.running.clone();
    std::thread::spawn(move || {
        while let Ok(cmd) = cmd_rx.recv() {
            let quit = matches!(cmd, TrayCmd::Quit);
            if async_cmd_tx.blocking_send(cmd).is_err() || quit {
                break;
            }
        }
    });

    let mut poll = tokio::time::interval(Duration::from_secs(cfg.poll_interval_secs.max(15)));
    tracing::info!(server = %cfg.server_url, "agent background started");

    while shared.running.load(Ordering::Relaxed) {
        tokio::select! {
            _ = poll.tick() => {
                let local_enabled = shared.enabled.load(Ordering::Relaxed);
                if let Err(e) = client.heartbeat(&host, &os, local_enabled).await {
                    tracing::warn!(error = %e, "heartbeat failed");
                    if let Ok(mut s) = shared.status.lock() {
                        *s = "เชื่อมต่อเซิร์ฟเวอร์ไม่ได้".into();
                    }
                }
                match client.get_policy().await {
                    Ok(policy) => {
                        let active = local_enabled && policy.enabled && policy.local_enabled;
                        if active {
                            let (due, key) = schedule::should_run_now(&policy.schedule_cron, &last_run_key);
                            if policy.run_now || due {
                                if let Ok(mut s) = shared.status.lock() {
                                    *s = "กำลังซิงค์…".into();
                                }
                                let _ = queue::drain_once(&client, &queue_path).await;
                                let _ = queue::mark_backup_starting(&queue_path);
                                match runner::run_backup(&client, &policy, &cfg.extra_paths, Some(&queue_path)).await {
                                    Ok(()) => {
                                        if due { last_run_key = key; }
                                        if let Ok(mut s) = shared.status.lock() {
                                            *s = "ซิงค์ล่าสุดสำเร็จ".into();
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!(error = %e, "backup failed");
                                        if due { last_run_key = key; }
                                        if let Ok(mut s) = shared.status.lock() {
                                            *s = "ซิงค์ล้มเหลว".into();
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => tracing::warn!(error = %e, "policy fetch failed"),
                }
            }
            cmd = async_cmd_rx.recv() => {
                match cmd {
                    Some(TrayCmd::SyncNow) => {
                        if !shared.enabled.load(Ordering::Relaxed) {
                            continue;
                        }
                        if let Ok(policy) = client.get_policy().await {
                            if policy.enabled && policy.local_enabled {
                                if let Ok(mut s) = shared.status.lock() {
                                    *s = "กำลังซิงค์…".into();
                                }
                                let _ = queue::drain_once(&client, &queue_path).await;
                                let _ = queue::mark_backup_starting(&queue_path);
                                match runner::run_backup(&client, &policy, &cfg.extra_paths, Some(&queue_path)).await {
                                    Ok(()) => {
                                        if let Ok(mut s) = shared.status.lock() {
                                            *s = "ซิงค์ล่าสุดสำเร็จ".into();
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!(error = %e, "backup failed");
                                        if let Ok(mut s) = shared.status.lock() {
                                            *s = "ซิงค์ล้มเหลว".into();
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Some(TrayCmd::ToggleEnabled) => {
                        if let Some(path) = &cfg.config_path {
                            if let Ok(mut c) = AgentConfig::load(path) {
                                c.enabled = shared.enabled.load(Ordering::Relaxed);
                                let _ = c.save(path);
                            }
                        }
                    }
                    Some(TrayCmd::ReloadConfig) => {
                        if let Some(path) = &cfg.config_path {
                            if let Ok(new_cfg) = AgentConfig::load(path) {
                                cfg.server_url = new_cfg.server_url;
                                cfg.agent_token = new_cfg.agent_token;
                                cfg.enabled = new_cfg.enabled;
                                cfg.extra_paths = new_cfg.extra_paths;
                                cfg.config_path = Some(path.clone());
                                client.reconfigure(&cfg);
                                if let Ok(mut u) = shared.server_url.lock() {
                                    *u = cfg.server_url.clone();
                                }
                                if let Ok(mut t) = shared.agent_token.lock() {
                                    *t = cfg.agent_token.clone();
                                }
                                shared.enabled.store(cfg.enabled, Ordering::Relaxed);
                                tracing::info!("config reloaded from settings popup");
                            }
                        }
                    }
                    Some(TrayCmd::Quit) | None => {
                        running.store(false, Ordering::SeqCst);
                        break;
                    }
                    _ => {}
                }
            }
            cmd = push_rx.recv() => {
                if let Some(PushCmd::RunNow) = cmd {
                    if shared.enabled.load(Ordering::Relaxed) {
                        if let Ok(policy) = client.get_policy().await {
                            if policy.enabled && policy.local_enabled {
                                if let Ok(mut s) = shared.status.lock() {
                                    *s = "กำลังซิงค์…".into();
                                }
                                let _ = queue::drain_once(&client, &queue_path).await;
                                let _ = queue::mark_backup_starting(&queue_path);
                                match runner::run_backup(&client, &policy, &cfg.extra_paths, Some(&queue_path)).await {
                                    Ok(()) => {
                                        if let Ok(mut s) = shared.status.lock() {
                                            *s = "ซิงค์ล่าสุดสำเร็จ".into();
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!(error = %e, "backup failed");
                                        if let Ok(mut s) = shared.status.lock() {
                                            *s = "ซิงค์ล้มเหลว".into();
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    tracing::info!("agent background stopped");
}

async fn run_once(cfg: &AgentConfig) -> anyhow::Result<()> {
    let client = ApiClient::new(cfg);
    let queue_path = WorkQueue::path_for_config(cfg.config_path.as_deref());
    let host = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".into());
    let os = std::env::consts::OS.to_string();
    client.heartbeat(&host, &os, cfg.enabled).await?;
    let policy = client.get_policy().await?;
    if !(cfg.enabled && policy.enabled && policy.local_enabled) {
        anyhow::bail!("backup disabled (local or server policy)");
    }
    let _ = queue::drain_once(&client, &queue_path).await;
    let _ = queue::mark_backup_starting(&queue_path);
    runner::run_backup(&client, &policy, &cfg.extra_paths, Some(&queue_path)).await
}

async fn run_loop(mut cfg: AgentConfig, with_tray: bool) -> anyhow::Result<()> {
    let mut client = ApiClient::new(&cfg);
    let queue_path = WorkQueue::path_for_config(cfg.config_path.as_deref());
    let host = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".into());
    let os = std::env::consts::OS.to_string();
    let mut last_run_key = String::new();

    let (push_tx, mut push_rx) = mpsc::channel::<PushCmd>(8);
    tokio::spawn(ws::run_push_loop(cfg.clone(), push_tx.clone()));

    let (reload_tx, mut reload_rx) = mpsc::channel::<()>(4);

    let mut tray_enabled = if with_tray {
        let handles = tray::spawn_tray(tray::TrayOptions {
            enabled_initial: cfg.enabled,
            server_url: cfg.server_url.clone(),
            agent_token: cfg.agent_token.clone(),
            config_path: cfg.config_path.clone(),
        })?;
        let (quit_tx, quit_rx) = tokio::sync::oneshot::channel();
        let enabled = handles.enabled.clone();
        let status = handles.status.clone();
        let cmd_rx = handles.cmd_rx;
        tokio::spawn(tray::forward_tray_to_push(
            cmd_rx,
            push_tx.clone(),
            enabled.clone(),
            cfg.config_path.clone(),
            quit_tx,
            reload_tx,
        ));
        Some((enabled, status, quit_rx))
    } else {
        None
    };

    tracing::info!(
        server = %cfg.server_url,
        poll_secs = cfg.poll_interval_secs,
        tray = with_tray,
        "agent started"
    );

    let mut poll = tokio::time::interval(Duration::from_secs(cfg.poll_interval_secs.max(15)));

    loop {
        tokio::select! {
            _ = poll.tick() => {
                let local_enabled = tray_enabled
                    .as_ref()
                    .map(|(en, _, _)| en.load(Ordering::Relaxed))
                    .unwrap_or(cfg.enabled);

                if let Err(e) = client.heartbeat(&host, &os, local_enabled).await {
                    tracing::warn!(error = %e, "heartbeat failed");
                }

                match client.get_policy().await {
                    Ok(policy) => {
                        let active = local_enabled && policy.enabled && policy.local_enabled;
                        if active {
                            let (due, key) = schedule::should_run_now(&policy.schedule_cron, &last_run_key);
                            let triggered = policy.run_now || due;
                            if triggered {
                                if let Some((_, st, _)) = &tray_enabled {
                                    if let Ok(mut s) = st.lock() {
                                        *s = "กำลังซิงค์…".into();
                                    }
                                }
                                if let Err(e) = queue::drain_once(&client, &queue_path).await {
                                    tracing::warn!(error = %e, "queue drain failed");
                                }
                                let _ = queue::mark_backup_starting(&queue_path);
                                match runner::run_backup(&client, &policy, &cfg.extra_paths, Some(&queue_path)).await {
                                    Ok(()) => {
                                        if due { last_run_key = key; }
                                        if let Some((_, st, _)) = &tray_enabled {
                                            if let Ok(mut s) = st.lock() {
                                                *s = "ซิงค์ล่าสุดสำเร็จ".into();
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!(error = %e, "backup failed");
                                        if due { last_run_key = key; }
                                        if let Some((_, st, _)) = &tray_enabled {
                                            if let Ok(mut s) = st.lock() {
                                                *s = "ซิงค์ล้มเหลว".into();
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => tracing::warn!(error = %e, "policy fetch failed"),
                }
            }
            reload = reload_rx.recv() => {
                match reload {
                    Some(()) => {
                        if let Some(path) = &cfg.config_path {
                            if let Ok(new_cfg) = AgentConfig::load(path) {
                                cfg.server_url = new_cfg.server_url;
                                cfg.agent_token = new_cfg.agent_token;
                                cfg.enabled = new_cfg.enabled;
                                cfg.extra_paths = new_cfg.extra_paths;
                                cfg.config_path = Some(path.clone());
                                client.reconfigure(&cfg);
                            }
                        }
                    }
                    None => std::future::pending::<()>().await,
                }
            }
            cmd = push_rx.recv() => {
                match cmd {
                    Some(PushCmd::RunNow) => {
                        let local_enabled = tray_enabled
                            .as_ref()
                            .map(|(en, _, _)| en.load(Ordering::Relaxed))
                            .unwrap_or(cfg.enabled);
                        if !local_enabled { continue; }
                        if let Ok(policy) = client.get_policy().await {
                            if policy.enabled && policy.local_enabled {
                                let _ = queue::drain_once(&client, &queue_path).await;
                                let _ = queue::mark_backup_starting(&queue_path);
                                let _ = runner::run_backup(&client, &policy, &cfg.extra_paths, Some(&queue_path)).await;
                            }
                        }
                    }
                    None => break,
                }
            }
            _ = async {
                if let Some((_, _, quit_rx)) = tray_enabled.as_mut() {
                    let _ = quit_rx.await;
                } else {
                    std::future::pending::<()>().await;
                }
            } => {
                if let Some((en, _, _)) = &tray_enabled {
                    cfg.enabled = en.load(Ordering::Relaxed);
                    if let Some(path) = &cfg.config_path {
                        let _ = cfg.save(path);
                    }
                }
                break;
            }
        }
    }
    Ok(())
}
