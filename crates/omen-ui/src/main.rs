use anyhow::{Context, Result};
use eframe::egui;
use egui::{viewport::IconData, Color32, RichText, Stroke};
use egui_plot::{Line, Plot, PlotPoints};
use omen_core::*;
use resvg::{tiny_skia, usvg};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const ZOOM_KEY: &str = "omen_ui_zoom";
const POLL_INTERVAL: Duration = Duration::from_millis(500);
const IDLE_REPAINT: Duration = Duration::from_millis(1000);
const WIDE_BREAKPOINT: f32 = 980.0;
const MAX_CONTENT_WIDTH: f32 = 1400.0;
const MIN_ZOOM: f32 = 0.75;
const MAX_ZOOM: f32 = 2.0;

/// Messages flowing from the background IO worker back to the UI thread.
enum UiMsg {
    Snapshot(Snapshot),
    Error(String),
}

/// Spawns a background thread that owns all daemon-socket IO, so the UI
/// thread never blocks on a `connect()`/`read()` while painting a frame.
/// It auto-polls on a timer and also drains any user-triggered requests,
/// waking the UI (`ctx.request_repaint()`) only when there's new data.
fn spawn_worker(
    ctx: egui::Context,
    cmd_rx: mpsc::Receiver<Request>,
    msg_tx: mpsc::Sender<UiMsg>,
) {
    thread::spawn(move || {
        let mut last_poll = Instant::now() - POLL_INTERVAL;
        loop {
            let elapsed = last_poll.elapsed();
            let timeout = POLL_INTERVAL.saturating_sub(elapsed);
            let req = match cmd_rx.recv_timeout(timeout) {
                Ok(req) => req,
                Err(mpsc::RecvTimeoutError::Timeout) => Request::GetSnapshot,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            };
            last_poll = Instant::now();
            let msg = match send(req) {
                Ok(Response::Snapshot(s)) => UiMsg::Snapshot(s),
                Ok(Response::Error { message }) => UiMsg::Error(message),
                Err(err) => UiMsg::Error(format!("Daemon unavailable: {err:#}")),
            };
            if msg_tx.send(msg).is_err() {
                return; // UI gone
            }
            ctx.request_repaint();
        }
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Dashboard,
    Curve,
    Power,
}

#[derive(Clone)]
struct DraftCurve {
    profile: String,
    curve: FanCurve,
    dirty: bool,
    dragging: bool,
    last_edit: Instant,
}

struct App {
    snapshot: Option<Snapshot>,
    selected_profile: String,
    draft_curve: Option<DraftCurve>,
    status: String,
    connected: bool,
    zoom: f32,
    cmd_tx: mpsc::Sender<Request>,
    msg_rx: mpsc::Receiver<UiMsg>,
    page: Page,
}

fn load_icon() -> IconData {
    let svg = include_bytes!("../assets/omen.svg");
    let mut db = usvg::fontdb::Database::new();
    db.load_system_fonts();
    let tree = usvg::Tree::from_data(svg, &usvg::Options::default()).expect("svg");
    let size = tree.size().to_int_size();
    let mut pixmap = tiny_skia::Pixmap::new(size.width(), size.height()).expect("pixmap");
    resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());
    IconData {
        rgba: pixmap.data().to_vec(),
        width: size.width(),
        height: size.height(),
    }
}

fn main() -> Result<()> {
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("OMEN Control")
            .with_app_id("omen-control")
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([560.0, 480.0])
            .with_icon(load_icon()),
        persist_window: true,
        ..Default::default()
    };
    eframe::run_native("OMEN Control", native, Box::new(|cc| Ok(Box::new(App::new(cc)?))))
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    Ok(())
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Result<Self> {
        let zoom = cc
            .storage
            .and_then(|s| eframe::get_value::<f32>(s, ZOOM_KEY))
            .unwrap_or(1.0)
            .clamp(MIN_ZOOM, MAX_ZOOM);
        cc.egui_ctx.set_pixels_per_point(zoom);
        set_style(&cc.egui_ctx);

        let (cmd_tx, cmd_rx) = mpsc::channel::<Request>();
        let (msg_tx, msg_rx) = mpsc::channel::<UiMsg>();
        spawn_worker(cc.egui_ctx.clone(), cmd_rx, msg_tx);

        Ok(Self {
            snapshot: None,
            selected_profile: "Balanced".into(),
            draft_curve: None,
            status: "Connecting…".into(),
            connected: false,
            zoom,
            cmd_tx,
            msg_rx,
            page: Page::Dashboard,
        })
    }

    /// Fire-and-forget: hand the request to the worker thread and move on.
    /// The resulting snapshot (or error) arrives async via `msg_rx` and is
    /// picked up next frame in `drain_worker`.
    fn send(&mut self, req: Request) {
        let _ = self.cmd_tx.send(req);
    }

    fn refresh(&mut self) {
        self.send(Request::GetSnapshot);
    }

    fn drain_worker(&mut self) {
        while let Ok(msg) = self.msg_rx.try_recv() {
            match msg {
                UiMsg::Snapshot(s) => {
                    if self.snapshot.is_none() {
                        self.selected_profile = s.state.active_profile.clone();
                    }
                    self.snapshot = Some(s);
                    self.status = "Live".into();
                    self.connected = true;
                }
                UiMsg::Error(message) => {
                    self.status = message;
                    self.connected = false;
                }
            }
        }
    }

    fn set_zoom(&mut self, ctx: &egui::Context, zoom: f32) {
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        ctx.set_pixels_per_point(self.zoom);
    }

    fn sync_draft_from_snapshot(&mut self) {
        let Some(snapshot) = &self.snapshot else { return; };
        let needs_reset = self
            .draft_curve
            .as_ref()
            .map(|d| d.profile.as_str() != self.selected_profile.as_str())
            .unwrap_or(true);
        if needs_reset {
            if let Some(profile) = snapshot.state.profile(&self.selected_profile) {
                self.draft_curve = Some(DraftCurve {
                    profile: self.selected_profile.clone(),
                    curve: profile.curve.clone(),
                    dirty: false,
                    dragging: false,
                    last_edit: Instant::now(),
                });
            }
        }
    }
}

impl eframe::App for App {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, ZOOM_KEY, &self.zoom);
    }

    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.drain_worker();
        self.sync_draft_from_snapshot();

        // Debounced curve autosave: only push to the daemon once edits settle.
        if let Some(draft) = &self.draft_curve {
            if draft.dirty && !draft.dragging && draft.last_edit.elapsed() > Duration::from_millis(350)
            {
                let profile = draft.profile.clone();
                let mut curve = draft.curve.clone();
                curve.normalize();
                self.send(Request::SetCurve { profile, curve });
                if let Some(d) = &mut self.draft_curve {
                    d.dirty = false;
                }
            }
        }

        top_bar(self, ctx);

        let Some(snapshot) = self.snapshot.clone() else {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.centered_and_justified(|ui| {
                    ui.vertical_centered(|ui| {
                        ui.heading("Waiting for omen-daemon…");
                        ui.add_space(6.0);
                        ui.label(RichText::new(&self.status).color(Color32::GRAY));
                    });
                });
            });
            ctx.request_repaint_after(IDLE_REPAINT);
            return;
        };

        // Narrow windows: collapse the rail into compact icon-only buttons
        // instead of squeezing full labels into too little space.
        let compact_rail = ctx.screen_rect().width() < WIDE_BREAKPOINT;
        nav_rail(self, ctx, compact_rail);

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let avail = ui.available_width();
                let content_width = avail.min(MAX_CONTENT_WIDTH);
                let margin = ((avail - content_width) / 2.0).max(0.0);
                ui.horizontal(|ui| {
                    if margin > 1.0 {
                        ui.add_space(margin);
                    }
                    ui.vertical(|ui| {
                        ui.set_width(content_width);
                        ui.add_space(4.0);
                        match self.page {
                            Page::Dashboard => page_dashboard(self, &snapshot, ui),
                            Page::Curve => page_curve(self, &snapshot, ui),
                            Page::Power => page_power(self, &snapshot, ui),
                        }
                        ui.add_space(12.0);
                    });
                });
            });
        });

        // Bounded idle redraw rate: the worker wakes us immediately when
        // new data arrives, this just guarantees the telemetry graph keeps
        // ticking even if a poll response is ever delayed.
        ctx.request_repaint_after(IDLE_REPAINT);
    }
}

fn top_bar(app: &mut App, ctx: &egui::Context) {
    egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let page_title = match app.page {
                Page::Dashboard => "Dashboard",
                Page::Curve => "Curve Studio",
                Page::Power => "Modes & Power",
            };
            ui.label(RichText::new(page_title).size(17.0).strong().color(Color32::from_gray(230)));
            ui.add_space(10.0);
            let (dot, text) = if app.connected {
                (Color32::from_rgb(64, 200, 128), "Connected")
            } else {
                (Color32::from_rgb(224, 76, 76), app.status.as_str())
            };
            ui.colored_label(dot, "●");
            ui.label(RichText::new(text).color(Color32::GRAY).small());

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::new("Refresh").rounding(10.0)).clicked() {
                    app.refresh();
                }
                ui.add_space(12.0);
                if ui.small_button("＋").clicked() {
                    app.set_zoom(ctx, app.zoom + 0.1);
                }
                ui.label(RichText::new(format!("{:.0}%", app.zoom * 100.0)).small().color(Color32::GRAY));
                if ui.small_button("－").clicked() {
                    app.set_zoom(ctx, app.zoom - 0.1);
                }
            });
        });
        ui.add_space(6.0);
    });
}

fn nav_rail(app: &mut App, ctx: &egui::Context, compact: bool) {
    let width = if compact { 56.0 } else { 168.0 };
    egui::SidePanel::left("nav_rail")
        .resizable(false)
        .exact_width(width)
        .frame(egui::Frame::none().fill(Color32::from_rgb(13, 15, 22)).inner_margin(egui::Margin::symmetric(if compact { 6.0 } else { 12.0 }, 14.0)))
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                draw_logo_mark(ui, if compact { 30.0 } else { 40.0 });
                if !compact {
                    ui.add_space(4.0);
                    ui.label(RichText::new("OMEN").strong().size(15.0).color(Color32::from_rgb(255, 116, 82)));
                }
            });
            ui.add_space(18.0);
            nav_button(ui, app, Page::Dashboard, "📊", "Dashboard", compact);
            nav_button(ui, app, Page::Curve, "📈", "Curve", compact);
            nav_button(ui, app, Page::Power, "⚡", "Power", compact);
        });
}

fn nav_button(ui: &mut egui::Ui, app: &mut App, page: Page, icon: &str, label: &str, compact: bool) {
    let selected = app.page == page;
    let text = if compact {
        RichText::new(icon).size(18.0)
    } else {
        RichText::new(format!("{icon}  {label}")).size(14.0)
    };
    let button = egui::Button::new(text)
        .min_size(egui::vec2(ui.available_width(), 34.0))
        .fill(if selected { Color32::from_rgb(255, 96, 64) } else { Color32::TRANSPARENT })
        .stroke(Stroke::NONE)
        .rounding(10.0);
    if ui.add(button).clicked() {
        app.page = page;
    }
    ui.add_space(4.0);
}

fn page_dashboard(app: &mut App, snapshot: &Snapshot, ui: &mut egui::Ui) {
    glass_card(ui, |ui| {
        ui.heading("Live Telemetry");
        ui.add_space(8.0);
        ui.columns(3, |cols| {
            stat_tile(&mut cols[0], "CPU Temp", &format!("{:.1} °C", snapshot.live.cpu_temp_c), Color32::from_rgb(255, 108, 72));
            stat_tile(&mut cols[1], "Fan RPM", &format!("{}", snapshot.live.rpm), Color32::from_rgb(72, 142, 255));
            stat_tile(&mut cols[2], "Duty", &format!("{:.0}%", snapshot.live.duty_pct), Color32::from_rgb(176, 92, 255));
        });
        ui.columns(3, |cols| {
            stat_tile(&mut cols[0], "Avg Temp", &format!("{:.1} °C", snapshot.live.avg_temp_c), Color32::from_rgb(255, 182, 92));
            stat_tile(&mut cols[1], "Profile", &snapshot.state.active_profile, Color32::from_rgb(92, 214, 196));
            stat_tile(
                &mut cols[2],
                "Power",
                match snapshot.live.power_source {
                    PowerSource::AC => "AC",
                    PowerSource::Battery => "Battery",
                    PowerSource::Unknown => "Unknown",
                },
                Color32::from_rgb(104, 136, 255),
            );
        });
        if snapshot.live.max_fan_active {
            ui.add_space(8.0);
            badge(ui, "MAX FAN ACTIVE", Color32::from_rgb(255, 72, 72));
        }
        if let Some(mode) = &snapshot.live.graphics_mode {
            ui.add_space(4.0);
            badge(ui, &format!("GPU: {}", mode.label()), Color32::from_rgb(146, 106, 255));
        }
        ui.add_space(10.0);
        let temp_points = PlotPoints::from_iter(snapshot.live.history.iter().enumerate().map(|(i, p)| [i as f64, p.temp_c as f64]));
        let duty_points = PlotPoints::from_iter(snapshot.live.history.iter().enumerate().map(|(i, p)| [i as f64, p.duty_pct as f64]));
        Plot::new("telemetry_plot")
            .height(220.0)
            .allow_zoom(false)
            .allow_drag(false)
            .show(ui, |plot_ui| {
                plot_ui.line(Line::new(temp_points).name("Temp").color(Color32::from_rgb(255, 108, 72)));
                plot_ui.line(Line::new(duty_points).name("Duty").color(Color32::from_rgb(176, 92, 255)));
            });
    });

    ui.add_space(14.0);

    glass_card(ui, |ui| {
        ui.heading("Quick Profile Actions");
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            for p in &snapshot.state.profiles {
                let selected = app.selected_profile == p.name;
                let button = egui::Button::new(RichText::new(&p.name).strong())
                    .fill(if selected { Color32::from_rgb(255, 96, 64) } else { Color32::from_rgb(24, 28, 40) })
                    .stroke(Stroke::new(1.0, if selected { Color32::from_rgb(255, 96, 64) } else { Color32::from_rgb(44, 50, 68) }))
                    .rounding(12.0);
                if ui.add(button).clicked() {
                    app.selected_profile = p.name.clone();
                }
            }
        });
        ui.add_space(10.0);
        if ui.add(action_button("Apply Selected Profile")).clicked() {
            app.send(Request::SetActiveProfile {
                name: app.selected_profile.clone(),
            });
        }
    });
}

fn profile_switcher(app: &mut App, snapshot: &Snapshot, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Editing profile:").color(Color32::GRAY).small());
        for p in &snapshot.state.profiles {
            let selected = app.selected_profile == p.name;
            if ui.add(pill_button(&p.name, selected)).clicked() {
                app.selected_profile = p.name.clone();
            }
        }
    });
}

fn page_power(app: &mut App, snapshot: &Snapshot, ui: &mut egui::Ui) {
    profile_switcher(app, snapshot, ui);
    ui.add_space(14.0);
    glass_card(ui, |ui| {
        ui.heading("Modes & Power");
        ui.add_space(8.0);
        let Some(profile) = snapshot.state.profile(&app.selected_profile) else {
            return;
        };

        ui.label(RichText::new("Fan Mode").strong());
        ui.horizontal_wrapped(|ui| {
            for mode in [FanMode::Auto, FanMode::Curve, FanMode::Manual, FanMode::Max] {
                let active = profile.fan_mode == mode;
                if ui
                    .add(pill_button(&format!("{:?}", mode), active))
                    .clicked()
                {
                    app.send(Request::SetFanMode {
                        profile: app.selected_profile.clone(),
                        mode,
                    });
                }
            }
        });

        ui.add_space(10.0);
        let mut manual = profile.manual_pct as f32;
        ui.add_enabled_ui(profile.fan_mode == FanMode::Manual, |ui| {
            if ui
                .add(egui::Slider::new(&mut manual, 25.0..=100.0).text("Manual Fan %"))
                .changed()
            {
                app.send(Request::SetManualDuty {
                    profile: app.selected_profile.clone(),
                    duty_pct: manual.round() as u8,
                });
            }
        });

        ui.add_space(8.0);
        let mut max_fan = profile.max_fan;
        if ui.toggle_value(&mut max_fan, "Max Fan Override").changed() {
            app.send(Request::SetMaxFan {
                profile: app.selected_profile.clone(),
                enabled: max_fan,
            });
        }

        ui.separator();
        ui.label(RichText::new("Platform Mode").strong());
        egui::ComboBox::from_id_salt("platform_mode")
            .selected_text(profile.platform_mode.clone().unwrap_or_else(|| "(none)".into()))
            .show_ui(ui, |ui| {
                if ui.selectable_label(profile.platform_mode.is_none(), "(none)").clicked() {
                    app.send(Request::SetPlatformMode {
                        profile: app.selected_profile.clone(),
                        mode: None,
                    });
                }
                for mode in &snapshot.state.capabilities.platform_modes {
                    if ui
                        .selectable_label(profile.platform_mode.as_deref() == Some(mode.key.as_str()), &mode.label)
                        .clicked()
                    {
                        app.send(Request::SetPlatformMode {
                            profile: app.selected_profile.clone(),
                            mode: Some(mode.key.clone()),
                        });
                    }
                }
            });

        if snapshot.state.capabilities.supports_graphics_mode {
            ui.add_space(8.0);
            ui.label(RichText::new("Graphics Mode").strong());
            egui::ComboBox::from_id_salt("graphics_mode")
                .selected_text(
                    profile
                        .graphics_mode
                        .clone()
                        .map(|m| m.label().to_string())
                        .unwrap_or_else(|| "(inherit)".into()),
                )
                .show_ui(ui, |ui| {
                    if ui.selectable_label(profile.graphics_mode.is_none(), "(inherit)").clicked() {
                        app.send(Request::SetGraphicsMode {
                            profile: app.selected_profile.clone(),
                            mode: None,
                        });
                    }
                    for mode in &snapshot.state.capabilities.graphics_modes {
                        if ui
                            .selectable_label(profile.graphics_mode.as_ref() == Some(mode), mode.label())
                            .clicked()
                        {
                            app.send(Request::SetGraphicsMode {
                                profile: app.selected_profile.clone(),
                                mode: Some(mode.clone()),
                            });
                        }
                    }
                });
        }

        ui.separator();
        ui.label(RichText::new("Battery Automation").strong());
        let mut enabled = snapshot.state.battery_behavior.enabled;
        if ui.checkbox(&mut enabled, "Enable battery mode automation").changed() {
            app.send(Request::SetBatteryBehavior {
                enabled,
                battery_mode: snapshot.state.battery_behavior.battery_mode.clone(),
                restore_ac_profile: snapshot.state.battery_behavior.restore_ac_profile,
            });
        }
        let mut restore = snapshot.state.battery_behavior.restore_ac_profile;
        if ui.checkbox(&mut restore, "Restore AC profile on reconnect").changed() {
            app.send(Request::SetBatteryBehavior {
                enabled: snapshot.state.battery_behavior.enabled,
                battery_mode: snapshot.state.battery_behavior.battery_mode.clone(),
                restore_ac_profile: restore,
            });
        }
        egui::ComboBox::from_id_salt("battery_mode")
            .selected_text(snapshot.state.battery_behavior.battery_mode.clone().unwrap_or_else(|| "(none)".into()))
            .show_ui(ui, |ui| {
                if ui.selectable_label(snapshot.state.battery_behavior.battery_mode.is_none(), "(none)").clicked() {
                    app.send(Request::SetBatteryBehavior {
                        enabled: snapshot.state.battery_behavior.enabled,
                        battery_mode: None,
                        restore_ac_profile: snapshot.state.battery_behavior.restore_ac_profile,
                    });
                }
                for mode in &snapshot.state.capabilities.platform_modes {
                    if ui
                        .selectable_label(snapshot.state.battery_behavior.battery_mode.as_deref() == Some(mode.key.as_str()), &mode.label)
                        .clicked()
                    {
                        app.send(Request::SetBatteryBehavior {
                            enabled: snapshot.state.battery_behavior.enabled,
                            battery_mode: Some(mode.key.clone()),
                            restore_ac_profile: snapshot.state.battery_behavior.restore_ac_profile,
                        });
                    }
                }
            });
    });
}

fn page_curve(app: &mut App, snapshot: &Snapshot, ui: &mut egui::Ui) {
    profile_switcher(app, snapshot, ui);
    ui.add_space(14.0);
    glass_card(ui, |ui| {
        ui.heading("Curve Studio");
        ui.label("Drag points. Double-click to add. Right-click a point to delete. Curve autosaves after edits settle.");
        ui.add_space(6.0);

        if let Some(draft) = &mut app.draft_curve {
            let (changed, dragging) = curve_editor(ui, &mut draft.curve);
            if changed {
                draft.dirty = true;
                draft.last_edit = Instant::now();
            }
            draft.dragging = dragging;
        }

        let should_apply = app.draft_curve.as_ref().map(|d| d.dirty).unwrap_or(false);

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.add_enabled(should_apply, action_button("Apply Now")).clicked() {
                let data_to_apply = app.draft_curve.as_ref().map(|draft| {
                    (draft.profile.clone(), draft.curve.clone())
                });
                if let Some((profile, mut curve)) = data_to_apply {
                    curve.normalize();
                    app.send(Request::SetCurve { profile, curve });
                    if let Some(draft) = &mut app.draft_curve {
                        draft.dirty = false;
                    }
                }
            }
            if ui.button("Reset to Saved").clicked() {
                if let Some(draft) = &mut app.draft_curve {
                    if let Some(saved) = snapshot.state.profile(&draft.profile) {
                        draft.curve = saved.curve.clone();
                        draft.dirty = false;
                        draft.dragging = false;
                    }
                }
            }
        });
    });
}

fn curve_editor(ui: &mut egui::Ui, curve: &mut FanCurve) -> (bool, bool) {
    const MAX_POINTS: usize = 14;
    const MIN_TEMP: f32 = 45.0;
    const MAX_TEMP: f32 = 90.0;
    let mut changed = false;

    if curve.points.is_empty() {
        curve.points = FanCurve::default().points;
        changed = true;
    }

    let desired = egui::vec2(ui.available_width(), 300.0);
    let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 16.0, Color32::from_rgb(16, 18, 26));

    for i in 0..=9 {
        let x = egui::lerp(rect.left()..=rect.right(), i as f32 / 9.0);
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            Stroke::new(1.0, Color32::from_gray(34)),
        );
    }
    for i in 0..=10 {
        let y = egui::lerp(rect.bottom()..=rect.top(), i as f32 / 10.0);
        painter.line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            Stroke::new(1.0, Color32::from_gray(34)),
        );
    }

    let to_screen = |p: CurvePoint| {
        egui::pos2(
            rect.left() + ((p.temp_c - MIN_TEMP) / (MAX_TEMP - MIN_TEMP)) * rect.width(),
            rect.bottom() - (p.duty_pct / 100.0) * rect.height(),
        )
    };
    let from_screen = |pos: egui::Pos2| CurvePoint {
        temp_c: (MIN_TEMP + ((pos.x - rect.left()) / rect.width()) * (MAX_TEMP - MIN_TEMP))
            .clamp(MIN_TEMP, MAX_TEMP),
        duty_pct: (((rect.bottom() - pos.y) / rect.height()) * 100.0).clamp(0.0, 100.0),
    };

    for pair in curve.points.windows(2) {
        painter.line_segment(
            [to_screen(pair[0]), to_screen(pair[1])],
            Stroke::new(3.0, Color32::from_rgb(255, 96, 64)),
        );
    }

    let drag_id = ui.id().with("curve_drag_index");
    let mut dragging: Option<usize> = ui.ctx().data(|d| d.get_temp(drag_id)).unwrap_or(None);

    if response.drag_started() {
        if let Some(pointer) = response.interact_pointer_pos() {
            dragging = curve
                .points
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| {
                    to_screen(**a)
                        .distance(pointer)
                        .partial_cmp(&to_screen(**b).distance(pointer))
                        .unwrap()
                })
                .map(|(i, _)| i);
            if let Some(i) = dragging {
                if to_screen(curve.points[i]).distance(pointer) > 18.0 {
                    dragging = None;
                }
            }
        }
    }

    if response.dragged() {
        if let (Some(i), Some(pointer)) = (dragging, response.interact_pointer_pos()) {
            let mut p = from_screen(pointer);
            if i == 0 {
                p.temp_c = MIN_TEMP;
                p.duty_pct = 38.0;
            } else {
                p.temp_c = p.temp_c.max(curve.points[i - 1].temp_c + 1.0);
                p.duty_pct = p.duty_pct.max(curve.points[i - 1].duty_pct);
            }
            if i == curve.points.len() - 1 {
                p.temp_c = MAX_TEMP;
                p.duty_pct = 100.0;
            } else {
                p.temp_c = p.temp_c.min(curve.points[i + 1].temp_c - 1.0);
                p.duty_pct = p.duty_pct.min(curve.points[i + 1].duty_pct);
            }
            if curve.points[i] != p {
                curve.points[i] = p;
                changed = true;
            }
        }
    }
    if response.drag_stopped() {
        dragging = None;
    }
    ui.ctx().data_mut(|d| d.insert_temp(drag_id, dragging));

    if response.double_clicked() {
        if let Some(pointer) = response.interact_pointer_pos() {
            if curve.points.len() < MAX_POINTS {
                let mut p = from_screen(pointer);
                let idx = curve
                    .points
                    .iter()
                    .position(|q| q.temp_c > p.temp_c)
                    .unwrap_or(curve.points.len());
                if idx > 0 && idx < curve.points.len() {
                    let left = curve.points[idx - 1];
                    let right = curve.points[idx];
                    if right.temp_c - left.temp_c > 2.0 {
                        p.temp_c = p.temp_c.clamp(left.temp_c + 1.0, right.temp_c - 1.0);
                        p.duty_pct = p.duty_pct.clamp(left.duty_pct, right.duty_pct);
                        curve.points.insert(idx, p);
                        changed = true;
                    }
                }
            }
        }
    }

    if response.secondary_clicked() {
        if let Some(pointer) = response.interact_pointer_pos() {
            if let Some((i, _)) = curve
                .points
                .iter()
                .enumerate()
                .find(|(_, p)| to_screen(**p).distance(pointer) < 10.0)
            {
                if i != 0 && i != curve.points.len() - 1 && curve.points.len() > 2 {
                    curve.points.remove(i);
                    changed = true;
                }
            }
        }
    }

    for (i, p) in curve.points.iter().enumerate() {
        let pos = to_screen(*p);
        painter.circle_filled(
            pos,
            7.0,
            if i == 0 || i == curve.points.len() - 1 {
                Color32::from_rgb(255, 128, 128)
            } else {
                Color32::WHITE
            },
        );
        painter.text(
            pos + egui::vec2(8.0, -10.0),
            egui::Align2::LEFT_BOTTOM,
            format!("{:.0}°/{:.0}%", p.temp_c, p.duty_pct),
            egui::TextStyle::Small.resolve(ui.style()),
            Color32::LIGHT_GRAY,
        );
    }

    (changed, dragging.is_some())
}

fn draw_logo_mark(ui: &mut egui::Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, size * 0.22, Color32::from_rgb(11, 13, 18));

    let pt = |x: f32, y: f32| egui::pos2(rect.left() + x * rect.width(), rect.top() + y * rect.height());

    // Outer diamond (convex — required for Shape::convex_polygon).
    let outer = vec![pt(0.5, 0.08), pt(0.92, 0.5), pt(0.5, 0.92), pt(0.08, 0.5)];
    painter.add(egui::Shape::convex_polygon(outer, Color32::from_rgb(255, 116, 82), Stroke::NONE));

    // Inner diamond cut out in the background colour, giving a hollow
    // flame-like silhouette rather than a solid gem.
    let inner = vec![pt(0.5, 0.30), pt(0.70, 0.5), pt(0.5, 0.70), pt(0.30, 0.5)];
    painter.add(egui::Shape::convex_polygon(inner, Color32::from_rgb(11, 13, 18), Stroke::NONE));
}

fn set_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = Color32::from_rgb(10, 12, 18);
    style.visuals.window_fill = Color32::from_rgb(10, 12, 18);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(255, 96, 64);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(178, 52, 38);
    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(22, 26, 36);
    style.visuals.selection.bg_fill = Color32::from_rgb(255, 96, 64);
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 7.0);
    style.visuals.window_rounding = 14.0.into();
    style.visuals.menu_rounding = 12.0.into();
    style.visuals.widgets.noninteractive.rounding = 12.0.into();
    style.visuals.widgets.inactive.rounding = 12.0.into();
    style.visuals.widgets.hovered.rounding = 12.0.into();
    style.visuals.widgets.active.rounding = 12.0.into();
    ctx.set_style(style);
}

fn glass_card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::none()
        .fill(Color32::from_rgba_premultiplied(18, 21, 30, 235))
        .stroke(Stroke::new(1.0, Color32::from_rgb(34, 39, 58)))
        .rounding(16.0)
        .inner_margin(16.0)
        .show(ui, add);
}

fn stat_tile(ui: &mut egui::Ui, title: &str, value: &str, color: Color32) {
    egui::Frame::none()
        .fill(Color32::from_rgb(16, 20, 28))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.6)))
        .rounding(12.0)
        .inner_margin(12.0)
        .show(ui, |ui: &mut egui::Ui| {
            ui.label(RichText::new(title).small().color(Color32::GRAY));
            ui.label(RichText::new(value).size(24.0).strong().color(color));
        });
}

fn badge(ui: &mut egui::Ui, text: &str, color: Color32) {
    egui::Frame::none()
        .fill(color.gamma_multiply(0.16))
        .stroke(Stroke::new(1.0, color))
        .rounding(999.0)
        .inner_margin(egui::Margin::symmetric(10.0, 6.0))
        .show(ui, |ui: &mut egui::Ui| {
            ui.label(RichText::new(text).strong().color(color));
        });
}

fn action_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(text).strong())
        .fill(Color32::from_rgb(255, 96, 64))
        .stroke(Stroke::new(1.0, Color32::from_rgb(255, 96, 64)))
        .rounding(12.0)
}

fn pill_button(text: &str, active: bool) -> egui::Button<'_> {
    egui::Button::new(RichText::new(text).strong())
        .fill(if active { Color32::from_rgb(255, 96, 64) } else { Color32::from_rgb(24, 28, 40) })
        .stroke(Stroke::new(1.0, if active { Color32::from_rgb(255, 96, 64) } else { Color32::from_rgb(44, 50, 68) }))
        .rounding(999.0)
}

fn send(req: Request) -> Result<Response> {
    let mut stream = UnixStream::connect(SOCKET_PATH).context("connect daemon")?;
    let payload = serde_json::to_string(&req)?;
    stream.write_all(payload.as_bytes())?;
    stream.write_all(b"\n")?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    Ok(serde_json::from_str(&line)?)
}
