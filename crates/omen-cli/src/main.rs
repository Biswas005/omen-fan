use anyhow::{bail, Context, Result};
use omen_core::{FanCurve, FanMode, PowerSource, Request, Response, Snapshot};
use std::env;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const ORANGE: &str = "\x1b[38;5;209m";
const RED: &str = "\x1b[38;5;203m";
const GREEN: &str = "\x1b[38;5;114m";
const GRAY: &str = "\x1b[38;5;245m";

fn banner() {
    println!("{ORANGE}      ◆{RESET}");
    println!("{ORANGE}    ◆◆◆◆◆{RESET}   {BOLD}OMEN{RESET}{DIM} control{RESET}");
    println!("{ORANGE}  ◆◆◆   ◆◆◆{RESET}");
    println!("{ORANGE}    ◆◆◆◆◆{RESET}");
    println!("{ORANGE}      ◆{RESET}");
    println!();
}

fn send(req: Request) -> Result<Response> {
    let mut stream = UnixStream::connect(omen_core::SOCKET_PATH).context("connect to omen-daemon")?;
    let payload = serde_json::to_string(&req)?;
    stream.write_all(payload.as_bytes())?;
    stream.write_all(b"\n")?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    Ok(serde_json::from_str(&line)?)
}

fn get_snapshot() -> Result<Snapshot> {
    match send(Request::GetSnapshot)? {
        Response::Snapshot(s) => Ok(s),
        Response::Error { message } => bail!("daemon error: {message}"),
    }
}

fn print_status(snapshot: &Snapshot) {
    let live = &snapshot.live;
    let power = match live.power_source {
        PowerSource::AC => format!("{GREEN}AC{RESET}"),
        PowerSource::Battery => format!("{ORANGE}Battery{RESET}"),
        PowerSource::Unknown => format!("{GRAY}Unknown{RESET}"),
    };
    println!("{BOLD}Active profile{RESET}   {ORANGE}{}{RESET}", snapshot.state.active_profile);
    println!("{BOLD}CPU temp{RESET}         {:.1} °C", live.cpu_temp_c);
    println!("{BOLD}Fan RPM{RESET}          {}", live.rpm);
    println!("{BOLD}Duty{RESET}             {:.0}%", live.duty_pct);
    println!("{BOLD}Power source{RESET}     {power}");
    if let Some(mode) = &live.current_platform_mode {
        println!("{BOLD}Platform mode{RESET}    {mode}");
    }
    if let Some(mode) = &live.graphics_mode {
        println!("{BOLD}Graphics mode{RESET}    {}", mode.label());
    }
    if live.max_fan_active {
        println!("{RED}{BOLD}MAX FAN ACTIVE{RESET}");
    }
}

fn print_curve(curve: &FanCurve) {
    println!("{BOLD}{:>8}   {:>6}{RESET}", "temp °C", "duty %");
    for p in &curve.points {
        println!("{:>8.1}   {:>6.0}", p.temp_c, p.duty_pct);
    }
}

fn parse_fan_mode(s: &str) -> Result<FanMode> {
    Ok(match s.to_lowercase().as_str() {
        "auto" => FanMode::Auto,
        "curve" => FanMode::Curve,
        "manual" => FanMode::Manual,
        "max" => FanMode::Max,
        other => bail!("unknown fan mode '{other}' (expected auto|curve|manual|max)"),
    })
}

fn usage() {
    banner();
    println!("{BOLD}Usage:{RESET} omen-cli <command> [args]");
    println!();
    println!("{BOLD}Commands:{RESET}");
    println!("  status                          Show live telemetry and active profile");
    println!("  profile list                    List configured profiles");
    println!("  profile set <name>              Make <name> the active profile");
    println!("  fan mode <profile> <mode>       Set fan mode (auto|curve|manual|max)");
    println!("  fan manual <profile> <pct>      Set manual duty percent (25-100)");
    println!("  fan max <profile> <on|off>      Toggle the max-fan override");
    println!("  curve show <profile>            Print a profile's fan curve");
    println!("  platform <profile> <mode>       Set a profile's platform mode");
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    let cmd = args.first().map(|s| s.as_str());

    match cmd {
        None | Some("status") => {
            let snapshot = get_snapshot()?;
            print_status(&snapshot);
        }
        Some("profile") => match args.get(1).map(|s| s.as_str()) {
            Some("list") => {
                let snapshot = get_snapshot()?;
                for p in &snapshot.state.profiles {
                    let marker = if p.name == snapshot.state.active_profile { format!("{ORANGE}●{RESET}") } else { format!("{DIM}○{RESET}") };
                    println!("{marker} {} {DIM}({:?}){RESET}", p.name, p.fan_mode);
                }
            }
            Some("set") => {
                let name = args.get(2).context("usage: omen-cli profile set <name>")?;
                match send(Request::SetActiveProfile { name: name.clone() })? {
                    Response::Snapshot(s) => println!("{GREEN}Active profile set to {}{RESET}", s.state.active_profile),
                    Response::Error { message } => bail!("daemon error: {message}"),
                }
            }
            _ => usage(),
        },
        Some("fan") => match args.get(1).map(|s| s.as_str()) {
            Some("mode") => {
                let profile = args.get(2).context("usage: omen-cli fan mode <profile> <mode>")?.clone();
                let mode = parse_fan_mode(args.get(3).context("usage: omen-cli fan mode <profile> <mode>")?)?;
                send(Request::SetFanMode { profile: profile.clone(), mode })?;
                println!("{GREEN}{profile}: fan mode set to {mode:?}{RESET}");
            }
            Some("manual") => {
                let profile = args.get(2).context("usage: omen-cli fan manual <profile> <pct>")?.clone();
                let pct: u8 = args.get(3).context("usage: omen-cli fan manual <profile> <pct>")?.parse().context("pct must be a number 25-100")?;
                send(Request::SetManualDuty { profile: profile.clone(), duty_pct: pct })?;
                println!("{GREEN}{profile}: manual duty set to {pct}%{RESET}");
            }
            Some("max") => {
                let profile = args.get(2).context("usage: omen-cli fan max <profile> <on|off>")?.clone();
                let enabled = match args.get(3).map(|s| s.as_str()) {
                    Some("on") => true,
                    Some("off") => false,
                    _ => bail!("usage: omen-cli fan max <profile> <on|off>"),
                };
                send(Request::SetMaxFan { profile: profile.clone(), enabled })?;
                println!("{GREEN}{profile}: max fan override {}{RESET}", if enabled { "enabled" } else { "disabled" });
            }
            _ => usage(),
        },
        Some("curve") => match args.get(1).map(|s| s.as_str()) {
            Some("show") => {
                let profile_name = args.get(2).context("usage: omen-cli curve show <profile>")?;
                let snapshot = get_snapshot()?;
                let profile = snapshot.state.profile(profile_name).with_context(|| format!("no such profile '{profile_name}'"))?;
                print_curve(&profile.curve);
            }
            _ => usage(),
        },
        Some("platform") => {
            let profile = args.get(1).context("usage: omen-cli platform <profile> <mode>")?.clone();
            let mode = args.get(2).context("usage: omen-cli platform <profile> <mode>")?.clone();
            send(Request::SetPlatformMode { profile: profile.clone(), mode: Some(mode.clone()) })?;
            println!("{GREEN}{profile}: platform mode set to {mode}{RESET}");
        }
        _ => usage(),
    }

    Ok(())
}
