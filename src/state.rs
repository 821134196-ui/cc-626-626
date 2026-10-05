use std::path::PathBuf;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub uploads_dir: PathBuf,
    pub static_dir: Option<PathBuf>,
}

impl AppState {
    pub async fn connect(
        db_path: PathBuf,
        uploads_dir: PathBuf,
        static_dir: Option<PathBuf>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        tokio::fs::create_dir_all(&uploads_dir).await?;

        let options = SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true)
            // 每个连接建立后启用外键约束
            .foreign_keys(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;

        // 单写者:测试与小型部署足够,避免 SQLITE_BUSY
        sqlx::query("PRAGMA journal_mode=WAL").execute(&pool).await?;
        sqlx::query("PRAGMA busy_timeout=5000").execute(&pool).await?;

        sqlx::migrate!("./migrations").run(&pool).await?;

        Ok(Self { pool, uploads_dir, static_dir })
    }
}
