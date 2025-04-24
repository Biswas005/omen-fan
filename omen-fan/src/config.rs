use std::fs;
use std::path::Path;
use serde::{Deserialize, Serialize};
use crate::constants::CONFIG_FILE;

#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    pub service: ServiceConfig,
    #[serde(skip)]
    pub temp_curve: Vec<u8>,
    #[serde(skip)]
    pub speed_curve: Vec<u8>,
    #[serde(skip)]
    pub idle_speed: u8,
    #[serde(skip)]
    pub poll_interval: u64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ServiceConfig {
    pub TEMP_CURVE: Vec<u8>,
    pub SPEED_CURVE: Vec<u8>,
    pub IDLE_SPEED: u8,
    pub POLL_INTERVAL: u64,
}

impl Config {
    pub fn load() -> Self {
        Self::ensure_config_exists();
        
        let config_content = fs::read_to_string(CONFIG_FILE)
            .expect("Failed to read config file");
        
        let mut config: Config = toml::from_str(&config_content)
            .expect("Failed to parse config file");
        
        // Copy values from the service struct to the main fields for easier access
        config.temp_curve = config.service.TEMP_CURVE.clone();
        config.speed_curve = config.service.SPEED_CURVE.clone();
        config.idle_speed = config.service.IDLE_SPEED;
        config.poll_interval = config.service.POLL_INTERVAL;
        
        config
    }
    
    pub fn ensure_config_exists() {
        if !Path::new(CONFIG_FILE).exists() {
            println!("Configuration file not found. Generating default config...");
            let default_config = r#"
[service]
TEMP_CURVE = [46, 49, 52, 55, 58, 61, 64, 67, 70, 73, 76, 79, 82, 85, 93]
SPEED_CURVE = [37, 40, 43, 46, 49, 52, 55, 58, 61, 64, 67, 70, 85, 90, 100]
IDLE_SPEED = 0
POLL_INTERVAL = 1
"#;
            fs::create_dir_all("/etc/omen-fan").expect("Failed to create config directory.");
            fs::write(CONFIG_FILE, default_config).expect("Failed to write default config.");
            println!("Default configuration file created at {}", CONFIG_FILE);
        }
    }
}