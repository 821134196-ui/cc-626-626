use axum::{
    body::Body,
    extract::{Multipart, Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{sqlite::SqliteRow, Row, SqlitePool};

use crate::auth::{Duty, Inspector, Role, User};
use crate::error::{AppError, AppResult};
use crate::sim::{recognize_plate, send_sms};
use crate::state::AppState;

const PHOTO_LEN_LIMIT: usize = 10 * 1024 * 1024;

// ---------- 序列化模型 ----------

#[derive(Serialize, sqlx::FromRow, Clone)]
struct EventView {
    id: i64,
    spot_no: String,
    confirmed_plate: Option<String>,
    status: String,
    first_reported_at: String,
    last_report_at: String,
    closed_at: Option<String>,
    last_left_at: Option<String>,
    created_by: i64,
    creator_name: String,
    report_count: i64,
    notice_count: i64,
}

#[derive(Serialize, sqlx::FromRow)]
struct ReportView {
    id: i64,
    event_id: i64,
    inspector_id: i64,
    inspector_name: String,
    plate_entered: String,
    occurred_at: String,
    photo_path: String,
    created_at: String,
    #[sqlx(default)]
    suggested_plate: Option<String>,
    #[sqlx(default)]
    confidence: Option<f64>,
    #[sqlx(default)]
    needs_manual: Option<bool>,
}

#[derive(Serialize, sqlx::FromRow)]
struct RecheckView {
    id: i64,
    event_id: i64,
    inspector_id: i64,
    inspector_name: String,
    vehicle_left: bool,
    photo_path: Option<String>,
    note: Option<String>,
    created_at: String,
}

#[derive(Serialize, sqlx::FromRow)]
struct NoticeView {
    id: i64,
    plate: String,
    message: String,
    status: String,
    created_at: String,
}

#[derive(Deserialize)]
pub struct LoginReq {
    pub username: String,
}

#[derive(Deserialize)]
pub struct VerifyReq {
    pub plate: String,
    pub note: Option<String>,
}

// ---------- 认证 ----------

pub async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginReq>,
) -> AppResult<Json<serde_json::Value>> {
    let row = sqlx::query(
        "SELECT id, username, display_name, role, token FROM users WHERE username = ?1",
    )
    .bind(req.username.trim())
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("用户名不存在".into()))?;

    Ok(Json(json!({
        "token": row.get::<String, _>("token"),
        "user": {
            "id": row.get::<i64, _>("id"),
            "username": row.get::<String, _>("username"),
            "display_name": row.get::<String, _>("display_name"),
            "role": row.get::<String, _>("role"),
        }
    })))
}

pub async fn me(user: User) -> Json<serde_json::Value> {
    Json(json!({
        "id": user.id,
        "username": user.username,
        "display_name": user.display_name,
        "role": user.role.as_str(),
    }))
}

// ---------- 事件查询 ----------

/// 巡查员可见:自己创建/参与的事件 + 所有待复查事件(任何巡查员都可到场复查);
/// 值班员可见:全部事件。
pub async fn list_events(
    State(state): State<AppState>,
    user: User,
) -> AppResult<Json<serde_json::Value>> {
    let events = match user.role {
        Role::Duty => sqlx::query_as::<_, EventView>(&format!(
            "{EVENT_SELECT} ORDER BY {EVENT_ORDER}"
        ))
        .fetch_all(&state.pool)
        .await?,
        Role::Inspector => {
            sqlx::query_as::<_, EventView>(&format!(
                "{EVENT_SELECT}
                 WHERE e.status = 'pending_recheck'
                    OR e.created_by = ?1
                    OR EXISTS (SELECT 1 FROM reports r WHERE r.event_id = e.id AND r.inspector_id = ?1)
                    OR EXISTS (SELECT 1 FROM rechecks rc WHERE rc.event_id = e.id AND rc.inspector_id = ?1)
                 ORDER BY {EVENT_ORDER}"
            ))
            .bind(user.id)
            .fetch_all(&state.pool)
            .await?
        }
    };
    Ok(Json(json!({ "events": events })))
}

const EVENT_SELECT: &str = "
SELECT e.id, e.spot_no, e.confirmed_plate, e.status,
       e.first_reported_at, e.last_report_at, e.closed_at, e.last_left_at,
       e.created_by, u.display_name AS creator_name,
       (SELECT COUNT(*) FROM reports r WHERE r.event_id = e.id) AS report_count,
       (SELECT COUNT(*) FROM notifications n WHERE n.event_id = e.id) AS notice_count
FROM events e JOIN users u ON u.id = e.created_by";

const EVENT_ORDER: &str = "CASE e.status
            WHEN 'pending_recheck' THEN 0
            WHEN 'pending_review' THEN 1
            WHEN 'verified' THEN 2
            ELSE 3 END,
         e.last_report_at DESC";

pub async fn event_detail(
    State(state): State<AppState>,
    user: User,
    Path(id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    let event = fetch_event(&state.pool, id).await?;
    ensure_can_view(&state.pool, &user, &event).await?;

    let reports = sqlx::query_as::<_, ReportView>(
        "SELECT r.id, r.event_id, r.inspector_id, u.display_name AS inspector_name,
                r.plate_entered, r.occurred_at, r.photo_path, r.created_at,
                g.suggested_plate, g.confidence, g.needs_manual
         FROM reports r
         JOIN users u ON u.id = r.inspector_id
         LEFT JOIN recognitions g ON g.report_id = r.id
         WHERE r.event_id = ?1 ORDER BY r.id",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;

    let rechecks = sqlx::query_as::<_, RecheckView>(
        "SELECT rc.id, rc.event_id, rc.inspector_id, u.display_name AS inspector_name,
                rc.vehicle_left, rc.photo_path, rc.note, rc.created_at
         FROM rechecks rc JOIN users u ON u.id = rc.inspector_id
         WHERE rc.event_id = ?1 ORDER BY rc.id",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;

    let notices = sqlx::query_as::<_, NoticeView>(
        "SELECT id, plate, message, status, created_at
         FROM notifications WHERE event_id = ?1 ORDER BY id",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;

    let verification: Option<(String, String, String)> = sqlx::query(
        "SELECT v.confirmed_plate, u.display_name, v.created_at
         FROM verifications v JOIN users u ON u.id = v.duty_id
         WHERE v.event_id = ?1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .map(|row| {
        (
            row.get::<String, _>("confirmed_plate"),
            row.get::<String, _>("display_name"),
            row.get::<String, _>("created_at"),
        )
    });

    Ok(Json(json!({
        "event": event,
        "reports": reports,
        "rechecks": rechecks,
        "notifications": notices,
        "verification": verification.map(|(plate, by, at)| json!({
            "confirmed_plate": plate, "duty_name": by, "created_at": at
        })),
    })))
}

async fn fetch_event(pool: &SqlitePool, id: i64) -> AppResult<EventView> {
    sqlx::query_as::<_, EventView>(
        "SELECT e.id, e.spot_no, e.confirmed_plate, e.status,
                e.first_reported_at, e.last_report_at, e.closed_at, e.last_left_at,
                e.created_by, u.display_name AS creator_name,
                (SELECT COUNT(*) FROM reports r WHERE r.event_id = e.id) AS report_count,
                (SELECT COUNT(*) FROM notifications n WHERE n.event_id = e.id) AS notice_count
         FROM events e JOIN users u ON u.id = e.created_by
         WHERE e.id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::NotFound("事件不存在".into()))
}

/// 资料权限:值班员可查看全部;巡查员仅可查看自己参与或待复查(职责需到场)的事件。
async fn ensure_can_view(pool: &SqlitePool, user: &User, event: &EventView) -> AppResult<()> {
    match user.role {
        Role::Duty => Ok(()),
        Role::Inspector => {
            if event.status == "pending_recheck" || event.created_by == user.id {
                return Ok(());
            }
            let involved: i64 = sqlx::query(
                "SELECT (EXISTS (SELECT 1 FROM reports WHERE event_id=?1 AND inspector_id=?2)
                       OR EXISTS (SELECT 1 FROM rechecks WHERE event_id=?1 AND inspector_id=?2))
                 AS v",
            )
            .bind(event.id)
            .bind(user.id)
            .fetch_one(pool)
            .await?
            .get("v");
            if involved == 1 {
                Ok(())
            } else {
                Err(AppError::Forbidden(
                    "无权查看该事件资料:仅限本人参与的事件或待复查车位".into(),
                ))
            }
        }
    }
}

// ---------- 巡查员:提交举报(举证) ----------

pub async fn submit_report(
    State(state): State<AppState>,
    inspector: Inspector,
    mut multipart: Multipart,
) -> AppResult<Response> {
    let user = inspector.0;
    let mut spot_no: Option<String> = None;
    let mut plate: Option<String> = None;
    let mut occurred_at: Option<String> = None;
    let mut photo_bytes: Option<Vec<u8>> = None;

    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(format!("multipart 解析失败: {e}")))?
    {
        match field.name() {
            Some("spot_no") => spot_no = Some(field.text().await.unwrap_or_default()),
            Some("plate") => plate = Some(field.text().await.unwrap_or_default()),
            Some("occurred_at") => occurred_at = Some(field.text().await.unwrap_or_default()),
            Some("photo") => {
                let mut buf = Vec::new();
                while let Some(chunk) = field
                    .chunk()
                    .await
                    .map_err(|e| AppError::BadRequest(e.to_string()))?
                {
                    buf.extend_from_slice(&chunk);
                    if buf.len() > PHOTO_LEN_LIMIT {
                        return Err(AppError::BadRequest("照片超过 10MB 限制".into()));
                    }
                }
                photo_bytes = Some(buf);
            }
            _ => {
                let _ = field.bytes().await;
            }
        }
    }

    let spot_no = spot_no
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::BadRequest("缺少车位号".into()))?;
    let plate = plate
        .map(|s| normalize_plate(&s))
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::BadRequest("缺少车牌号".into()))?;
    let photo = photo_bytes
        .filter(|b| !b.is_empty())
        .ok_or_else(|| AppError::BadRequest("缺少现场照片".into()))?;
    let occurred_at = occurred_at
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| Utc::now().to_rfc3339());
    let now = Utc::now().to_rfc3339();

    // 保存照片到本地 uploads(文件名不可猜测,避免越权直接 URL 拉取)
    let filename = format!("{}.jpg", hex::encode(rand::random::<[u8; 16]>()));
    tokio::fs::write(state.uploads_dir.join(&filename), &photo).await?;

    // ANPR 仅为辅助识别,结果入库留痕,不参与通知决策
    let recog = recognize_plate(&photo, &plate);

    let mut tx = state.pool.begin().await?;

    // 归并:同一车位"未关闭"且发生时间晚于最近一次确认离开时间的举报,
    // 并入当前事件;否则新建事件(车辆离开后再次驶入 → 重新立案)。
    let open: Option<SqliteRow> = sqlx::query(
        "SELECT id, status, last_left_at FROM events
         WHERE spot_no = ?1 AND status != 'closed'
         ORDER BY id DESC LIMIT 1",
    )
    .bind(&spot_no)
    .fetch_optional(&mut *tx)
    .await?;

    let merge = match open.as_ref() {
        Some(row) => {
            let last_left: Option<String> = row.get("last_left_at");
            // last_left 为 None:占用从未被确认离开 → 归并;
            // 有时间分界:只有发生时间晚于离开时间才算新一轮占用(补报的旧情况仍归入)
            last_left.as_deref().map_or(true, |t| occurred_at.as_str() > t)
        }
        None => false,
    };

    let event_id = if merge {
        let id: i64 = open.expect("checked").get("id");
        sqlx::query("UPDATE events SET last_report_at = ?1 WHERE id = ?2")
            // 发生时间可能补报为更早时间,取较晚者
            .bind(max_ts(&occurred_at, &now))
            .bind(id)
            .execute(&mut *tx)
            .await?;
        id
    } else {
        let r = sqlx::query(
            "INSERT INTO events
                (spot_no, status, first_reported_at, last_report_at, created_by)
             VALUES (?1, 'pending_review', ?2, ?2, ?3)",
        )
        .bind(&spot_no)
        .bind(&occurred_at)
        .bind(user.id)
        .execute(&mut *tx)
        .await?;
        r.last_insert_rowid()
    };

    let report_r = sqlx::query(
        "INSERT INTO reports (event_id, inspector_id, plate_entered, occurred_at, photo_path, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )
    .bind(event_id)
    .bind(user.id)
    .bind(&plate)
    .bind(&occurred_at)
    .bind(&filename)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    let report_id = report_r.last_insert_rowid();

    sqlx::query(
        "INSERT INTO recognitions (report_id, suggested_plate, confidence, needs_manual, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(report_id)
    .bind(recog.suggested_plate.as_deref())
    .bind(recog.confidence)
    .bind(recog.needs_manual)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    let event = fetch_event(&state.pool, event_id).await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "merged": merge,
            "event": event,
            "recognition": {
                "suggested_plate": recog.suggested_plate,
                "confidence": recog.confidence,
                "needs_manual": recog.needs_manual,
                "note": if recog.needs_manual {
                    "照片不清晰或无法识别,必须转人工核实车牌"
                } else {
                    "识别结果仅作辅助,仍需值班员核实"
                }
            }
        })),
    )
        .into_response())
}

// ---------- 值班员:人工核实 ----------

pub async fn verify_event(
    State(state): State<AppState>,
    duty: Duty,
    Path(id): Path<i64>,
    Json(req): Json<VerifyReq>,
) -> AppResult<Json<serde_json::Value>> {
    let event = fetch_event(&state.pool, id).await?;
    if event.status != "pending_review" {
        return Err(AppError::BadRequest(format!(
            "当前状态为「{}」,不能重复核实",
            status_label(&event.status)
        )));
    }
    let plate = normalize_plate(&req.plate);
    if plate.is_empty() {
        return Err(AppError::BadRequest("请填写人工核实确认的车牌".into()));
    }
    let now = Utc::now().to_rfc3339();
    let mut tx = state.pool.begin().await?;
    sqlx::query(
        "INSERT INTO verifications (event_id, duty_id, confirmed_plate, note, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(id)
    .bind(duty.0.id)
    .bind(&plate)
    .bind(req.note.as_deref())
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE events SET confirmed_plate = ?1, status = 'verified' WHERE id = ?2")
        .bind(&plate)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    Ok(Json(json!({ "event": fetch_event(&state.pool, id).await? })))
}

// ---------- 值班员:通知车主(模拟短信) ----------

#[derive(Deserialize)]
pub struct NotifyReq {
    #[serde(default)]
    pub note: Option<String>,
}

pub async fn notify_owner(
    State(state): State<AppState>,
    duty: Duty,
    Path(id): Path<i64>,
    body: Option<Json<NotifyReq>>,
) -> AppResult<Json<serde_json::Value>> {
    let event = fetch_event(&state.pool, id).await?;
    // 必须先人工核实;pending_recheck 下可再次催办(再次发短信,事件仍需复查)
    if event.status != "verified" && event.status != "pending_recheck" {
        return Err(AppError::BadRequest(
            "事件尚未经人工核实车牌,不能向车主发送通知".into(),
        ));
    }
    let plate = event
        .confirmed_plate
        .clone()
        .ok_or_else(|| AppError::BadRequest("缺少人工确认车牌,拒绝发送通知".into()))?;

    let message = format!(
        "【停车场管理】车牌 {} 的车辆占用了新能源专用车位 {}，请立即驶离。通知发送不代表车辆已离开,工作人员将现场复查。",
        plate, event.spot_no
    );
    let receipt = send_sms(&plate, &event.spot_no, &message);
    let now = Utc::now().to_rfc3339();

    let mut tx = state.pool.begin().await?;
    sqlx::query(
        "INSERT INTO notifications (event_id, duty_id, plate, message, status, created_at)
         VALUES (?1, ?2, ?3, ?4, 'sent', ?5)",
    )
    .bind(id)
    .bind(duty.0.id)
    .bind(&plate)
    .bind(&message)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE events SET status = 'pending_recheck' WHERE id = ?1 AND status != 'pending_recheck'")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    Ok(Json(json!({
        "event": fetch_event(&state.pool, id).await?,
        "sms": { "provider_msg_id": receipt.provider_msg_id, "status": receipt.status },
        "reminder": "短信发送成功不代表车辆离开,事件保持「待复查」,须由巡查员现场确认",
        "note": body.and_then(|b| b.0.note),
    })))
}

// ---------- 巡查员:现场复查(唯一的结案途径) ----------

pub async fn submit_recheck(
    State(state): State<AppState>,
    inspector: Inspector,
    Path(id): Path<i64>,
    mut multipart: Multipart,
) -> AppResult<Response> {
    let user = inspector.0;
    let event = fetch_event(&state.pool, id).await?;
    // 巡查员只能对自己可见的待复查事件操作
    ensure_can_view(&state.pool, &user, &event).await?;
    if event.status != "pending_recheck" {
        return Err(AppError::BadRequest(format!(
            "仅「待复查」事件可提交复查结果(当前:「{}」);未通知车主或已关闭的事件不能复查结案",
            status_label(&event.status)
        )));
    }

    let mut vehicle_left: Option<bool> = None;
    let mut note: Option<String> = None;
    let mut photo: Option<Vec<u8>> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(format!("multipart 解析失败: {e}")))?
    {
        match field.name() {
            Some("vehicle_left") => {
                let v = field.text().await.unwrap_or_default();
                vehicle_left = Some(matches!(v.trim(), "true" | "1" | "yes"));
            }
            Some("note") => note = Some(field.text().await.unwrap_or_default()),
            Some("photo") => {
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|e| AppError::BadRequest(e.to_string()))?;
                if !bytes.is_empty() {
                    photo = Some(bytes.to_vec());
                }
            }
            _ => {
                let _ = field.bytes().await;
            }
        }
    }

    let vehicle_left =
        vehicle_left.ok_or_else(|| AppError::BadRequest("缺少 vehicle_left 字段".into()))?;

    let now = Utc::now().to_rfc3339();
    let photo_path = if let Some(bytes) = photo {
        let filename = format!("recheck-{}.jpg", hex::encode(rand::random::<[u8; 16]>()));
        tokio::fs::write(state.uploads_dir.join(&filename), bytes).await?;
        Some(filename)
    } else {
        None
    };

    let mut tx = state.pool.begin().await?;
    sqlx::query(
        "INSERT INTO rechecks (event_id, inspector_id, vehicle_left, photo_path, note, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )
    .bind(id)
    .bind(user.id)
    .bind(vehicle_left)
    .bind(photo_path)
    .bind(note.as_deref())
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    if vehicle_left {
        // 确认离开 → 关闭事件,并记录离开时间作为新一轮占用的归并分界
        sqlx::query(
            "UPDATE events SET status='closed', closed_at=?1, last_left_at=?1 WHERE id=?2",
        )
        .bind(&now)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    }
    // vehicle_left=false:状态保持 pending_recheck,事件仍显示待处理。
    tx.commit().await?;

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "event": fetch_event(&state.pool, id).await?,
            "vehicle_left": vehicle_left,
            "message": if vehicle_left {
                "已确认车辆离开,事件关闭"
            } else {
                "车辆仍占用,事件保持待复查,值班员可再次催办"
            }
        })),
    )
        .into_response())
}

// ---------- 照片(受权限控制的本地文件) ----------

pub async fn photo(
    State(state): State<AppState>,
    user: User,
    Path(filename): Path<String>,
) -> AppResult<Response> {
    // 只允许单段文件名,杜绝路径穿越
    if filename.contains('/') || filename.contains("..") || filename.contains('\\') {
        return Err(AppError::BadRequest("非法文件名".into()));
    }

    // 反查照片所属事件并做资料权限校验
    let row = sqlx::query(
        "SELECT r.event_id AS event_id FROM reports r WHERE r.photo_path = ?1
         UNION ALL
         SELECT rc.event_id FROM rechecks rc WHERE rc.photo_path = ?1
         LIMIT 1",
    )
    .bind(&filename)
    .bind(&filename)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("照片不存在".into()))?;
    let event_id: i64 = row.get("event_id");
    let event = fetch_event(&state.pool, event_id).await?;
    ensure_can_view(&state.pool, &user, &event).await?;

    let path = state.uploads_dir.join(&filename);
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| AppError::NotFound("照片文件已丢失".into()))?;

    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, "image/jpeg".parse().unwrap());
    headers.insert(
        header::CACHE_CONTROL,
        "private, no-store".parse().unwrap(),
    );
    Ok((StatusCode::OK, headers, Body::from(bytes)).into_response())
}

// ---------- 静态资源 / SPA 回退 ----------

/// 未知路径:/api 下返回 JSON 404;静态文件存在则返回;其余路由回退 index.html(200)。
pub async fn spa_fallback(
    State(state): State<AppState>,
    uri: axum::http::Uri,
) -> AppResult<Response> {
    let path = uri.path();
    if path.starts_with("/api/") || path == "/api" {
        return Err(AppError::NotFound("接口不存在".into()));
    }
    let dir = match state.static_dir.as_ref() {
        Some(d) => d.clone(),
        None => return Err(AppError::NotFound("未构建前端静态资源".into())),
    };

    // 解析为相对路径,拒绝穿越
    let rel = path.trim_start_matches('/');
    if rel.contains("..") || rel.contains('\\') || rel.contains('\0') {
        return Err(AppError::BadRequest("非法路径".into()));
    }

    if !rel.is_empty() {
        let candidate = dir.join(rel);
        if candidate.is_file() {
            let bytes = tokio::fs::read(&candidate).await?;
            let ct = mime_for(rel);
            let mut headers = HeaderMap::new();
            headers.insert(header::CONTENT_TYPE, ct.parse().unwrap());
            return Ok((StatusCode::OK, headers, Body::from(bytes)).into_response());
        }
    }

    let index = tokio::fs::read(dir.join("index.html")).await?;
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, "text/html; charset=utf-8".parse().unwrap());
    Ok((StatusCode::OK, headers, Body::from(index)).into_response())
}

fn mime_for(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or_default() {
        "js" => "application/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "html" => "text/html; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "map" => "application/json",
        _ => "application/octet-stream",
    }
}

// ---------- 工具 ----------

fn normalize_plate(s: &str) -> String {
    s.trim().to_uppercase().replace(' ', "")
}

fn max_ts(a: &str, b: &str) -> String {
    if a >= b { a.to_string() } else { b.to_string() }
}

pub fn status_label(s: &str) -> &'static str {
    match s {
        "pending_review" => "待核实",
        "verified" => "已核实待通知",
        "pending_recheck" => "待复查",
        "closed" => "已关闭",
        _ => "未知",
    }
}
