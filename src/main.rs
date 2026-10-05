use std::net::SocketAddr;
use std::path::PathBuf;

use ev_parking_evidence::{router, AppState};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string());
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);

    let base = PathBuf::from(&data_dir);
    let db_path = base.join("evidence.db");
    let uploads = base.join("uploads");
    let static_dir = PathBuf::from("./frontend/dist");
    let static_dir = if static_dir.exists() {
        Some(static_dir)
    } else {
        println!("[warn] 未找到 frontend/dist,仅提供 API(先运行 npm run build)");
        None
    };

    let state = AppState::connect(db_path, uploads, static_dir).await?;
    let app = router(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("新能源车位占用举证系统已启动: http://{addr}");
    println!("演示账号: inspector1 / inspector2(巡查员), duty1(值班员),登录无需密码");
    axum::serve(listener, app).await?;
    Ok(())
}
