use std::path::PathBuf;

mod auth;
mod error;
mod handlers;
mod sim;
mod state;

pub use state::AppState;

use axum::{
    routing::{get, post},
    Router,
};

/// 构建应用路由。未匹配路径由 fallback 处理:静态资源、SPA 回退、/api 404。
pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/login", post(handlers::login))
        .route("/me", get(handlers::me))
        .route("/events", get(handlers::list_events))
        .route("/reports", post(handlers::submit_report))
        .route("/events/{id}", get(handlers::event_detail))
        // 注意:不存在通用的 PATCH /events/:id —— 状态只能通过职责端点流转,
        // 无法靠修改请求直接越权结案。
        .route("/events/{id}/verify", post(handlers::verify_event))
        .route("/events/{id}/notify", post(handlers::notify_owner))
        .route("/events/{id}/rechecks", post(handlers::submit_recheck))
        .route("/photos/{filename}", get(handlers::photo));

    Router::new()
        .nest("/api", api)
        .fallback(handlers::spa_fallback)
        .with_state(state)
}

/// 测试辅助:在临时目录上构建 AppState。
pub async fn test_state(dir: &std::path::Path) -> AppState {
    let db_path: PathBuf = dir.join("test.db");
    let uploads = dir.join("uploads");
    tokio::fs::create_dir_all(&uploads).await.unwrap();
    AppState::connect(db_path, uploads, None).await.unwrap()
}
