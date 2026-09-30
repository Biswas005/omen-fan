//! Extra sensors and controls beyond the core fan/thermal loop: CPU
//! frequency/load/power, GPU telemetry via nvidia-smi, battery health,
//! and the optional convenience controls (charge limit, CPU boost,
//! keyboard/screen backlight, airplane mode, GPU power limit).
//!
//! Every read here returns `Option`/gracefully degrades instead of
//! erroring — this hardware varies a lot across boards/kernels/drivers,
//! and a missing sensor should just not show up in the UI, not crash
//! the daemon.

use anyhow::{bail, Context, Result};
use std::{
    path::{Path, PathBuf},
    process::Command,
    time::Instant,
};

// ---------- CPU frequency ----------

pub fn read_cpu_freq_mhz() -> Option<u32> {
    let mut total: u64 = 0;
    let mut count: u64 = 0;
    for entry in std::fs::read_dir("/sys/devices/system/cpu").ok()?.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let is_cpu_dir = name.starts_with("cpu")
            && name[3..].chars().all(|c| c.is_ascii_digit())
            && !name[3..].is_empty();
        if !is_cpu_dir {
            continue;
        }
        let path = entry.path().join("cpufreq/scaling_cur_freq");
        if let Ok(s) = std::fs::read_to_string(&path) {
            if let Ok(khz) = s.trim().parse::<u64>() {
                total += khz;
                count += 1;
            }
        }
    }
    (count > 0).then(|| ((total / count) / 1000) as u32)
}

// ---------- CPU load (needs state across ticks) ----------

#[derive(Default)]
pub struct CpuLoadTracker {
    prev_idle: u64,
    prev_total: u64,
    primed: bool,
}

impl CpuLoadTracker {
    pub fn sample(&mut self) -> Option<f32> {
        let content = std::fs::read_to_string("/proc/stat").ok()?;
        let line = content.lines().next()?;
        let fields: Vec<u64> = line.split_whitespace().skip(1).filter_map(|s| s.parse().ok()).collect();
        if fields.len() < 4 {
            return None;
        }
        let idle = fields[3] + fields.get(4).copied().unwrap_or(0);
        let total: u64 = fields.iter().sum();

        let result = if self.primed {
            let dt = total.saturating_sub(self.prev_total);
            let di = idle.saturating_sub(self.prev_idle);
            (dt > 0).then(|| (1.0 - (di as f32 / dt as f32)) * 100.0)
        } else {
            None
        };
        self.prev_idle = idle;
        self.prev_total = total;
        self.primed = true;
        result
    }
}

// ---------- CPU package power (RAPL — best effort, not guaranteed on AMD) ----------

#[derive(Default)]
pub struct RaplTracker {
    path: Option<PathBuf>,
    prev_uj: u64,
    prev_time: Option<Instant>,
}

impl RaplTracker {
    pub fn new() -> Self {
        let candidates = [
            "/sys/class/powercap/intel-rapl:0/energy_uj",
            "/sys/class/powercap/amd-rapl:0/energy_uj",
        ];
        let path = candidates.iter().map(PathBuf::from).find(|p| p.exists());
        Self { path, prev_uj: 0, prev_time: None }
    }

    pub fn supported(&self) -> bool {
        self.path.is_some()
    }

    pub fn sample(&mut self) -> Option<f32> {
        let path = self.path.as_ref()?;
        let uj: u64 = std::fs::read_to_string(path).ok()?.trim().parse().ok()?;
        let now = Instant::now();
        let result = self.prev_time.and_then(|prev| {
            let dt = now.duration_since(prev).as_secs_f32();
            if dt < 0.05 {
                return None;
            }
            let duj = uj.wrapping_sub(self.prev_uj) as f32;
            Some((duj / dt) / 1_000_000.0)
        });
        self.prev_uj = uj;
        self.prev_time = Some(now);
        result
    }
}

// ---------- GPU telemetry via nvidia-smi ----------

pub struct GpuTelemetry {
    pub temp_c: f32,
    pub hotspot_c: Option<f32>,
    pub power_w: f32,
}

fn run_nvidia_smi(args: &[&str]) -> Option<String> {
    let output = Command::new("nvidia-smi").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout).lines().next().map(|s| s.to_string())
}

/// Core temp + power in one call (always supported on NVIDIA); hotspot is
/// queried separately since it's only present on newer driver branches and
/// we don't want one unsupported field to break the whole reading.
pub fn read_gpu_telemetry() -> Option<GpuTelemetry> {
    let core = run_nvidia_smi(&["--query-gpu=temperature.gpu,power.draw", "--format=csv,noheader,nounits"])?;
    let parts: Vec<&str> = core.split(',').map(|s| s.trim()).collect();
    let temp_c: f32 = parts.first()?.parse().ok()?;
    let power_w: f32 = parts.get(1)?.parse().ok()?;
    let hotspot_c = run_nvidia_smi(&["--query-gpu=temperature.hotspot", "--format=csv,noheader,nounits"])
        .and_then(|s| s.trim().parse().ok());
    Some(GpuTelemetry { temp_c, hotspot_c, power_w })
}

pub fn read_gpu_power_limit_range() -> Option<(u32, u32)> {
    let min = run_nvidia_smi(&["--query-gpu=power.min_limit", "--format=csv,noheader,nounits"])?
        .trim().parse::<f32>().ok()? as u32;
    let max = run_nvidia_smi(&["--query-gpu=power.max_limit", "--format=csv,noheader,nounits"])?
        .trim().parse::<f32>().ok()? as u32;
    (max > min).then_some((min, max))
}

pub fn apply_gpu_power_limit(watts: u32) -> Result<()> {
    let status = Command::new("nvidia-smi")
        .args(["-pl", &watts.to_string()])
        .status()
        .context("run nvidia-smi -pl")?;
    if !status.success() {
        bail!("nvidia-smi -pl exited with a failure status");
    }
    Ok(())
}

// ---------- Battery percent + health ----------

fn battery_dir() -> Option<PathBuf> {
    let dir = std::fs::read_dir("/sys/class/power_supply").ok()?;
    for entry in dir.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("BAT") {
            return Some(entry.path());
        }
    }
    None
}

fn read_ratio(base: &Path, full: &str, design: &str) -> Option<f32> {
    let f: f32 = std::fs::read_to_string(base.join(full)).ok()?.trim().parse().ok()?;
    let d: f32 = std::fs::read_to_string(base.join(design)).ok()?.trim().parse().ok()?;
    (d > 0.0).then(|| (f / d) * 100.0)
}

pub fn read_battery() -> Option<(f32, Option<f32>)> {
    let base = battery_dir()?;
    let pct: f32 = std::fs::read_to_string(base.join("capacity")).ok()?.trim().parse().ok()?;
    let health = read_ratio(&base, "energy_full", "energy_full_design")
        .or_else(|| read_ratio(&base, "charge_full", "charge_full_design"));
    Some((pct, health))
}

pub fn charge_limit_path() -> Option<PathBuf> {
    let p = battery_dir()?.join("charge_control_end_threshold");
    p.exists().then_some(p)
}

pub fn read_charge_limit() -> Option<u8> {
    std::fs::read_to_string(charge_limit_path()?).ok()?.trim().parse().ok()
}

pub fn apply_charge_limit(pct: u8) -> Result<()> {
    let path = charge_limit_path().context("charge limit not supported on this battery")?;
    std::fs::write(path, pct.clamp(20, 100).to_string()).context("write charge_control_end_threshold")
}

// ---------- CPU boost ----------

pub fn cpu_boost_path() -> Option<PathBuf> {
    let p = PathBuf::from("/sys/devices/system/cpu/cpufreq/boost");
    p.exists().then_some(p)
}

pub fn read_cpu_boost() -> Option<bool> {
    std::fs::read_to_string(cpu_boost_path()?).ok().map(|s| s.trim() == "1")
}

pub fn apply_cpu_boost(enabled: bool) -> Result<()> {
    let path = cpu_boost_path().context("CPU boost control not supported on this kernel")?;
    std::fs::write(path, if enabled { "1" } else { "0" }).context("write cpufreq/boost")
}

// ---------- Keyboard backlight ----------

pub fn kbd_backlight_path() -> Option<PathBuf> {
    let dir = std::fs::read_dir("/sys/class/leds").ok()?;
    for entry in dir.flatten() {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if name.contains("kbd_backlight") {
            return Some(entry.path());
        }
    }
    None
}

pub fn read_kbd_backlight_pct() -> Option<u8> {
    let path = kbd_backlight_path()?;
    let max: u32 = std::fs::read_to_string(path.join("max_brightness")).ok()?.trim().parse().ok()?;
    let cur: u32 = std::fs::read_to_string(path.join("brightness")).ok()?.trim().parse().ok()?;
    (max > 0).then(|| ((cur * 100) / max) as u8)
}

pub fn apply_kbd_backlight(pct: u8) -> Result<()> {
    let path = kbd_backlight_path().context("keyboard backlight not detected on this board")?;
    let max: u32 = std::fs::read_to_string(path.join("max_brightness"))
        .context("read max_brightness")?
        .trim()
        .parse()
        .context("parse max_brightness")?;
    let value = ((pct.clamp(0, 100) as u32 * max) / 100).to_string();
    std::fs::write(path.join("brightness"), value).context("write brightness")
}

// ---------- Screen brightness ----------

pub fn backlight_path() -> Option<PathBuf> {
    std::fs::read_dir("/sys/class/backlight").ok()?.flatten().next().map(|e| e.path())
}

pub fn read_screen_brightness_pct() -> Option<u8> {
    let path = backlight_path()?;
    let max: u32 = std::fs::read_to_string(path.join("max_brightness")).ok()?.trim().parse().ok()?;
    let cur: u32 = std::fs::read_to_string(path.join("brightness")).ok()?.trim().parse().ok()?;
    (max > 0).then(|| ((cur * 100) / max) as u8)
}

pub fn apply_screen_brightness(pct: u8) -> Result<()> {
    let path = backlight_path().context("no backlight device detected")?;
    let max: u32 = std::fs::read_to_string(path.join("max_brightness"))
        .context("read max_brightness")?
        .trim()
        .parse()
        .context("parse max_brightness")?;
    let value = ((pct.clamp(1, 100) as u32 * max) / 100).to_string();
    std::fs::write(path.join("brightness"), value).context("write brightness")
}

// ---------- Airplane mode (rfkill soft-block, all radios) ----------

pub fn read_airplane_mode() -> Option<bool> {
    let mut any = false;
    let mut all_blocked = true;
    for entry in std::fs::read_dir("/sys/class/rfkill").ok()?.flatten() {
        any = true;
        let soft = std::fs::read_to_string(entry.path().join("soft")).ok()?;
        if soft.trim() != "1" {
            all_blocked = false;
        }
    }
    any.then_some(all_blocked)
}

pub fn apply_airplane_mode(enabled: bool) -> Result<()> {
    let dir = std::fs::read_dir("/sys/class/rfkill").context("read /sys/class/rfkill")?;
    let mut touched = false;
    for entry in dir.flatten() {
        let soft_path = entry.path().join("soft");
        if soft_path.exists() {
            let _ = std::fs::write(soft_path, if enabled { "1" } else { "0" });
            touched = true;
        }
    }
    if !touched {
        bail!("no rfkill devices found");
    }
    Ok(())
}
