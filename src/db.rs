use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;

use crate::auth::hash_password;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS users (
    id            INTEGER PRIMARY KEY,
    username      TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    name          TEXT NOT NULL,
    role          TEXT NOT NULL CHECK (role IN ('inspector', 'officer'))
);

CREATE TABLE IF NOT EXISTS sessions (
    token      TEXT PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users(id),
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS events (
    id               INTEGER PRIMARY KEY,
    spot_number      TEXT NOT NULL,
    plate_reported   TEXT NOT NULL,
    plate_confirmed  TEXT,
    status           TEXT NOT NULL DEFAULT 'pending_verify'
                     CHECK (status IN ('pending_verify','verified','notified','closed','rejected')),
    created_by       INTEGER NOT NULL REFERENCES users(id),
    created_at       TEXT NOT NULL,
    verified_by      INTEGER REFERENCES users(id),
    verified_at      TEXT,
    notified_count   INTEGER NOT NULL DEFAULT 0,
    last_notified_at TEXT,
    closed_by        INTEGER REFERENCES users(id),
    closed_at        TEXT,
    close_note       TEXT
);

CREATE INDEX IF NOT EXISTS idx_events_open_spot
    ON events(spot_number) WHERE status IN ('pending_verify','verified','notified');

CREATE TABLE IF NOT EXISTS reports (
    id                     INTEGER PRIMARY KEY,
    event_id               INTEGER NOT NULL REFERENCES events(id),
    reporter_id            INTEGER NOT NULL REFERENCES users(id),
    spot_number            TEXT NOT NULL,
    plate                  TEXT NOT NULL,
    occurred_at            TEXT NOT NULL,
    photo_path             TEXT NOT NULL,
    recognized_plate       TEXT,
    recognition_confidence REAL,
    needs_manual_review    INTEGER NOT NULL DEFAULT 0,
    created_at             TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_reports_event ON reports(event_id);

CREATE TABLE IF NOT EXISTS notifications (
    id         INTEGER PRIMARY KEY,
    event_id   INTEGER NOT NULL REFERENCES events(id),
    sent_by    INTEGER NOT NULL REFERENCES users(id),
    plate      TEXT NOT NULL,
    phone      TEXT NOT NULL,
    content    TEXT NOT NULL,
    status     TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_notifications_event ON notifications(event_id);

CREATE TABLE IF NOT EXISTS rechecks (
    id           INTEGER PRIMARY KEY,
    event_id     INTEGER NOT NULL REFERENCES events(id),
    inspector_id INTEGER NOT NULL REFERENCES users(id),
    result       TEXT NOT NULL CHECK (result IN ('left','still_occupied')),
    note         TEXT,
    created_at   TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_rechecks_event ON rechecks(event_id);
"#;

pub async fn connect(path: &str) -> anyhow::Result<SqlitePool> {
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(opts)
        .await?;
    Ok(pool)
}

pub async fn init(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::raw_sql(SCHEMA).execute(pool).await?;
    Ok(())
}

/// 演示账号（首次启动时写入）
pub async fn seed_users(pool: &SqlitePool) -> anyhow::Result<()> {
    const USERS: &[(&str, &str, &str, &str)] = &[
        ("xuncha01", "xuncha123", "巡查员·李", "inspector"),
        ("xuncha02", "xuncha123", "巡查员·王", "inspector"),
        ("zhiban01", "zhiban123", "值班员·赵", "officer"),
    ];
    for (username, password, name, role) in USERS {
        sqlx::query(
            "INSERT OR IGNORE INTO users (username, password_hash, name, role) VALUES (?, ?, ?, ?)",
        )
        .bind(username)
        .bind(hash_password(password))
        .bind(name)
        .bind(role)
        .execute(pool)
        .await?;
    }
    Ok(())
}
