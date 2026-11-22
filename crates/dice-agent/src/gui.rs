//! Windows tray host + settings popup (Google Drive–style).
//! Tray icon is created on the GUI thread so it appears in the system tray / overflow.

use std::cell::RefCell;
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};

use eframe::egui::{self, Color32, RichText, Sense, Stroke, Vec2, ViewportCommand};
use eframe::App;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{
    MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
};

use crate::icon;
use crate::tray::TrayCmd;

#[derive(Clone)]
pub struct GuiShared {
    pub status: Arc<Mutex<String>>,
    pub enabled: Arc<AtomicBool>,
    pub server_url: Arc<Mutex<String>>,
    pub agent_token: Arc<Mutex<String>>,
    pub config_path: Option<PathBuf>,
    pub running: Arc<AtomicBool>,
}

/// Block on the GUI thread: shows system-tray icon + settings popup.
pub fn run_tray_host(shared: GuiShared, cmd_tx: mpsc::Sender<TrayCmd>) -> anyhow::Result<()> {
    let icon = icon::tray_icon();

    let menu = Menu::new();
    let item_open = MenuItem::new("เปิดการตั้งค่า", true, None);
    let item_sync = MenuItem::new("บังคับซิงค์เลย", true, None);
    let item_pause = MenuItem::new(
        if shared.enabled.load(Ordering::Relaxed) {
            "หยุดการสำรองชั่วคราว"
        } else {
            "เปิดการสำรองต่อ"
        },
        true,
        None,
    );
    let item_portal = MenuItem::new("เปิดพอร์ทัลบนเว็บ", true, None);
    let item_quit = MenuItem::new("ออกจาก TORTA Dice", true, None);
    menu.append(&item_open)
        .map_err(|e| anyhow::anyhow!("menu: {e}"))?;
    menu.append(&PredefinedMenuItem::separator())
        .map_err(|e| anyhow::anyhow!("menu: {e}"))?;
    menu.append(&item_sync)
        .map_err(|e| anyhow::anyhow!("menu: {e}"))?;
    menu.append(&item_pause)
        .map_err(|e| anyhow::anyhow!("menu: {e}"))?;
    menu.append(&PredefinedMenuItem::separator())
        .map_err(|e| anyhow::anyhow!("menu: {e}"))?;
    menu.append(&item_portal)
        .map_err(|e| anyhow::anyhow!("menu: {e}"))?;
    menu.append(&PredefinedMenuItem::separator())
        .map_err(|e| anyhow::anyhow!("menu: {e}"))?;
    menu.append(&item_quit)
        .map_err(|e| anyhow::anyhow!("menu: {e}"))?;

    let open_id = item_open.id().clone();
    let sync_id = item_sync.id().clone();
    let pause_id = item_pause.id().clone();
    let portal_id = item_portal.id().clone();
    let quit_id = item_quit.id().clone();

    let tray_slot: Rc<RefCell<Option<TrayIcon>>> = Rc::new(RefCell::new(None));
    let tray_for_build = tray_slot.clone();
    let pause_item = item_pause.clone();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([420.0, 580.0])
            .with_min_inner_size([380.0, 520.0])
            .with_resizable(false)
            .with_always_on_top()
            .with_visible(false) // start hidden — open from tray click
            .with_title("TORTA Dice — ตั้งค่า")
            .with_icon(icon::window_icon()),
        centered: true,
        ..Default::default()
    };

    let app_shared = shared.clone();
    let result = eframe::run_native(
        "TORTA Dice",
        options,
        Box::new(move |cc| {
            // Create tray on GUI thread (required on Windows)
            match TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_tooltip("TORTA Dice — คลิกเพื่อตั้งค่า")
                .with_icon(icon)
                .build()
            {
                Ok(tray) => {
                    *tray_for_build.borrow_mut() = Some(tray);
                    tracing::info!("system tray icon ready");
                }
                Err(e) => tracing::error!(error = %e, "failed to create tray icon"),
            }

            apply_theme(&cc.egui_ctx);
            Ok(Box::new(ControlPanel {
                shared: app_shared,
                cmd_tx,
                server_url: String::new(),
                agent_token: String::new(),
                show_token: false,
                message: String::new(),
                logo: None,
                window_visible: false,
                synced_fields: false,
                tray: tray_slot,
                pause_item,
                open_id,
                sync_id,
                pause_id,
                portal_id,
                quit_id,
            }) as Box<dyn App>)
        }),
    );

    shared.running.store(false, Ordering::SeqCst);
    result.map_err(|e| anyhow::anyhow!("tray UI: {e}"))
}

fn apply_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = Vec2::new(10.0, 10.0);
    style.spacing.button_padding = Vec2::new(14.0, 8.0);
    style.visuals.window_fill = Color32::from_rgb(250, 250, 250);
    style.visuals.panel_fill = Color32::from_rgb(250, 250, 250);
    ctx.set_style(style);
}

fn open_url(target: &str) {
    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("cmd")
            .args(["/C", "start", "", target])
            .spawn();
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = Command::new("xdg-open").arg(target).spawn();
    }
}

struct ControlPanel {
    shared: GuiShared,
    cmd_tx: mpsc::Sender<TrayCmd>,
    server_url: String,
    agent_token: String,
    show_token: bool,
    message: String,
    logo: Option<egui::TextureHandle>,
    window_visible: bool,
    synced_fields: bool,
    tray: Rc<RefCell<Option<TrayIcon>>>,
    pause_item: MenuItem,
    open_id: tray_icon::menu::MenuId,
    sync_id: tray_icon::menu::MenuId,
    pause_id: tray_icon::menu::MenuId,
    portal_id: tray_icon::menu::MenuId,
    quit_id: tray_icon::menu::MenuId,
}

impl ControlPanel {
    fn show_window(&mut self, ctx: &egui::Context) {
        if !self.synced_fields {
            self.server_url = self
                .shared
                .server_url
                .lock()
                .map(|s| s.clone())
                .unwrap_or_default();
            self.agent_token = self
                .shared
                .agent_token
                .lock()
                .map(|s| s.clone())
                .unwrap_or_default();
            self.synced_fields = true;
        }
        self.window_visible = true;
        ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(ViewportCommand::Focus);
        ctx.send_viewport_cmd(ViewportCommand::WindowLevel(
            egui::WindowLevel::AlwaysOnTop,
        ));
    }

    fn hide_window(&mut self, ctx: &egui::Context) {
        self.window_visible = false;
        ctx.send_viewport_cmd(ViewportCommand::Visible(false));
    }

    fn ensure_logo(&mut self, ctx: &egui::Context) {
        if self.logo.is_some() {
            return;
        }
        const BYTES: &[u8] = include_bytes!("../assets/icon-256.png");
        if let Ok(img) = image::load_from_memory(BYTES) {
            let rgba = img.to_rgba8();
            let size = [rgba.width() as usize, rgba.height() as usize];
            let color = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
            self.logo = Some(ctx.load_texture("logo", color, Default::default()));
        }
    }

    fn save_connection(&mut self) {
        if let Ok(mut u) = self.shared.server_url.lock() {
            *u = self.server_url.trim().to_string();
        }
        if let Ok(mut t) = self.shared.agent_token.lock() {
            *t = self.agent_token.trim().to_string();
        }
        if let Some(path) = &self.shared.config_path {
            if let Ok(mut cfg) = crate::config::AgentConfig::load(path) {
                cfg.server_url = self.server_url.trim().to_string();
                cfg.agent_token = self.agent_token.trim().to_string();
                cfg.enabled = self.shared.enabled.load(Ordering::Relaxed);
                match cfg.save(path) {
                    Ok(()) => {
                        self.message = "บันทึกแล้ว — ใช้ค่าใหม่ในรอบถัดไป".into();
                        let _ = self.cmd_tx.send(TrayCmd::ReloadConfig);
                    }
                    Err(e) => self.message = format!("บันทึกไม่สำเร็จ: {e}"),
                }
            }
        } else {
            self.message = "ไม่พบไฟล์ config".into();
        }
    }

    fn toggle_pause(&mut self) {
        let now = !self.shared.enabled.load(Ordering::Relaxed);
        self.shared.enabled.store(now, Ordering::Relaxed);
        self.pause_item.set_text(if now {
            "หยุดการสำรองชั่วคราว"
        } else {
            "เปิดการสำรองต่อ"
        });
        if let Ok(mut s) = self.shared.status.lock() {
            *s = if now {
                "พร้อม".into()
            } else {
                "หยุดชั่วคราว".into()
            };
        }
        let _ = self.cmd_tx.send(TrayCmd::ToggleEnabled);
    }
}

impl App for ControlPanel {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ensure_logo(ctx);
        ctx.request_repaint_after(std::time::Duration::from_millis(400));

        // Left-click tray → open settings popup
        while let Ok(ev) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click {
                button,
                button_state,
                ..
            } = ev
            {
                if button == MouseButton::Left && button_state == MouseButtonState::Up {
                    self.show_window(ctx);
                }
            }
        }

        // Context menu actions
        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            if ev.id == self.open_id {
                self.show_window(ctx);
            } else if ev.id == self.sync_id {
                if self.shared.enabled.load(Ordering::Relaxed) {
                    if let Ok(mut s) = self.shared.status.lock() {
                        *s = "กำลังซิงค์…".into();
                    }
                    let _ = self.cmd_tx.send(TrayCmd::SyncNow);
                }
            } else if ev.id == self.pause_id {
                self.toggle_pause();
            } else if ev.id == self.portal_id {
                let url = self
                    .shared
                    .server_url
                    .lock()
                    .map(|s| s.trim_end_matches('/').to_string())
                    .unwrap_or_default();
                open_url(&url);
            } else if ev.id == self.quit_id {
                let _ = self.cmd_tx.send(TrayCmd::Quit);
                self.shared.running.store(false, Ordering::SeqCst);
                *self.tray.borrow_mut() = None;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        }

        // Closing the window hides it (stay in tray) instead of quitting
        ctx.input(|i| {
            if i.viewport().close_requested() && self.window_visible {
                // handled below via deferred hide — egui may already be closing
            }
        });
        if ctx.input(|i| i.viewport().close_requested()) {
            // Cancel quit: just hide (Drive-style)
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.hide_window(ctx);
            return;
        }

        if !self.window_visible {
            return;
        }

        let status = self
            .shared
            .status
            .lock()
            .map(|s| s.clone())
            .unwrap_or_else(|_| "—".into());
        let enabled = self.shared.enabled.load(Ordering::Relaxed);

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if let Some(tex) = &self.logo {
                    ui.add(egui::Image::new(tex).fit_to_exact_size(Vec2::splat(40.0)));
                }
                ui.vertical(|ui| {
                    ui.label(RichText::new("TORTA Dice").strong().size(20.0));
                    ui.label(
                        RichText::new("ตั้งค่า Agent / การเชื่อมต่อ")
                            .size(12.0)
                            .color(Color32::from_rgb(102, 102, 102)),
                    );
                });
            });
            ui.add_space(10.0);
            ui.separator();
            ui.add_space(8.0);

            egui::Frame::new()
                .fill(Color32::WHITE)
                .stroke(Stroke::new(1.0, Color32::from_rgb(234, 234, 234)))
                .corner_radius(8.0)
                .inner_margin(14.0)
                .show(ui, |ui| {
                    let (dot, label) = if !enabled {
                        (Color32::from_rgb(163, 91, 0), "หยุดชั่วคราว")
                    } else if status.contains("กำลัง") {
                        (Color32::from_rgb(0, 112, 243), "กำลังซิงค์")
                    } else if status.contains("ล้มเหลว") {
                        (Color32::from_rgb(224, 0, 0), "มีปัญหา")
                    } else {
                        (Color32::from_rgb(10, 123, 69), "พร้อมใช้งาน")
                    };
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                        ui.painter().circle_filled(rect.center(), 5.0, dot);
                        ui.label(RichText::new(label).strong());
                    });
                    ui.label(RichText::new(status).color(Color32::GRAY));
                });

            ui.add_space(12.0);

            if ui
                .add_sized(
                    [ui.available_width(), 38.0],
                    egui::Button::new(
                        RichText::new("บังคับซิงค์เลย")
                            .strong()
                            .color(Color32::WHITE),
                    )
                    .fill(Color32::BLACK)
                    .corner_radius(8.0),
                )
                .clicked()
            {
                if enabled {
                    let _ = self.cmd_tx.send(TrayCmd::SyncNow);
                    self.message = "สั่งซิงค์แล้ว".into();
                } else {
                    self.message = "เปิดการสำรองก่อน".into();
                }
            }

            ui.add_space(6.0);
            let pause_label = if enabled {
                "หยุดการสำรองชั่วคราว"
            } else {
                "เปิดการสำรองต่อ"
            };
            if ui
                .add_sized(
                    [ui.available_width(), 34.0],
                    egui::Button::new(pause_label).corner_radius(8.0),
                )
                .clicked()
            {
                self.toggle_pause();
            }

            ui.add_space(16.0);
            ui.label(RichText::new("ค่าเชื่อมต่อเซิร์ฟเวอร์").strong().size(15.0));
            ui.add_space(6.0);

            ui.label(RichText::new("Server URL").size(12.0).color(Color32::GRAY));
            ui.add(
                egui::TextEdit::singleline(&mut self.server_url)
                    .desired_width(f32::INFINITY)
                    .hint_text("http://127.0.0.1:8080"),
            );

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Agent token").size(12.0).color(Color32::GRAY));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button(if self.show_token { "ซ่อน" } else { "แสดง" })
                        .clicked()
                    {
                        self.show_token = !self.show_token;
                    }
                });
            });
            ui.add(
                egui::TextEdit::singleline(&mut self.agent_token)
                    .desired_width(f32::INFINITY)
                    .password(!self.show_token)
                    .hint_text("วาง token จาก Admin → Agents"),
            );

            ui.add_space(10.0);
            if ui
                .add_sized(
                    [ui.available_width(), 36.0],
                    egui::Button::new(RichText::new("บันทึกการตั้งค่า").strong())
                        .fill(Color32::from_rgb(0, 112, 243))
                        .corner_radius(8.0),
                )
                .clicked()
            {
                self.save_connection();
            }

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("เปิดพอร์ทัล").clicked() {
                    open_url(self.server_url.trim_end_matches('/'));
                }
                if ui.button("ซ่อนหน้าต่าง").clicked() {
                    self.hide_window(ctx);
                }
            });

            if !self.message.is_empty() {
                ui.add_space(10.0);
                ui.label(
                    RichText::new(&self.message)
                        .color(Color32::from_rgb(10, 123, 69))
                        .size(12.0),
                );
            }

            ui.add_space(18.0);
            ui.label(
                RichText::new("ไอคอนอยู่ในถาดระบบ (คลิก ^ ใกล้นาฬิกา) — คลิกซ้ายเพื่อเปิดหน้านี้")
                    .size(11.0)
                    .color(Color32::GRAY),
            );
        });
    }
}

