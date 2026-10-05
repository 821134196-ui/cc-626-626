use axum::{
    extract::FromRequestParts,
    http::{header::AUTHORIZATION, request::Parts},
};
use sqlx::sqlite::SqliteRow;
use sqlx::Row;

use crate::error::AppError;
use crate::state::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Inspector,
    Duty,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Inspector => "inspector",
            Role::Duty => "duty",
        }
    }
    fn parse(s: &str) -> Option<Self> {
        match s {
            "inspector" => Some(Role::Inspector),
            "duty" => Some(Role::Duty),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub display_name: String,
    pub role: Role,
}

impl User {
    fn from_row(row: SqliteRow) -> Self {
        let role_str: String = row.get("role");
        User {
            id: row.get("id"),
            username: row.get("username"),
            display_name: row.get("display_name"),
            role: Role::parse(&role_str).expect("db role valid"),
        }
    }
}

/// 从 Authorization: Bearer <token> 解析当前用户(Axum 0.8 原生 async trait)。
impl FromRequestParts<AppState> for User {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or_else(|| {
                AppError::NotFound("缺少登录令牌(Authorization: Bearer)".into())
            })?
            .trim();

        let row = sqlx::query(
            "SELECT id, username, display_name, role FROM users WHERE token = ?1",
        )
        .bind(token)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::NotFound("令牌无效或用户不存在".into()))?;

        Ok(User::from_row(row))
    }
}

/// 仅限巡查员
pub struct Inspector(pub User);

impl FromRequestParts<AppState> for Inspector {
    type Rejection = AppError;
    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let user = User::from_request_parts(parts, state).await?;
        match user.role {
            Role::Inspector => Ok(Inspector(user)),
            _ => Err(AppError::Forbidden(format!(
                "该操作仅巡查员可执行(当前身份:值班员 {})",
                user.display_name
            ))),
        }
    }
}

/// 仅限值班员
pub struct Duty(pub User);

impl FromRequestParts<AppState> for Duty {
    type Rejection = AppError;
    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let user = User::from_request_parts(parts, state).await?;
        match user.role {
            Role::Duty => Ok(Duty(user)),
            _ => Err(AppError::Forbidden(format!(
                "该操作仅值班员可执行(当前身份:巡查员 {})",
                user.display_name
            ))),
        }
    }
}
