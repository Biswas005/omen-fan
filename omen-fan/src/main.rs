use axum::{
    extract::{State, Json},
    response::IntoResponse,
    routing::{get, post},
    Router,
};

use serde::{Deserialize, Serialize};

use std::{net::SocketAddr, sync::{Arc, Mutex}, thread, time::Duration};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};

const EC_IO_FILE: &str = "/sys/kernel/debug/ec/ec0/io";
const FAN1_OFFSET: u64 = 0x34;
const FAN2_OFFSET: u64 = 0x35;
const CPU_TEMP_OFFSET: u64 = 0x57;
const GPU_TEMP_OFFSET: u64 = 0xB7;
const BIOS_CONTROL_OFFSET: u64 = 0x62;
const PERF_MODE_OFFSET: u64 = 0x95;
const TDP_OFFSET: u64 = 0xBA;

#[derive(Clone, Debug, Serialize, Deserialize)]
enum PowerMode {
    Balanced,
    Performance,
    PowerSaving,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct FanConfig {
    manual: bool,
    speed: u8, // 0–100%
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CurveConfig {
    temp_curve: Vec<u8>,
    speed_curve: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SystemStatus {
    cpu_temp: u8,
    gpu_temp: u8,
    fan1: u8,
    fan2: u8,
    power_mode: PowerMode,
}

#[tokio::main]
async fn main() {
    let state = Arc::new(Mutex::new(AppState {
        fan_config: FanConfig { manual: false, speed: 0 },
        power_mode: PowerMode::Balanced,
        curve_config: CurveConfig {
            temp_curve: vec![45, 55, 60, 70, 75, 80, 85, 93],
            speed_curve: vec![37, 45, 50, 60, 70, 80, 90, 100],
        },
    }));

    let cloned_state = Arc::clone(&state);
    thread::spawn(move || monitor_loop(cloned_state));

    let app = Router::new()
        .route("/status", get(get_status))
        .route("/fan", post(set_fan_config))
        .route("/mode", post(set_power_mode))
        .route("/curve", post(set_curve_config))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("Server running at http://{}", addr);
    axum::Server::bind(&addr).serve(app.into_make_service()).await.unwrap();
}

struct AppState {
    fan_config: FanConfig,
    power_mode: PowerMode,
    curve_config: CurveConfig,
}

async fn get_status(State(state): State<Arc<Mutex<AppState>>>) -> Json<SystemStatus> {
    let cpu_temp = read_ec(CPU_TEMP_OFFSET);
    let gpu_temp = read_ec(GPU_TEMP_OFFSET);
    let fan1 = read_ec(FAN1_OFFSET);
    let fan2 = read_ec(FAN2_OFFSET);
    let mode = state.lock().unwrap().power_mode.clone();
    Json(SystemStatus { cpu_temp, gpu_temp, fan1, fan2, power_mode: mode })
}

async fn set_fan_config(
    Json(config): Json<FanConfig>,
    State(state): State<Arc<Mutex<AppState>>>
) {
    state.lock().unwrap().fan_config = config;
}

async fn set_power_mode(
    Json(mode): Json<PowerMode>,
    State(state): State<Arc<Mutex<AppState>>>
) {
    let mut s = state.lock().unwrap();
    s.power_mode = mode.clone();
    match mode {
        PowerMode::Balanced => write_ec(PERF_MODE_OFFSET, 0x30),
        PowerMode::Performance => write_ec(PERF_MODE_OFFSET, 0x50),
        PowerMode::PowerSaving => {
            write_ec(PERF_MODE_OFFSET, 0x10);
            write_ec(TDP_OFFSET, 0x02); // Best effort due to reset
        }
    }
}

async fn set_curve_config(
    Json(curve): Json<CurveConfig>,
    State(state): State<Arc<Mutex<AppState>>>
) {
    state.lock().unwrap().curve_config = curve;
}

fn monitor_loop(state: Arc<Mutex<AppState>>) {
    loop {
        let mut lock = state.lock().unwrap();
        let temp = read_ec(CPU_TEMP_OFFSET).max(read_ec(GPU_TEMP_OFFSET));
        let fan_cfg = &lock.fan_config;
        let speed = if fan_cfg.manual {
            fan_cfg.speed
        } else {
            interpolate(temp, &lock.curve_config)
        };

        let pwm1 = (55 * speed) / 100;
        let pwm2 = (57 * speed) / 100;
        write_ec(FAN1_OFFSET, pwm1);
        write_ec(FAN2_OFFSET, pwm2);

        drop(lock);
        std::thread::sleep(Duration::from_secs(1));
    }
}

fn interpolate(temp: u8, curve: &CurveConfig) -> u8 {
    let (t, s) = (&curve.temp_curve, &curve.speed_curve);
    match temp {
        t0 if t0 <= t[0] => 0,
        t0 if t0 >= *t.last().unwrap() => *s.last().unwrap(),
        _ => {
            let i = t.iter().position(|&x| x > temp).unwrap();
            let (t0, t1, s0, s1) = (t[i-1], t[i], s[i-1], s[i]);
            s0 + ((s1 - s0) * (temp - t0) / (t1 - t0))
        }
    }
}

fn read_ec(offset: u64) -> u8 {
    let mut file = File::open(EC_IO_FILE).expect("Failed to open EC IO");
    file.seek(SeekFrom::Start(offset)).unwrap();
    let mut buf = [0u8; 1];
    file.read_exact(&mut buf).unwrap();
    buf[0]
}

fn write_ec(offset: u64, value: u8) {
    let mut file = OpenOptions::new().write(true).open(EC_IO_FILE).unwrap();
    file.seek(SeekFrom::Start(offset)).unwrap();
    file.write_all(&[value]).unwrap();
}
