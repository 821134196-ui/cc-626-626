pub mod auth;
pub mod db;
pub mod error;
pub mod mocks;
pub mod models;
pub mod routes;
pub mod state;

use std::path::PathBuf;

/// 构建完整应用（建库、建表、种子用户），供 main 与集成测试共用。
pub async fn build_app(database_path: &str, upload_dir: PathBuf) -> anyhow::Result<axum::Router> {
    tokio::fs::create_dir_all(&upload_dir).await?;
    if let Some(parent) = std::path::Path::new(database_path).parent() {
        if !parent.as_os_str().is_empty() {
            tokio::fs::create_dir_all(parent).await?;
        }
    }
    let pool = db::connect(database_path).await?;
    db::init(&pool).await?;
    db::seed_users(&pool).await?;
    Ok(routes::build(state::AppState {
        db: pool,
        upload_dir,
    }))
}
