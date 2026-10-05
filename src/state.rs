#[derive(Clone)]
pub struct AppState {
    pub db: sqlx::SqlitePool,
    pub upload_dir: std::path::PathBuf,
}
