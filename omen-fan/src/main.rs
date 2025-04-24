mod constants;
mod ec_interface;
mod fan_control;
mod config;
mod api;

use clap::{Parser, Subcommand};
use std::process::exit;
use std::thread::sleep;
use std::time::Duration;
// Fix for the Uid import error
use nix::unistd; // Import the unistd module
use nix::unistd::Uid;
use tokio::runtime::Runtime;

use constants::*;
use ec_interface::EcInterface;
use fan_control::FanControl;
use config::Config;

#[derive(Parser)]
#[command(name = "omen-fan-control")]
#[command(author = "Omen Fan Control Team")]
#[command(version = "1.0")]
#[command(about = "Control fan speeds and performance modes on HP Omen laptops", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Run in daemon mode with automatic fan control
    #[arg(short, long)]
    daemon: bool,

    /// Enable API server
    #[arg(long)]
    api: bool,

    /// API server port
    #[arg(long, default_value_t = DEFAULT_API_PORT)]
    port: u16,
}

#[derive(Subcommand)]
enum Commands {
    /// Get current temperatures and fan status
    Status,
    /// Set fan speed (0-100%)
    Fan {
        /// Speed percentage (0-100)
        #[arg(value_parser = clap::value_parser!(u8).range(0..=100))]
        speed: u8,
    },
    /// Set performance mode
    Mode {
        /// Mode (normal/performance)
        #[arg(value_parser = ["normal", "performance"])]
        mode: String,
    },
}

fn check_root() {
    // Fix for the root check using the correct nix API
    if !Uid::effective().is_root() {
        eprintln!("Root access is required to run this program.");
        exit(1);
    }
}

fn main() {
    check_root();
    EcInterface::load_ec_sys_module();
    
    let cli = Cli::parse();
    let mut fan_control = FanControl::new();
    
    // Always disable BIOS control
    EcInterface::disable_bios_control();

    match &cli.command {
        Some(Commands::Status) => {
            let status = FanControl::get_status();
            println!("{}", serde_json::to_string_pretty(&status).unwrap());
            return;
        },
        Some(Commands::Fan { speed }) => {
            fan_control.set_fan_speed_percentage(*speed);
            println!("Fan speed set to {}%", speed);
            return;
        },
        Some(Commands::Mode { mode }) => {
            match mode.to_lowercase().as_str() {
                "normal" => {
                    FanControl::set_normal_mode();
                    println!("Mode set to Normal");
                },
                "performance" => {
                    FanControl::set_performance_mode();
                    println!("Mode set to Performance");
                },
                _ => {
                    println!("Invalid mode. Use 'normal' or 'performance'");
                    exit(1);
                }
            }
            return;
        },
        None => {}
    }

    // Run API server if requested
    if cli.api {
        let runtime = Runtime::new().expect("Failed to create Tokio runtime");
        let port = cli.port;
        
        if cli.daemon {
            // Run both API server and daemon mode
            let config = Config::load();
            let poll_interval = Duration::from_secs(config.poll_interval);
            let api_fan_control = FanControl::new();
            
            runtime.block_on(async {
                // Start API server in a separate task
                let api_handle = tokio::spawn(api::run_api_server(api_fan_control, port));
                
                // Run daemon loop in main task
                tokio::spawn(async move {
                    let mut fan_control = FanControl::new();
                    loop {
                        EcInterface::disable_bios_control();
                        fan_control.adjust_fans_by_temp(&config);
                        tokio::time::sleep(poll_interval).await;
                    }
                });
                
                // Wait for API server
                api_handle.await.expect("API server failed").expect("API server error");
            });
        } else {
            // Run only API server
            runtime.block_on(async {
                api::run_api_server(fan_control, port).await.expect("API server failed");
            });
        }
    } else if cli.daemon {
        // Run only in daemon mode
        let config = Config::load();
        let poll_interval = Duration::from_secs(config.poll_interval);
        
        println!("Running in daemon mode. Press Ctrl+C to exit.");
        loop {
            EcInterface::disable_bios_control();
            fan_control.adjust_fans_by_temp(&config);
            sleep(poll_interval);
        }
    } else {
        // No command and no daemon/API mode - show usage
        println!("No command specified. Run with --help for usage information.");
    }
}