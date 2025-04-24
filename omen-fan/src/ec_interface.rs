use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::process::Command;
use crate::constants::*;

pub struct EcInterface;

impl EcInterface {
    pub fn load_ec_sys_module() {
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
    }

    pub fn read_ec_register(offset: u64) -> u8 {
        let mut file = File::open(EC_IO_FILE)
            .expect("Failed to open EC IO file. Ensure you have the necessary permissions.");
        file.seek(SeekFrom::Start(offset))
            .expect("Failed to seek to EC register.");
        
        let mut buffer = [0u8; 1];
        file.read_exact(&mut buffer)
            .expect("Failed to read EC register.");
        
        buffer[0]
    }

    pub fn write_ec_register(offset: u64, value: u8) {
        let mut file = OpenOptions::new()
            .write(true)
            .open(EC_IO_FILE)
            .expect("Failed to open EC IO file. Ensure you have the necessary permissions.");
        
        file.seek(SeekFrom::Start(offset))
            .expect("Failed to seek to EC register.");
        
        file.write_all(&[value])
            .expect("Failed to write to EC register.");
    }

    pub fn get_cpu_temp() -> u8 {
        Self::read_ec_register(CPU_TEMP_OFFSET)
    }

    pub fn get_gpu_temp() -> u8 {
        Self::read_ec_register(GPU_TEMP_OFFSET)
    }

    pub fn get_max_temp() -> u8 {
        let cpu_temp = Self::get_cpu_temp();
        let gpu_temp = Self::get_gpu_temp();
        cpu_temp.max(gpu_temp)
    }

    pub fn disable_bios_control() {
        Self::write_ec_register(BIOS_CONTROL_OFFSET, 0x06); // Disable BIOS control
    }

    pub fn enable_bios_control() {
        Self::write_ec_register(BIOS_CONTROL_OFFSET, 0x00); // Enable BIOS control
    }

    pub fn get_performance_mode() -> u8 {
        Self::read_ec_register(PERFORMANCE_OFFSET)
    }

    pub fn set_performance_mode(mode: u8) {
        Self::write_ec_register(PERFORMANCE_OFFSET, mode);
    }
}