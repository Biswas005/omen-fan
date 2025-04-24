use crate::constants::*;
use crate::ec_interface::EcInterface;
use crate::config::Config;
use serde_json::json;

pub struct FanControl {
    previous_speeds: (u8, u8),
}

impl FanControl {
    pub fn new() -> Self {
        Self {
            previous_speeds: (0, 0),
        }
    }

    pub fn set_fan_speed(&mut self, fan1_speed: u8, fan2_speed: u8) {
        if self.previous_speeds != (fan1_speed, fan2_speed) {
            EcInterface::write_ec_register(FAN1_OFFSET, fan1_speed);
            EcInterface::write_ec_register(FAN2_OFFSET, fan2_speed);
            self.previous_speeds = (fan1_speed, fan2_speed);
        }
    }

    pub fn set_fan_speed_percentage(&mut self, speed_percentage: u8) {
        let clamped_speed = speed_percentage.min(100);
        let fan1_speed = ((FAN1_MAX as u16 * clamped_speed as u16) / 100) as u8;
        let fan2_speed = ((FAN2_MAX as u16 * clamped_speed as u16) / 100) as u8;
        self.set_fan_speed(fan1_speed, fan2_speed)
    }

    pub fn adjust_fans_by_temp(&mut self, config: &Config) {
        let temp = EcInterface::get_max_temp();
        
        // Find the appropriate speed based on the temperature and configured curves
        let speed = if temp <= config.temp_curve[0] {
            config.idle_speed
        } else {
            // Find where temperature falls in the curve
            let mut speed = config.speed_curve.last().copied().unwrap_or(100);
            
            for i in 0..config.temp_curve.len() {
                if temp <= config.temp_curve[i] {
                    speed = config.speed_curve[i];
                    break;
                }
            }
            speed
        };

        self.set_fan_speed_percentage(speed);
    }

    pub fn get_mode_name() -> String {
        match EcInterface::get_performance_mode() {
            MODE_NORMAL => "Normal Mode".to_string(),
            MODE_PERFORMANCE => "Performance Mode".to_string(),
            _ => "Undefined Mode".to_string(),
        }
    }

    pub fn set_normal_mode() {
        EcInterface::set_performance_mode(MODE_NORMAL);
    }

    pub fn set_performance_mode() {
        EcInterface::set_performance_mode(MODE_PERFORMANCE);
    }

    pub fn get_status() -> serde_json::Value {
        json!({
            "cpu_temp": EcInterface::get_cpu_temp(),
            "gpu_temp": EcInterface::get_gpu_temp(),
            "max_temp": EcInterface::get_max_temp(),
            "mode": Self::get_mode_name(),
            "fan_speeds": {
                "fan1": 0,
                "fan2": 0
            }
        })
    }
}