use actix_web::{web, App, HttpServer, HttpResponse, Responder, get, post};
use actix_cors::Cors;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use crate::fan_control::FanControl;
use crate::constants::*;
use crate::ec_interface::EcInterface;
use serde_json::json;

#[derive(Debug, Serialize, Deserialize)]
struct SpeedRequest {
    speed: u8,
}

#[derive(Debug, Serialize, Deserialize)]
struct ModeRequest {
    mode: String,
}

struct AppState {
    fan_control: Mutex<FanControl>,
}

#[get("/status")]
async fn get_status(data: web::Data<Arc<AppState>>) -> impl Responder {
    let status = FanControl::get_status();
    HttpResponse::Ok().json(status)
}

#[post("/fan/speed")]
async fn set_fan_speed(data: web::Data<Arc<AppState>>, req: web::Json<SpeedRequest>) -> impl Responder {
    let mut fan_control = data.fan_control.lock().unwrap();
    fan_control.set_fan_speed_percentage(req.speed.min(100));
    HttpResponse::Ok().json(json!({
        "status": "ok", 
        "speed": req.speed
    }))
}

#[post("/mode")]
async fn set_mode(data: web::Data<Arc<AppState>>, req: web::Json<ModeRequest>) -> impl Responder {
    match req.mode.to_lowercase().as_str() {
        "normal" => {
            FanControl::set_normal_mode();
            HttpResponse::Ok().json(json!({
                "status": "ok", 
                "mode": "normal"
            }))
        },
        "performance" => {
            FanControl::set_performance_mode();
            HttpResponse::Ok().json(json!({
                "status": "ok", 
                "mode": "performance"
            }))
        },
        _ => HttpResponse::BadRequest().json(json!({
            "status": "error", 
            "message": "Invalid mode. Use 'normal' or 'performance'"
        }))
    }
}

pub fn run_api_server(fan_control: FanControl, port: u16) -> impl std::future::Future<Output = std::io::Result<()>> + Send + 'static {
    let app_state = Arc::new(AppState {
        fan_control: Mutex::new(fan_control),
    });

    println!("Starting API server on port {}", port);

    HttpServer::new(move || {
        let cors = Cors::default()
            .allow_any_origin()
            .allow_any_method()
            .allow_any_header();

        App::new()
            .wrap(cors)
            .app_data(web::Data::new(app_state.clone()))
            .service(get_status)
            .service(set_fan_speed)
            .service(set_mode)
    })
    .bind(("127.0.0.1", port))
    .expect("Failed to bind port")
    .run()
}