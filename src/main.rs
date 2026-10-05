#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let db_path = std::env::var("DATABASE_PATH").unwrap_or_else(|_| "data/parking.db".into());
    let upload_dir = std::env::var("UPLOAD_DIR").unwrap_or_else(|_| "uploads".into());
    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".into());

    let app = parking_evidence::build_app(&db_path, upload_dir.into()).await?;
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    tracing::info!("新能源车位占用举证系统已启动: http://localhost:{port}");
    axum::serve(listener, app).await?;
    Ok(())
}
