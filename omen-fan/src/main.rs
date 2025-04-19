<<<<<<< Updated upstream
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::process::exit;
use std::thread::sleep;
use std::time::Duration;
use std::process::Command;
use std::fs;
use std::path::Path;
=======
use axum::{
    extract::{Json, State},
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    net::SocketAddr,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};
>>>>>>> Stashed changes

const EC_IO_FILE: &str = "/sys/kernel/debug/ec/ec0/io";
const FAN1_OFFSET: u64 = 0x34; // Fan 1 Speed Set (units of 100RPM)
const FAN2_OFFSET: u64 = 0x35; // Fan 2 Speed Set (units of 100RPM)
const CPU_TEMP_OFFSET: u64 = 0x57; // CPU Temp (°C)
const GPU_TEMP_OFFSET: u64 = 0xB7; // GPU Temp (°C)
const BIOS_CONTROL_OFFSET: u64 = 0x62; // BIOS Control
const FAN1_MAX: u8 = 55; // Max speed for Fan 1
const FAN2_MAX: u8 = 57; // Max speed for Fan 2
const CONFIG_FILE: &str = "/etc/omen-fan/config.toml";

<<<<<<< Updated upstream
fn generate_config_file() {
    if !Path::new(CONFIG_FILE).exists() {
        println!("Configuration file not found. Generating default config...");
        let default_config = r#"
[service]
TEMP_CURVE =  [45, 55, 60, 70, 75, 80, 85, 93]
SPEED_CURVE = [37, 45, 50, 60, 70, 80, 90, 100]
IDLE_SPEED = 0
POLL_INTERVAL = 1
"#;
        fs::create_dir_all("/etc/omen-fan").expect("Failed to create config directory.");
        fs::write(CONFIG_FILE, default_config).expect("Failed to write default config.");
        println!("Default configuration file created at {}", CONFIG_FILE);
    }
}

fn load_ec_sys_module() {
    // Check if the `ec_sys` module is loaded
    let output = Command::new("lsmod")
        .output()
        .expect("Failed to execute `lsmod` command.");
    if !String::from_utf8_lossy(&output.stdout).contains("ec_sys") {
        // Load the `ec_sys` module with write support
        Command::new("modprobe")
            .args(&["ec_sys", "write_support=1"])
            .status()
            .expect("Failed to load `ec_sys` module.");
    }
=======
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

struct AppState {
    fan_config: FanConfig,
    power_mode: PowerMode,
    curve_config: CurveConfig,
}

#[tokio::main]
async fn main() {
    let state = Arc::new(Mutex::new(AppState {
        fan_config: FanConfig {
            manual: false,
            speed: 0,
        },
        power_mode: PowerMode::Balanced,
        curve_config: CurveConfig {
            temp_curve: vec![45, 55, 60, 70, 75, 80, 85, 93],
            speed_curve: vec![37, 45, 50, 60, 70, 80, 90, 100],
        },
    }));

    write_ec(BIOS_CONTROL_OFFSET, 0x06); // Disable BIOS fan control

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
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn get_status(State(state): State<Arc<Mutex<AppState>>>) -> impl IntoResponse {
    let state = state.lock().unwrap();
    let cpu_temp = read_ec(CPU_TEMP_OFFSET);
    let gpu_temp = read_ec(GPU_TEMP_OFFSET);
    let fan1 = read_ec(FAN1_OFFSET);
    let fan2 = read_ec(FAN2_OFFSET);

    Json(SystemStatus {
        cpu_temp,
        gpu_temp,
        fan1,
        fan2,
        power_mode: state.power_mode.clone(),
    })
}

async fn set_fan_config(
    State(state): State<Arc<Mutex<AppState>>>,
    Json(config): Json<FanConfig>,
) -> impl IntoResponse {
    state.lock().unwrap().fan_config = config;
    "Fan config updated"
}

async fn set_power_mode(
    State(state): State<Arc<Mutex<AppState>>>,
    Json(mode): Json<PowerMode>,
) -> impl IntoResponse {
    let mut s = state.lock().unwrap();
    s.power_mode = mode.clone();
    match mode {
        PowerMode::Balanced => write_ec(PERF_MODE_OFFSET, 0x30),
        PowerMode::Performance => write_ec(PERF_MODE_OFFSET, 0x50),
        PowerMode::PowerSaving => {
            write_ec(PERF_MODE_OFFSET, 0x10);
            write_ec(TDP_OFFSET, 0x02); // Will reset fast, best effort
        }
    };
    "Power mode updated"
}

async fn set_curve_config(
    State(state): State<Arc<Mutex<AppState>>>,
    Json(curve): Json<CurveConfig>,
) -> impl IntoResponse {
    state.lock().unwrap().curve_config = curve;
    "Curve config updated"
>>>>>>> Stashed changes
}

fn read_ec_register(offset: u64) -> u8 {
    let mut file = File::open(EC_IO_FILE).expect("Failed to open EC IO file. Ensure you have the necessary permissions.");
    file.seek(SeekFrom::Start(offset))
        .expect("Failed to seek to EC register.");
    let mut buffer = [0u8; 1];
    file.read_exact(&mut buffer)
        .expect("Failed to read EC register.");
    buffer[0]
}

fn write_ec_register(offset: u64, value: u8) {
    let mut file = OpenOptions::new()
        .write(true)
        .open(EC_IO_FILE)
        .expect("Failed to open EC IO file. Ensure you have the necessary permissions.");
    file.seek(SeekFrom::Start(offset))
        .expect("Failed to seek to EC register.");
    file.write_all(&[value])
        .expect("Failed to write to EC register.");
}

fn get_max_temp() -> u8 {
    let cpu_temp = read_ec_register(CPU_TEMP_OFFSET);
    let gpu_temp = read_ec_register(GPU_TEMP_OFFSET);
    cpu_temp.max(gpu_temp)
}

fn set_fan_speed(fan1_speed: u8, fan2_speed: u8) {
    write_ec_register(FAN1_OFFSET, fan1_speed);
    write_ec_register(FAN2_OFFSET, fan2_speed);
}

fn disable_bios_control() {
    write_ec_register(BIOS_CONTROL_OFFSET, 0x06); // Disable BIOS control
}

// fn enable_bios_control() {
//    write_ec_register(BIOS_CONTROL_OFFSET, 0x00); // Enable BIOS control
// }

fn main() {
    if !nix::unistd::Uid::effective().is_root() {
        eprintln!("Root access is required to run this program.");
        exit(1);
    }

     // Perform setup tasks
     load_ec_sys_module();
     generate_config_file();
     disable_bios_control();

    let temp_curve = [45, 55, 60, 70, 75, 80, 85, 93];
    let speed_curve = [37, 45, 50, 60, 70, 80, 90, 100];
    let idle_speed = 0;
    let poll_interval = Duration::from_secs(1);

    let mut previous_speed = (0, 0);

    loop {
<<<<<<< Updated upstream
        let temp = get_max_temp();

        let speed = match temp {
            t if t <= temp_curve[0] => idle_speed,
            t if t >= temp_curve[temp_curve.len() - 1] => speed_curve[speed_curve.len() - 1],
            _ => {
                let index = temp_curve.iter().position(|&t| t > temp).unwrap();
                let t0 = temp_curve[index - 1];
                let t1 = temp_curve[index];
                let s0 = speed_curve[index - 1];
                let s1 = speed_curve[index];
                (s0 as usize + ((s1 - s0) as usize * (temp - t0) as usize / (t1 - t0) as usize)) as u8
            }
=======
        let lock = state.lock().unwrap();
        let temp = read_ec(CPU_TEMP_OFFSET).max(read_ec(GPU_TEMP_OFFSET));
        let fan_cfg = &lock.fan_config;
        let speed = if fan_cfg.manual {
            fan_cfg.speed
        } else {
            interpolate(temp, &lock.curve_config)
>>>>>>> Stashed changes
        };

        let fan1_speed = ((FAN1_MAX as u16 * speed as u16) / 100) as u8;
        let fan2_speed = ((FAN2_MAX as u16 * speed as u16) / 100) as u8;

<<<<<<< Updated upstream
        if previous_speed != (fan1_speed, fan2_speed) {
            set_fan_speed(fan1_speed, fan2_speed);
            previous_speed = (fan1_speed, fan2_speed);
=======
        drop(lock);
        thread::sleep(Duration::from_secs(1));
    }
}

fn interpolate(temp: u8, curve: &CurveConfig) -> u8 {
    let (t, s) = (&curve.temp_curve, &curve.speed_curve);
    match temp {
        t0 if t0 <= t[0] => 0,
        t0 if t0 >= *t.last().unwrap() => *s.last().unwrap(),
        _ => {
            let i = t.iter().position(|&x| x > temp).unwrap();
            let (t0, t1, s0, s1) = (t[i - 1], t[i], s[i - 1], s[i]);
            s0 + ((s1 - s0) * (temp - t0) / (t1 - t0))
>>>>>>> Stashed changes
        }

        sleep(poll_interval);
    }
<<<<<<< Updated upstream
=======
}

fn read_ec(offset: u64) -> u8 {
    let mut file = File::open(EC_IO_FILE).expect("Failed to open EC IO file");
    file.seek(SeekFrom::Start(offset)).unwrap();
    let mut buf = [0u8; 1];
    file.read_exact(&mut buf).unwrap();
    buf[0]
}

fn write_ec(offset: u64, value: u8) {
    let mut file = OpenOptions::new().write(true).open(EC_IO_FILE).unwrap();
    file.seek(SeekFrom::Start(offset)).unwrap();
    file.write_all(&[value]).unwrap();
>>>>>>> Stashed changes
}