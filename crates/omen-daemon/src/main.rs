use anyhow::{Context, Result};
use chrono::Utc;
use omen_core::*;
use parking_lot::Mutex;
use rusqlite::{params, Connection};
use std::{
    collections::VecDeque,
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::Duration,
};
use tracing::{error, info, warn};

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let daemon = Arc::new(Mutex::new(Daemon::new(PathBuf::from(DB_PATH))?));
    daemon.lock().apply_startup()?;

    let worker = daemon.clone();
    thread::spawn(move || loop {
        let sleep_ms = {
            let mut d = worker.lock();
            if let Err(err) = d.tick() {
                error!("tick failed: {err:#}");
            }
            d.state.poll_interval_ms.max(250)
        };
        thread::sleep(Duration::from_millis(sleep_ms));
    });

    serve(daemon)
}

fn serve(daemon: Arc<Mutex<Daemon>>) -> Result<()> {
    let path = Path::new(SOCKET_PATH);
    if path.exists() {
        let _ = fs::remove_file(path);
    }
    let listener = UnixListener::bind(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o666)).ok();
    info!("listening on {}", path.display());

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let daemon = daemon.clone();
                thread::spawn(move || {
                    if let Err(err) = handle_client(stream, daemon) {
                        error!("client: {err:#}");
                    }
                });
            }
            Err(err) => warn!("accept failed: {err}"),
        }
    }
    Ok(())
}

fn handle_client(mut stream: UnixStream, daemon: Arc<Mutex<Daemon>>) -> Result<()> {
    let mut line = String::new();
    BufReader::new(stream.try_clone()?).read_line(&mut line)?;
    if line.trim().is_empty() {
        return Ok(());
    }
    let req: Request = serde_json::from_str(&line)?;
    let resp = daemon.lock().handle(req);
    stream.write_all(serde_json::to_string(&resp)?.as_bytes())?;
    stream.write_all(b"\n")?;
    Ok(())
}

struct Daemon {
    db_path: PathBuf,
    state: AppState,
    hw: Hardware,
    applied_duty_pct: f32,
    temp_history: VecDeque<f32>,
    telemetry_history: VecDeque<TelemetryPoint>,
    power_source: PowerSource,
    max_fan_active: bool,
    graphics_mode: Option<GraphicsMode>,
}

impl Daemon {
    fn new(db_path: PathBuf) -> Result<Self> {
        let hw = Hardware::detect();
        let state = load_or_init_db(&db_path, &hw)?;
        Ok(Self {
            db_path,
            state,
            hw,
            applied_duty_pct: 0.0,
            temp_history: VecDeque::with_capacity(8),
            telemetry_history: VecDeque::with_capacity(240),
            power_source: PowerSource::Unknown,
            max_fan_active: false,
            graphics_mode: None,
        })
    }

    fn apply_startup(&mut self) -> Result<()> {
        self.apply_power_behavior()?;
        let profile = self.state.active().clone();
        self.apply_profile(&profile)
    }

    fn tick(&mut self) -> Result<()> {
        self.apply_power_behavior()?;
        let profile = self.state.active().clone();
        let raw = self.hw.read_temp_c().unwrap_or(0.0);
        self.temp_history.push_back(raw);
        while self.temp_history.len() > profile.tuning.moving_average_window.max(1) {
            self.temp_history.pop_front();
        }
        let avg = self.avg_temp();

        if profile.max_fan || profile.fan_mode == FanMode::Max {
            self.hw.set_max_fan(true)?;
            self.max_fan_active = true;
            self.applied_duty_pct = 100.0;
        } else {
            match profile.fan_mode {
                FanMode::Auto => {
                    self.hw.set_auto()?;
                    self.max_fan_active = false;
                    self.applied_duty_pct = 0.0;
                }
                FanMode::Manual => {
                    self.hw.set_max_fan(false)?;
                    self.max_fan_active = false;
                    self.apply_target(profile.manual_pct as f32, &profile)?;
                }
                FanMode::Curve => {
                    self.hw.set_max_fan(false)?;
                    self.max_fan_active = false;
                    let target = self.compute_curve_target(&profile, raw, avg);
                    self.apply_target(target, &profile)?;
                }
                FanMode::Max => unreachable!(),
            }
        }

        self.push_history(raw);
        Ok(())
    }

    fn avg_temp(&self) -> f32 {
        if self.temp_history.is_empty() {
            0.0
        } else {
            self.temp_history.iter().copied().sum::<f32>() / self.temp_history.len() as f32
        }
    }

    fn compute_curve_target(&self, profile: &Profile, raw: f32, avg: f32) -> f32 {
        let t = &profile.tuning;
        if raw >= t.critical_temp_c || avg >= t.critical_temp_c {
            return 100.0;
        }
        if self.applied_duty_pct <= 0.0 && avg < t.on_temp_c {
            return 0.0;
        }
        if self.applied_duty_pct > 0.0 && avg < t.off_temp_c {
            return 0.0;
        }

        let mut target = profile.curve.duty_for_temp(avg.clamp(45.0, 90.0));
        if target > 0.0 {
            target = target.max(t.min_spin_pct);
        }
        quantize_pct(target, t.levels)
    }

    fn apply_target(&mut self, target: f32, profile: &Profile) -> Result<()> {
        let t = &profile.tuning;
        let current = self.applied_duty_pct;
        let delta = target - current;
        if delta.abs() < 0.5 {
            return Ok(());
        }
        let next = if delta > 0.0 {
            current + delta.min(t.ramp_up_pct_per_tick)
        } else {
            current + delta.max(-t.ramp_down_pct_per_tick)
        };
        let applied = quantize_pct(next.clamp(0.0, 100.0), t.levels);
        if (applied - self.applied_duty_pct).abs() >= 0.5 {
            self.hw.set_manual_pct(applied)?;
            self.applied_duty_pct = applied;
        }
        Ok(())
    }

    fn apply_power_behavior(&mut self) -> Result<()> {
        let current = self.hw.read_power_source();
        if current == self.power_source {
            return Ok(());
        }
        self.power_source = current.clone();
        if !self.state.battery_behavior.enabled {
            return Ok(());
        }
        match current {
            PowerSource::Battery => {
                self.state.last_ac_profile = self.state.active_profile.clone();
                // Switch to Quiet profile on battery
                let fallback = self.state.profiles.iter()
                    .find(|p| p.name.to_lowercase() == "quiet")
                    .or_else(|| self.state.profiles.first())
                    .map(|p| p.clone());
                if let Some(mut profile) = fallback {
                    self.state.active_profile = profile.name.clone();
                    // Override platform mode with the selected battery mode
                    if let Some(battery_mode) = &self.state.battery_behavior.battery_mode {
                        profile.platform_mode = Some(battery_mode.clone());
                    }
                    self.apply_profile(&profile)?;
                }
                save_state_db(&self.db_path, &self.state)?;
            }
            PowerSource::AC => {
                if self.state.battery_behavior.restore_ac_profile {
                    let restore = self.state.last_ac_profile.clone();
                    if self.state.profile(&restore).is_some() {
                        self.state.active_profile = restore;
                    }
                }
                let profile = self.state.active().clone();
                self.apply_profile(&profile)?;
                save_state_db(&self.db_path, &self.state)?;
            }
            PowerSource::Unknown => {}
        }
        Ok(())
    }

    fn apply_profile(&mut self, profile: &Profile) -> Result<()> {
        let _ = self.hw.set_platform_mode(profile.platform_mode.as_deref());
        if let Some(mode) = &profile.graphics_mode {
            let _ = self.hw.set_graphics_mode(mode.clone());
            self.graphics_mode = Some(mode.clone());
        }
        if profile.max_fan || profile.fan_mode == FanMode::Max {
            self.hw.set_max_fan(true)?;
            self.max_fan_active = true;
            self.applied_duty_pct = 100.0;
            return Ok(());
        }
        match profile.fan_mode {
            FanMode::Auto => {
                self.hw.set_auto()?;
                self.max_fan_active = false;
                self.applied_duty_pct = 0.0;
            }
            FanMode::Manual => {
                self.hw.set_max_fan(false)?;
                self.max_fan_active = false;
                self.hw.set_manual_pct(profile.manual_pct as f32)?;
                self.applied_duty_pct = profile.manual_pct as f32;
            }
            FanMode::Curve => {
                self.hw.set_max_fan(false)?;
                self.max_fan_active = false;
            }
            FanMode::Max => unreachable!(),
        }
        Ok(())
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            state: self.state.clone(),
            live: LiveTelemetry {
                cpu_temp_c: self.hw.read_temp_c().unwrap_or(0.0),
                rpm: self.hw.read_rpm().unwrap_or(0),
                duty_pct: self.applied_duty_pct,
                avg_temp_c: self.avg_temp(),
                power_source: self.hw.read_power_source(),
                current_platform_mode: self.hw.read_platform_mode(),
                max_fan_active: self.max_fan_active,
                graphics_mode: self.hw.read_graphics_mode().or_else(|| self.graphics_mode.clone()),
                history: self.telemetry_history.iter().cloned().collect(),
            },
        }
    }

    fn push_history(&mut self, raw_temp: f32) {
        let rpm = self.hw.read_rpm().unwrap_or(0);
        self.telemetry_history.push_back(TelemetryPoint {
            ts: Utc::now(),
            temp_c: raw_temp,
            rpm,
            duty_pct: self.applied_duty_pct,
        });
        while self.telemetry_history.len() > 180 {
            self.telemetry_history.pop_front();
        }
    }

    fn handle(&mut self, req: Request) -> Response {
        let res = (|| -> Result<Snapshot> {
            match req {
                Request::GetSnapshot => {}
                Request::SetActiveProfile { name } => {
                    anyhow::ensure!(self.state.profile(&name).is_some(), "unknown profile");
                    self.state.active_profile = name.clone();
                    if self.hw.read_power_source() == PowerSource::AC {
                        self.state.last_ac_profile = name;
                    }
                    let p = self.state.active().clone();
                    self.apply_profile(&p)?;
                    save_state_db(&self.db_path, &self.state)?;
                }
                Request::SetPlatformMode { profile, mode } => {
                    let p = self.state.profile_mut(&profile).context("unknown profile")?;
                    p.platform_mode = mode.clone();
                    if self.state.active_profile == profile {
                        let _ = self.hw.set_platform_mode(mode.as_deref());
                    }
                    save_state_db(&self.db_path, &self.state)?;
                }
                Request::SetFanMode { profile, mode } => {
                    let p = self.state.profile_mut(&profile).context("unknown profile")?;
                    p.fan_mode = mode.clone();
                    if mode != FanMode::Max {
                        p.max_fan = false;
                    }
                    if self.state.active_profile == profile {
                        let p = self.state.active().clone();
                        self.apply_profile(&p)?;
                    }
                    save_state_db(&self.db_path, &self.state)?;
                }
                Request::SetManualDuty { profile, duty_pct } => {
                    let fan_mode = {
                        let p = self.state.profile_mut(&profile).context("unknown profile")?;
                        p.manual_pct = duty_pct.clamp(25, 100);
                        p.fan_mode
                    };
                    if self.state.active_profile == profile && fan_mode == FanMode::Manual {
                        let p = self.state.profile_mut(&profile).context("unknown profile")?;
                        self.hw.set_max_fan(false)?;
                        self.hw.set_manual_pct(p.manual_pct as f32)?;
                        self.applied_duty_pct = p.manual_pct as f32;
                    }
                    save_state_db(&self.db_path, &self.state)?;
                }
                Request::SetCurve { profile, mut curve } => {
                    curve.normalize();
                    let p = self.state.profile_mut(&profile).context("unknown profile")?;
                    p.curve = curve;
                    save_state_db(&self.db_path, &self.state)?;
                }
                Request::SetMaxFan { profile, enabled } => {
                    let p = self.state.profile_mut(&profile).context("unknown profile")?;
                    p.max_fan = enabled;
                    if self.state.active_profile == profile {
                        let p = self.state.active().clone();
                        self.apply_profile(&p)?;
                    }
                    save_state_db(&self.db_path, &self.state)?;
                }
                Request::SetGraphicsMode { profile, mode } => {
                    let p = self.state.profile_mut(&profile).context("unknown profile")?;
                    p.graphics_mode = mode.clone();
                    if self.state.active_profile == profile {
                        if let Some(mode) = mode {
                            let _ = self.hw.set_graphics_mode(mode.clone());
                            self.graphics_mode = Some(mode);
                        }
                    }
                    save_state_db(&self.db_path, &self.state)?;
                }
                Request::SetBatteryBehavior { enabled, battery_mode, restore_ac_profile } => {
                    self.state.battery_behavior.enabled = enabled;
                    self.state.battery_behavior.battery_mode = battery_mode;
                    self.state.battery_behavior.restore_ac_profile = restore_ac_profile;
                    save_state_db(&self.db_path, &self.state)?;
                }
                Request::Reload => {
                    self.state = load_or_init_db(&self.db_path, &self.hw)?;
                }
            }
            Ok(self.snapshot())
        })();

        match res {
            Ok(s) => Response::Snapshot(s),
            Err(err) => Response::Error {
                message: format!("{err:#}"),
            },
        }
    }
}

#[derive(Clone)]
struct Hardware {
    caps: Capabilities,
    pwm_enable: Option<PathBuf>,
    pwm: Option<PathBuf>,
    rpm: Option<PathBuf>,
    temp: Option<PathBuf>,
    platform_profile: Option<PathBuf>,
    graphics_mode: Option<PathBuf>,
    graphics_modes: Vec<GraphicsMode>,
    max_fan: Option<PathBuf>,
    power_online: Option<PathBuf>,
}

impl Hardware {
    fn detect() -> Self {
        let hwmon = detect_hwmon();
        let temp = detect_temp();
        let platform_profile = PathBuf::from("/sys/firmware/acpi/platform_profile");
        let platform_choices = PathBuf::from("/sys/firmware/acpi/platform_profile_choices");
        let graphics_mode = PathBuf::from("/sys/devices/platform/hp-wmi/graphics_mode");
        let graphics_modes_path = PathBuf::from("/sys/devices/platform/hp-wmi/graphics_modes");
        let max_fan = PathBuf::from("/sys/devices/platform/hp-wmi/fan_speed_max");
        let board_name = fs::read_to_string("/sys/devices/platform/hp-wmi/board_name")
            .unwrap_or_else(|_| "HP Omen".into())
            .trim()
            .to_string();
        let board_id = fs::read_to_string("/sys/devices/platform/hp-wmi/board_id")
            .ok()
            .map(|s| s.trim().to_string());
        let platform_modes = if let Ok(text) = fs::read_to_string(&platform_choices) {
            text.split_whitespace()
                .map(|t| t.trim_matches(['[', ']']))
                .filter(|s| !s.is_empty())
                .map(PlatformMode::new)
                .collect()
        } else {
            vec![]
        };
        let graphics_modes = if let Ok(text) = fs::read_to_string(&graphics_modes_path) {
            parse_graphics_modes(&text)
        } else {
            vec![]
        };
        let caps = Capabilities {
            board_name,
            board_id,
            platform_modes,
            graphics_modes: graphics_modes.clone(),
            supports_max_fan: max_fan.exists(),
            supports_graphics_mode: graphics_mode.exists(),
            pwm_path: hwmon.as_ref().map(|h| h.join("pwm1").display().to_string()),
            rpm_path: hwmon.as_ref().map(|h| h.join("fan1_input").display().to_string()),
            temp_path: temp.as_ref().map(|p| p.display().to_string()),
        };
        let power_online = detect_power();
        Self {
            caps,
            pwm_enable: hwmon.as_ref().map(|h| h.join("pwm1_enable")).filter(|p| p.exists()),
            pwm: hwmon.as_ref().map(|h| h.join("pwm1")).filter(|p| p.exists()),
            rpm: hwmon.as_ref().map(|h| h.join("fan1_input")).filter(|p| p.exists()),
            temp,
            platform_profile: platform_profile.exists().then_some(platform_profile),
            graphics_mode: graphics_mode.exists().then_some(graphics_mode),
            graphics_modes,
            max_fan: max_fan.exists().then_some(max_fan),
            power_online,
        }
    }

    fn read_temp_c(&self) -> Option<f32> {
        let s = fs::read_to_string(self.temp.as_ref()?).ok()?;
        let v = s.trim().parse::<f32>().ok()?;
        Some(if v > 1000.0 { v / 1000.0 } else { v })
    }

    fn read_rpm(&self) -> Option<u32> {
        fs::read_to_string(self.rpm.as_ref()?).ok()?.trim().parse().ok()
    }

    fn read_power_source(&self) -> PowerSource {
        match &self.power_online {
            Some(p) => match fs::read_to_string(p) {
                Ok(v) if v.trim() == "1" => PowerSource::AC,
                Ok(v) if v.trim() == "0" => PowerSource::Battery,
                _ => PowerSource::Unknown,
            },
            None => PowerSource::Unknown,
        }
    }

    fn set_auto(&self) -> Result<()> {
        if let Some(p) = &self.pwm_enable {
            fs::write(p, b"2")?;
        }
        Ok(())
    }

    fn set_manual_pct(&self, pct: f32) -> Result<()> {
        let _ = self.set_max_fan(false);
        if let Some(p) = &self.pwm_enable {
            fs::write(p, b"1")?;
        }
        if let Some(p) = &self.pwm {
            let pwm = ((pct.clamp(0.0, 100.0) / 100.0) * 255.0).round() as u8;
            fs::write(p, pwm.to_string())?;
        }
        Ok(())
    }

    fn set_max_fan(&self, enabled: bool) -> Result<()> {
        if let Some(p) = &self.max_fan {
            fs::write(p, if enabled { "0" } else { "1" })?;
        }
        Ok(())
    }

    fn read_platform_mode(&self) -> Option<String> {
        fs::read_to_string(self.platform_profile.as_ref()?)
            .ok()
            .map(|s| s.trim().to_string())
    }

    fn set_platform_mode(&self, mode: Option<&str>) -> Result<()> {
        let Some(mode) = mode else { return Ok(()); };
        let Some(path) = &self.platform_profile else { return Ok(()); };
        anyhow::ensure!(
            self.caps.platform_modes.iter().any(|m| m.key == mode),
            "platform profile '{mode}' not supported on this machine"
        );
        if self.read_platform_mode().as_deref() == Some(mode) {
            return Ok(());
        }
        fs::write(path, mode)?;
        Ok(())
    }

    fn set_graphics_mode(&self, mode: GraphicsMode) -> Result<()> {
        let Some(path) = &self.graphics_mode else { return Ok(()); };
        let wanted = match mode {
            GraphicsMode::Hybrid => "hybrid".to_string(),
            GraphicsMode::Discrete => "discrete".to_string(),
            GraphicsMode::Optimus => "optimus".to_string(),
            GraphicsMode::Custom(v) => v,
        };
        anyhow::ensure!(self.graphics_modes.iter().any(|m| match m {
            GraphicsMode::Hybrid => wanted == "hybrid",
            GraphicsMode::Discrete => wanted == "discrete",
            GraphicsMode::Optimus => wanted == "optimus",
            GraphicsMode::Custom(v) => *v == wanted,
        }), "graphics mode not supported");
        fs::write(path, wanted)?;
        Ok(())
    }

    fn read_graphics_mode(&self) -> Option<GraphicsMode> {
        let raw = fs::read_to_string(self.graphics_mode.as_ref()?).ok()?;
        Some(match raw.trim() {
            "hybrid" => GraphicsMode::Hybrid,
            "discrete" => GraphicsMode::Discrete,
            "optimus" => GraphicsMode::Optimus,
            other => GraphicsMode::Custom(other.to_string()),
        })
    }
}

fn parse_graphics_modes(text: &str) -> Vec<GraphicsMode> {
    let mut out = Vec::new();
    for token in text.split_whitespace() {
        let t = token.trim_matches(['[', ']']);
        let m = match t {
            "hybrid" => GraphicsMode::Hybrid,
            "discrete" => GraphicsMode::Discrete,
            "optimus" => GraphicsMode::Optimus,
            other => GraphicsMode::Custom(other.to_string()),
        };
        if !out.contains(&m) {
            out.push(m);
        }
    }
    out
}

fn detect_hwmon() -> Option<PathBuf> {
    let base = Path::new("/sys/devices/platform/hp-wmi/hwmon");
    fs::read_dir(base)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.join("pwm1").exists())
}

fn detect_temp() -> Option<PathBuf> {
    let base = Path::new("/sys/class/hwmon");
    for entry in fs::read_dir(base).ok()?.flatten() {
        let hw = entry.path();
        let name = fs::read_to_string(hw.join("name")).unwrap_or_default();
        if ["k10temp", "coretemp", "zenpower"].contains(&name.trim()) {
            for i in 1..=10 {
                let p = hw.join(format!("temp{}_input", i));
                if p.exists() {
                    return Some(p);
                }
            }
        }
    }
    let fallback = PathBuf::from("/sys/class/thermal/thermal_zone0/temp");
    fallback.exists().then_some(fallback)
}

fn detect_power() -> Option<PathBuf> {
    for entry in fs::read_dir("/sys/class/power_supply").ok()?.flatten() {
        let p = entry.path();
        if fs::read_to_string(p.join("type")).ok()?.trim() == "Mains" {
            let online = p.join("online");
            if online.exists() {
                return Some(online);
            }
        }
    }
    None
}

fn load_or_init_db(path: &Path, hw: &Hardware) -> Result<AppState> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS app_state (id INTEGER PRIMARY KEY CHECK(id=1), json TEXT NOT NULL)",
        [],
    )?;
    let json: Option<String> = conn
        .query_row("SELECT json FROM app_state WHERE id=1", [], |r| r.get(0))
        .ok();
    if let Some(json) = json {
        let mut state: AppState = serde_json::from_str(&json)?;
        state.capabilities = hw.caps.clone();
        return Ok(state);
    }
    let state = AppState::default_with_caps(hw.caps.clone());
    save_state_db(path, &state)?;
    Ok(state)
}

fn save_state_db(path: &Path, state: &AppState) -> Result<()> {
    let conn = Connection::open(path)?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS app_state (id INTEGER PRIMARY KEY CHECK(id=1), json TEXT NOT NULL)",
        [],
    )?;
    let json = serde_json::to_string_pretty(state)?;
    conn.execute(
        "INSERT INTO app_state(id, json) VALUES(1, ?1) ON CONFLICT(id) DO UPDATE SET json=excluded.json",
        params![json],
    )?;
    Ok(())
}
