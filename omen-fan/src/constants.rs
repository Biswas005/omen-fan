// EC register addresses and constants
pub const EC_IO_FILE: &str = "/sys/kernel/debug/ec/ec0/io";
pub const PERFORMANCE_OFFSET: u64 = 0x95;
pub const FAN1_OFFSET: u64 = 0x34; // Fan 1 Speed Set (units of 100RPM)
pub const FAN2_OFFSET: u64 = 0x35; // Fan 2 Speed Set (units of 100RPM)
pub const CPU_TEMP_OFFSET: u64 = 0x57; // CPU Temp (°C)
pub const GPU_TEMP_OFFSET: u64 = 0xB7; // GPU Temp (°C)
pub const BIOS_CONTROL_OFFSET: u64 = 0x62; // BIOS Control
pub const FAN1_MAX: u8 = 55; // Max speed for Fan 1
pub const FAN2_MAX: u8 = 57; // Max speed for Fan 2
pub const CONFIG_FILE: &str = "/etc/omen-fan/config.toml";

// Performance modes
pub const MODE_NORMAL: u8 = 0x30;
pub const MODE_PERFORMANCE: u8 = 0x31;

// API server settings
pub const DEFAULT_API_PORT: u16 = 8080;