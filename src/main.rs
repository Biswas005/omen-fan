use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::process::{exit, Command};
use std::thread::sleep;
use std::time::{Duration, Instant};
use std::collections::VecDeque;
use serde::{Deserialize, Serialize};
use std::fs;
use clap::{Arg, Command as ClapCommand};

#[cfg(feature = "acpi_ec")]
const EC_IO_FILE: &str = "/dev/ec";

#[cfg(not(feature = "acpi_ec"))]
const EC_IO_FILE: &str = "/sys/kernel/debug/ec/ec0/io";

const PERFORMANCE_OFFSET: u64 = 0x95;
const FAN1_OFFSET: u64 = 0x34;
const FAN2_OFFSET: u64 = 0x35;
const CPU_TEMP_OFFSET: u64 = 0x57;
const GPU_TEMP_OFFSET: u64 = 0xB7;
const BIOS_CONTROL_OFFSET: u64 = 0x62;
const FAN1_MAX: u8 = 55;
const FAN2_MAX: u8 = 57;
const BIOS_LEGACY_DEFAULT_MODE: u8 = 0;
const BIOS_DEFAULT_MODE: u8 = 48; // 0x30
const BIOS_PERFORMANCE_MODE: u8 = 49; // 0x31
const BIOS_COOL_MODE: u8 = 80; // 0x50

#[derive(Debug, Deserialize, Serialize, Clone)]
struct FanPoint {
    temp: u64,
    speed: u8,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct ModeCurve {
    curve: Vec<FanPoint>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct FanConfig {
    mode: String,
    default: ModeCurve,
        performance: ModeCurve,
        cool: ModeCurve,
}

fn load_ec_sys_module() {
    if EC_IO_FILE == "/dev/ec" {
        return;
    } else {
        let output = Command::new("lsmod")
        .output()
        .expect("Failed to execute `lsmod` command.");
        if !String::from_utf8_lossy(&output.stdout).contains("ec_sys") {
            Command::new("modprobe")
            .args(&["ec_sys", "write_support=1"])
            .status()
            .expect("Failed to load `ec_sys` module.");
        }
    }
}

fn read_ec_register(offset: u64) -> u8 {
    let mut file = File::open(EC_IO_FILE).expect("Failed to open EC IO file. Ensure you have the necessary permissions.");
    file.seek(SeekFrom::Start(offset)).expect("Failed to seek to EC register.");
    let mut buffer = [0u8; 1];
    file.read_exact(&mut buffer).expect("Failed to read EC register.");
    buffer[0]
}

fn write_ec_register(offset: u64, value: u8) {
    let mut file = OpenOptions::new()
    .write(true)
    .open(EC_IO_FILE)
    .expect("Failed to open EC IO file for writing. Ensure you have the necessary permissions.");
    file.seek(SeekFrom::Start(offset)).expect("Failed to seek to EC register.");
    file.write_all(&[value]).expect("Failed to write to EC register.");
    println!("Wrote 0x{:02X} to EC register 0x{:02X}", value, offset); // Debug output
}

fn get_max_temp() -> u8 {
    let cpu_temp = read_ec_register(CPU_TEMP_OFFSET);
    let gpu_temp = read_ec_register(GPU_TEMP_OFFSET);
    cpu_temp.max(gpu_temp)
}

fn set_fan_speed(fan1_speed: u8, fan2_speed: u8) {
    println!("Writing Fan1 speed: {}, Fan2 speed: {}", fan1_speed, fan2_speed);
    write_ec_register(FAN1_OFFSET, fan1_speed);
    write_ec_register(FAN2_OFFSET, fan2_speed);
}

fn disable_bios_control() {
    println!("Disabling BIOS control (writing 0x06 to 0x{:02X})", BIOS_CONTROL_OFFSET);
    write_ec_register(BIOS_CONTROL_OFFSET, 0x06);
    let control_value = read_ec_register(BIOS_CONTROL_OFFSET);
    println!("BIOS control register (0x{:02X}): 0x{:02X}", BIOS_CONTROL_OFFSET, control_value);
}

fn enable_bios_control() {
    write_ec_register(BIOS_CONTROL_OFFSET, 0x00);
}

fn apply_bios_mode(mode: u8) {
    write_ec_register(PERFORMANCE_OFFSET, mode);
}

fn mode() -> String {
    let register_value = read_ec_register(PERFORMANCE_OFFSET);
    println!("Performance register (0x95): 0x{:02X}", register_value); // Debug output
    match register_value {
        0x30 => "Default Mode".to_string(),
        0x31 => "Performance Mode".to_string(),
        0x50 => "Cool Mode".to_string(),
        0x00 => "Legacy Default Mode".to_string(),
        _ => format!("Undefined Mode (0x{:02X})", register_value),
    }
}

fn read_fan_config(path: &str) -> Result<FanConfig, String> {
    match fs::read_to_string(path) {
        Ok(contents) => match toml::from_str(&contents) {
            Ok(config) => Ok(config),
            Err(e) => Err(format!("Invalid TOML format in {}: {}", path, e)),
        },
        Err(e) => Err(format!("Failed to read {}: {}", path, e)),
    }
}

fn read_runtime_mode(config: &FanConfig) -> String {
    println!("Read mode from config: {}", config.mode);
    config.mode.clone()
}

fn write_runtime_mode(path: &str, mode: &str, config: &FanConfig) {
    let new_config = FanConfig {
        mode: mode.to_string(),
        default: config.default.clone(),
            performance: config.performance.clone(),
            cool: config.cool.clone(),
    };
    let toml_string = toml::to_string(&new_config).expect("Failed to serialize config to TOML");
    fs::write(path, toml_string).expect("Failed to write to config file");
    println!("Wrote mode '{}' to {}", mode, path);
}

fn lookup_speed(curve: &[FanPoint], temp: u64) -> u8 {
    for point in curve.iter().rev() {
        if temp >= point.temp {
            return point.speed;
        }
    }
    0
}

fn main() {
    if !nix::unistd::Uid::effective().is_root() {
        eprintln!("Root access is required to run this program.");
        exit(1);
    }

    // Parse command-line arguments
    let matches = ClapCommand::new("omen-fan")
    .version("0.1.0")
    .arg(
        Arg::new("config")
        .long("config")
        .value_name("FILE")
        .help("Sets the path to the TOML config file")
        .default_value("fan_config.toml"),
    )
    .get_matches();

    let config_path = matches.get_one::<String>("config").expect("Config path missing");

    load_ec_sys_module();

    let mut config: FanConfig = read_fan_config(config_path).expect("Failed to load initial config");

    let idle_speed = 0;
    let poll_interval = Duration::from_secs(1);
    let bios_control_interval = Duration::from_secs(100); // 100 seconds
    let config_check_interval = Duration::from_secs(2); // Check TOML file every 5 seconds
    let mut last_bios_control = Instant::now();
    let mut last_config_check = Instant::now();

    let mut previous_speed = (0, 0);
    let mut previous_mode = "Legacy Default Mode".to_string();
    let mut previous_runtime_mode = String::new();
    let mut temp_history: VecDeque<u8> = VecDeque::with_capacity(5);

    loop {
        // Check if 100 seconds have elapsed to call disable_bios_control
        if last_bios_control.elapsed() >= bios_control_interval {
            disable_bios_control();
            last_bios_control = Instant::now(); // Reset timer
        }

        // Check if 5 seconds have elapsed to re-read fan_config.toml
        if last_config_check.elapsed() >= config_check_interval {
            match read_fan_config(config_path) {
                Ok(new_config) => {
                    if new_config.mode != config.mode {
                        println!("Detected mode change in TOML: {} -> {}", config.mode, new_config.mode);
                        config = new_config;
                    }
                }
                Err(e) => eprintln!("Failed to re-read config: {}", e),
            }
            last_config_check = Instant::now(); // Reset timer
        }

        let current_mode = mode();
        println!("Current mode: {current_mode}"); // Debug output

        let runtime_mode = read_runtime_mode(&config);
        // Apply BIOS mode based on config.mode
        if runtime_mode != previous_runtime_mode {
            let bios_mode = match runtime_mode.as_str() {
                "performance" => BIOS_PERFORMANCE_MODE, // 0x31
                "cool" => BIOS_COOL_MODE, // 0x50
                _ => BIOS_DEFAULT_MODE, // 0x30
            };
            println!("Applying BIOS mode: {} (0x{:02X})", runtime_mode, bios_mode);
            apply_bios_mode(bios_mode);
            write_runtime_mode(config_path, &runtime_mode, &config);
            previous_runtime_mode = runtime_mode.clone();
        }

        let current_temp = get_max_temp();
        if temp_history.len() == 5 {
            temp_history.pop_front();
        }
        temp_history.push_back(current_temp);

        let avg_temp = (temp_history.iter().map(|&x| x as u16).sum::<u16>() / temp_history.len() as u16) as u8;
        println!("Avg temperature: {}\u{00B0}C", avg_temp);

        let curve = match runtime_mode.as_str() {
            "performance" => &config.performance.curve,
            "cool" => &config.cool.curve,
            _ => &config.default.curve,
        };

        let speed = lookup_speed(curve, avg_temp as u64);
        let fan1_speed = ((FAN1_MAX as u16 * speed as u16) / 100) as u8;
        let fan2_speed = ((FAN2_MAX as u16 * speed as u16) / 100) as u8;
        println!("Selected curve: {}, Temp: {}°C, Speed: {}, Fan1: {}, Fan2: {}", runtime_mode, avg_temp, speed, fan1_speed, fan2_speed);

        if previous_speed != (fan1_speed, fan2_speed) {
            set_fan_speed(fan1_speed, fan2_speed);
            previous_speed = (fan1_speed, fan2_speed);
        }

        previous_mode = current_mode;
        sleep(poll_interval);
    }
}
