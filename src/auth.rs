use axum::{extract::FromRequestParts, http::request::Parts};
use sha2::{Digest, Sha256};

use crate::{
    error::{ApiError, ApiResult},
    models::role,
    state::AppState,
};

/// 演示用口令散列（生产环境应使用 argon2/bcrypt）
pub fn hash_password(password: &str) -> String {
    let digest = Sha256::digest(format!("parking-evidence-salt:{password}").as_bytes());
    hex::encode(digest)
}

/// 从请求中解析出的登录用户。角色权限只以服务端会话为准，
/// 请求体/参数中携带的任何"角色"字段一律忽略，防止篡改请求越权。
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub id: i64,
    pub username: String,
    pub name: String,
    pub role: String,
}

#[derive(sqlx::FromRow)]
struct SessionUserRow {
    id: i64,
    username: String,
    name: String,
    role: String,
}

impl AuthUser {
    pub fn require_inspector(&self) -> ApiResult<()> {
        if self.role != role::INSPECTOR {
            return Err(ApiError::Forbidden("仅巡查员可执行举证/复查操作"));
        }
        Ok(())
    }

    pub fn require_officer(&self) -> ApiResult<()> {
        if self.role != role::OFFICER {
            return Err(ApiError::Forbidden("仅值班员可执行核实/通知/结案操作"));
        }
        Ok(())
    }
}

fn token_from_query(parts: &Parts) -> Option<String> {
    let query = parts.uri.query()?;
    for pair in query.split('&') {
        if let Some(v) = pair.strip_prefix("token=") {
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .map(str::to_string)
            .or_else(|| token_from_query(parts))
            .ok_or(ApiError::Unauthorized)?;

        let row = sqlx::query_as::<_, SessionUserRow>(
            "SELECT u.id, u.username, u.name, u.role \
             FROM sessions s JOIN users u ON u.id = s.user_id \
             WHERE s.token = ?",
        )
        .bind(&token)
        .fetch_optional(&state.db)
        .await?
        .ok_or(ApiError::Unauthorized)?;

        Ok(AuthUser {
            id: row.id,
            username: row.username,
            name: row.name,
            role: row.role,
        })
    }
}
