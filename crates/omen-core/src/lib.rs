use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const SOCKET_PATH: &str = "/run/omen-fand.sock";
pub const DB_PATH: &str = "/var/lib/omen-fand/state.db";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct CurvePoint {
    pub temp_c: f32,
    pub duty_pct: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FanCurve {
    pub points: Vec<CurvePoint>,
}

impl Default for FanCurve {
    fn default() -> Self {
        Self {
            points: vec![
                CurvePoint { temp_c: 45.0, duty_pct: 38.0 },
                CurvePoint { temp_c: 50.0, duty_pct: 42.0 },
                CurvePoint { temp_c: 55.0, duty_pct: 48.0 },
                CurvePoint { temp_c: 60.0, duty_pct: 56.0 },
                CurvePoint { temp_c: 65.0, duty_pct: 64.0 },
                CurvePoint { temp_c: 70.0, duty_pct: 72.0 },
                CurvePoint { temp_c: 75.0, duty_pct: 80.0 },
                CurvePoint { temp_c: 82.0, duty_pct: 88.0 },
                CurvePoint { temp_c: 90.0, duty_pct: 100.0 },
            ],
        }
    }
}

impl FanCurve {
    pub fn normalize(&mut self) {
        self.points.sort_by(|a, b| a.temp_c.partial_cmp(&b.temp_c).unwrap());
        if self.points.is_empty() {
            self.points = Self::default().points;
        }
        for p in &mut self.points {
            p.temp_c = p.temp_c.clamp(45.0, 90.0);
            p.duty_pct = p.duty_pct.clamp(0.0, 100.0);
        }
        self.points[0].temp_c = 45.0;
        self.points[0].duty_pct = self.points[0].duty_pct.max(38.0);
        let last = self.points.len() - 1;
        self.points[last].temp_c = 90.0;
        self.points[last].duty_pct = 100.0;
        for i in 1..self.points.len() {
            if self.points[i].temp_c <= self.points[i - 1].temp_c {
                self.points[i].temp_c = (self.points[i - 1].temp_c + 1.0).min(90.0);
            }
            if self.points[i].duty_pct < self.points[i - 1].duty_pct {
                self.points[i].duty_pct = self.points[i - 1].duty_pct;
            }
        }
    }

    pub fn duty_for_temp(&self, temp_c: f32) -> f32 {
        if self.points.is_empty() {
            return 0.0;
        }
        let t = temp_c.clamp(45.0, 90.0);
        if t <= self.points[0].temp_c {
            return self.points[0].duty_pct;
        }
        for pair in self.points.windows(2) {
            let a = pair[0];
            let b = pair[1];
            if t >= a.temp_c && t <= b.temp_c {
                let denom = (b.temp_c - a.temp_c).max(0.001);
                let ratio = (t - a.temp_c) / denom;
                return a.duty_pct + ratio * (b.duty_pct - a.duty_pct);
            }
        }
        self.points.last().map(|p| p.duty_pct).unwrap_or(100.0)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum FanMode {
    Auto,
    Curve,
    Manual,
    Max,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PowerSource {
    AC,
    Battery,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum GraphicsMode {
    Hybrid,
    Discrete,
    Optimus,
    Custom(String),
}

impl GraphicsMode {
    pub fn label(&self) -> &str {
        match self {
            Self::Hybrid => "Hybrid",
            Self::Discrete => "Discrete",
            Self::Optimus => "Optimus",
            Self::Custom(v) => v,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlatformMode {
    pub key: String,
    pub label: String,
}

impl PlatformMode {
    pub fn new<S: Into<String>>(value: S) -> Self {
        let key = value.into();
        let label = key
            .split('-')
            .map(|part| {
                let mut chars = part.chars();
                chars
                    .next()
                    .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" ");
        Self { key, label }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlTuning {
    pub moving_average_window: usize,
    pub ramp_up_pct_per_tick: f32,
    pub ramp_down_pct_per_tick: f32,
    pub on_temp_c: f32,
    pub off_temp_c: f32,
    pub critical_temp_c: f32,
    pub min_spin_pct: f32,
    pub levels: u8,
}

impl Default for ControlTuning {
    fn default() -> Self {
        Self {
            moving_average_window: 5,
            ramp_up_pct_per_tick: 24.0,
            ramp_down_pct_per_tick: 9.0,
            on_temp_c: 45.0,
            off_temp_c: 43.0,
            critical_temp_c: 95.0,
            min_spin_pct: 38.0,
            levels: 15,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub fan_mode: FanMode,
    pub manual_pct: u8,
    pub platform_mode: Option<String>,
    pub graphics_mode: Option<GraphicsMode>,
    pub max_fan: bool,
    pub curve: FanCurve,
    pub tuning: ControlTuning,
}

impl Profile {
    pub fn balanced() -> Self {
        Self {
            name: "Balanced".into(),
            fan_mode: FanMode::Curve,
            manual_pct: 50,
            platform_mode: Some("balanced".into()),
            graphics_mode: None,
            max_fan: false,
            curve: FanCurve::default(),
            tuning: ControlTuning::default(),
        }
    }

    pub fn performance() -> Self {
        let mut p = Self::balanced();
        p.name = "Performance".into();
        p.platform_mode = Some("performance".into());
        p.curve.points = vec![
            CurvePoint { temp_c: 45.0, duty_pct: 38.0 },
            CurvePoint { temp_c: 48.0, duty_pct: 44.0 },
            CurvePoint { temp_c: 52.0, duty_pct: 52.0 },
            CurvePoint { temp_c: 56.0, duty_pct: 60.0 },
            CurvePoint { temp_c: 62.0, duty_pct: 70.0 },
            CurvePoint { temp_c: 68.0, duty_pct: 80.0 },
            CurvePoint { temp_c: 75.0, duty_pct: 88.0 },
            CurvePoint { temp_c: 90.0, duty_pct: 100.0 },
        ];
        p.tuning.ramp_up_pct_per_tick = 28.0;
        p.tuning.ramp_down_pct_per_tick = 10.0;
        p
    }

    pub fn quiet() -> Self {
        let mut p = Self::balanced();
        p.name = "Quiet".into();
        p.platform_mode = Some("low-power".into());
        p.curve.points = vec![
            CurvePoint { temp_c: 45.0, duty_pct: 38.0 },
            CurvePoint { temp_c: 55.0, duty_pct: 40.0 },
            CurvePoint { temp_c: 62.0, duty_pct: 46.0 },
            CurvePoint { temp_c: 70.0, duty_pct: 58.0 },
            CurvePoint { temp_c: 80.0, duty_pct: 74.0 },
            CurvePoint { temp_c: 90.0, duty_pct: 100.0 },
        ];
        p.tuning.ramp_down_pct_per_tick = 8.0;
        p
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatteryBehavior {
    pub enabled: bool,
    pub battery_mode: Option<String>,
    pub restore_ac_profile: bool,
}

impl Default for BatteryBehavior {
    fn default() -> Self {
        Self {
            enabled: true,
            battery_mode: Some("balanced".into()),
            restore_ac_profile: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capabilities {
    pub board_name: String,
    pub board_id: Option<String>,
    pub platform_modes: Vec<PlatformMode>,
    pub graphics_modes: Vec<GraphicsMode>,
    pub supports_max_fan: bool,
    pub supports_graphics_mode: bool,
    pub pwm_path: Option<String>,
    pub rpm_path: Option<String>,
    pub temp_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppState {
    pub active_profile: String,
    pub last_ac_profile: String,
    pub profiles: Vec<Profile>,
    pub battery_behavior: BatteryBehavior,
    pub capabilities: Capabilities,
    pub poll_interval_ms: u64,
}

impl AppState {
    pub fn default_with_caps(caps: Capabilities) -> Self {
        let battery_mode = if caps.platform_modes.iter().any(|m| m.key == "low-power") {
            Some("low-power".into())
        } else if caps.platform_modes.iter().any(|m| m.key == "balanced") {
            Some("balanced".into())
        } else {
            caps.platform_modes.first().map(|m| m.key.clone())
        };
        Self {
            active_profile: "Balanced".into(),
            last_ac_profile: "Performance".into(),
            profiles: vec![Profile::quiet(), Profile::balanced(), Profile::performance()],
            battery_behavior: BatteryBehavior {
                battery_mode,
                ..Default::default()
            },
            capabilities: caps,
            poll_interval_ms: 500,
        }
    }

    pub fn profile(&self, name: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.name == name)
    }

    pub fn profile_mut(&mut self, name: &str) -> Option<&mut Profile> {
        self.profiles.iter_mut().find(|p| p.name == name)
    }

    pub fn active(&self) -> &Profile {
        self.profile(&self.active_profile)
            .unwrap_or_else(|| self.profiles.first().expect("profiles"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryPoint {
    pub ts: DateTime<Utc>,
    pub temp_c: f32,
    pub rpm: u32,
    pub duty_pct: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveTelemetry {
    pub cpu_temp_c: f32,
    pub rpm: u32,
    pub duty_pct: f32,
    pub avg_temp_c: f32,
    pub power_source: PowerSource,
    pub current_platform_mode: Option<String>,
    pub max_fan_active: bool,
    pub graphics_mode: Option<GraphicsMode>,
    pub history: Vec<TelemetryPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub state: AppState,
    pub live: LiveTelemetry,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    #[serde(alias = "GetState")]
    GetSnapshot,
    SetActiveProfile { name: String },
    SetPlatformMode { profile: String, mode: Option<String> },
    SetFanMode { profile: String, mode: FanMode },
    SetManualDuty { profile: String, duty_pct: u8 },
    SetCurve { profile: String, curve: FanCurve },
    SetMaxFan { profile: String, enabled: bool },
    SetGraphicsMode { profile: String, mode: Option<GraphicsMode> },
    SetBatteryBehavior { enabled: bool, battery_mode: Option<String>, restore_ac_profile: bool },
    Reload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Response {
    Snapshot(Snapshot),
    Error { message: String },
}

pub fn quantize_pct(v: f32, steps: u8) -> f32 {
    if v <= 0.0 {
        return 0.0;
    }
    let levels = steps.max(2) as f32;
    let bucket = 100.0 / (levels - 1.0);
    (v / bucket).round() * bucket
}
