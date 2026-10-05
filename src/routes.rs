use axum::{
    extract::{DefaultBodyLimit, Multipart, Path, State},
    http::header,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tower_http::services::{ServeDir, ServeFile};
use uuid::Uuid;

use crate::{
    auth::{hash_password, AuthUser},
    error::{ApiError, ApiResult},
    mocks,
    models::{
        role, status, EventDetail, EventRow, EventSummary, NotificationView, RecheckView,
        ReportView,
    },
    state::AppState,
};

pub fn build(state: AppState) -> Router {
    let static_dir = std::env::var("STATIC_DIR").unwrap_or_else(|_| "frontend/dist".to_string());
    Router::new()
        .route("/api/login", post(login))
        .route("/api/me", get(me))
        .route("/api/reports", post(create_report))
        .route("/api/events", get(list_events))
        .route("/api/events/{id}", get(get_event))
        .route("/api/events/{id}/verify", post(verify_event))
        .route("/api/events/{id}/reject", post(reject_event))
        .route("/api/events/{id}/notify", post(notify_event))
        .route("/api/events/{id}/rechecks", post(create_recheck))
        .route("/api/events/{id}/close", post(close_event))
        .route("/api/reports/{id}/photo", get(report_photo))
        .layer(DefaultBodyLimit::max(20 * 1024 * 1024))
        .fallback_service(
            ServeDir::new(static_dir.clone())
                .not_found_service(ServeFile::new(format!("{static_dir}/index.html"))),
        )
        .with_state(state)
}

fn now() -> String {
    // 微秒精度：保证"通知→复查"等相邻事件的先后次序可被字符串比较正确表达
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

// ---------- 登录 ----------

#[derive(Deserialize)]
struct LoginReq {
    username: String,
    password: String,
}

#[derive(sqlx::FromRow)]
struct UserRow {
    id: i64,
    username: String,
    name: String,
    role: String,
    password_hash: String,
}

async fn login(State(s): State<AppState>, Json(req): Json<LoginReq>) -> ApiResult<Json<Value>> {
    let user = sqlx::query_as::<_, UserRow>(
        "SELECT id, username, name, role, password_hash FROM users WHERE username = ?",
    )
    .bind(req.username.trim())
    .fetch_optional(&s.db)
    .await?
    .ok_or(ApiError::Unauthorized)?;

    if user.password_hash != hash_password(&req.password) {
        return Err(ApiError::Unauthorized);
    }

    let token = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO sessions (token, user_id, created_at) VALUES (?, ?, ?)")
        .bind(&token)
        .bind(user.id)
        .bind(now())
        .execute(&s.db)
        .await?;

    Ok(Json(json!({
        "token": token,
        "user": { "id": user.id, "username": user.username, "name": user.name, "role": user.role }
    })))
}

async fn me(user: AuthUser) -> Json<Value> {
    Json(json!({
        "id": user.id, "username": user.username, "name": user.name, "role": user.role
    }))
}

// ---------- 巡查员：举证（重复举报自动归并） ----------

async fn create_report(
    State(s): State<AppState>,
    user: AuthUser,
    mut multipart: Multipart,
) -> ApiResult<Json<Value>> {
    user.require_inspector()?;

    let mut spot_number: Option<String> = None;
    let mut plate: Option<String> = None;
    let mut occurred_at: Option<String> = None;
    let mut photo: Option<(String, Vec<u8>)> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::BadRequest(format!("表单解析失败: {e}")))?
    {
        match field.name().unwrap_or("") {
            "spot_number" => {
                spot_number = Some(
                    field
                        .text()
                        .await
                        .map_err(|e| ApiError::BadRequest(e.to_string()))?,
                )
            }
            "plate" => {
                plate = Some(
                    field
                        .text()
                        .await
                        .map_err(|e| ApiError::BadRequest(e.to_string()))?,
                )
            }
            "occurred_at" => {
                occurred_at = Some(
                    field
                        .text()
                        .await
                        .map_err(|e| ApiError::BadRequest(e.to_string()))?,
                )
            }
            "photo" => {
                let filename = field.file_name().unwrap_or("photo.jpg").to_string();
                let data = field
                    .bytes()
                    .await
                    .map_err(|e| ApiError::BadRequest(format!("照片读取失败: {e}")))?
                    .to_vec();
                photo = Some((filename, data));
            }
            _ => {}
        }
    }

    let spot_number = spot_number
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .ok_or_else(|| ApiError::BadRequest("车位号不能为空".into()))?;
    let plate = plate
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .ok_or_else(|| ApiError::BadRequest("车牌号不能为空".into()))?;
    let occurred_at = occurred_at
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(now);
    let (orig_name, data) = photo.ok_or_else(|| ApiError::BadRequest("必须上传现场照片".into()))?;
    if data.is_empty() {
        return Err(ApiError::BadRequest("照片内容为空".into()));
    }

    // 照片保存到本地 uploads 目录
    let ext = orig_name
        .rsplit('.')
        .next()
        .filter(|e| e.len() <= 5 && e.chars().all(|c| c.is_ascii_alphanumeric()))
        .unwrap_or("jpg")
        .to_ascii_lowercase();
    let filename = format!("{}.{ext}", Uuid::new_v4());
    tokio::fs::write(s.upload_dir.join(&filename), &data).await?;

    // 模拟车牌识别，结果仅作辅助
    let rec = mocks::recognize_plate(&data, &orig_name, &plate);

    let now = now();
    let mut tx = s.db.begin().await?;

    // 同一车位存在未结案事件 → 重复举报归并；否则（首次占用或车辆离开后再驶入）新建事件
    let open_event_id = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM events \
         WHERE spot_number = ? AND status IN ('pending_verify','verified','notified') \
         ORDER BY id DESC LIMIT 1",
    )
    .bind(&spot_number)
    .fetch_optional(&mut *tx)
    .await?;

    let (event_id, merged) = match open_event_id {
        Some(id) => (id, true),
        None => {
            let id = sqlx::query_scalar::<_, i64>(
                "INSERT INTO events (spot_number, plate_reported, status, created_by, created_at) \
                 VALUES (?, ?, 'pending_verify', ?, ?) RETURNING id",
            )
            .bind(&spot_number)
            .bind(&plate)
            .bind(user.id)
            .bind(&now)
            .fetch_one(&mut *tx)
            .await?;
            (id, false)
        }
    };

    let report_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO reports \
           (event_id, reporter_id, spot_number, plate, occurred_at, photo_path, \
            recognized_plate, recognition_confidence, needs_manual_review, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(event_id)
    .bind(user.id)
    .bind(&spot_number)
    .bind(&plate)
    .bind(&occurred_at)
    .bind(&filename)
    .bind(rec.plate.as_deref())
    .bind(f64::from(rec.confidence))
    .bind(rec.needs_manual_review as i64)
    .bind(&now)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(Json(json!({
        "event_id": event_id,
        "report_id": report_id,
        "merged": merged,
        "recognition": rec,
    })))
}

// ---------- 事件列表（按角色过滤、排序） ----------

#[derive(sqlx::FromRow)]
struct EventListRow {
    id: i64,
    spot_number: String,
    plate_reported: String,
    plate_confirmed: Option<String>,
    status: String,
    created_at: String,
    notified_count: i64,
    last_notified_at: Option<String>,
    report_count: i64,
    manual_count: i64,
    left_rechecks: i64,
}

const EVENT_SELECT: &str = "\
SELECT e.id, e.spot_number, e.plate_reported, e.plate_confirmed, e.status, e.created_at, \
       e.notified_count, e.last_notified_at, \
       (SELECT COUNT(*) FROM reports r WHERE r.event_id = e.id) AS report_count, \
       (SELECT COUNT(*) FROM reports r WHERE r.event_id = e.id AND r.needs_manual_review = 1) AS manual_count, \
       (SELECT COUNT(*) FROM rechecks c WHERE c.event_id = e.id AND c.result = 'left' \
          AND (e.last_notified_at IS NULL OR c.created_at > e.last_notified_at)) AS left_rechecks \
FROM events e";

fn to_summary(row: EventListRow) -> EventSummary {
    let notified = row.status == status::NOTIFIED;
    EventSummary {
        id: row.id,
        spot_number: row.spot_number,
        plate_reported: row.plate_reported,
        plate_confirmed: row.plate_confirmed,
        status: row.status,
        created_at: row.created_at,
        notified_count: row.notified_count,
        last_notified_at: row.last_notified_at,
        report_count: row.report_count,
        needs_manual_review: row.manual_count > 0,
        pending_recheck: notified && row.left_rechecks == 0,
        can_close: notified && row.left_rechecks > 0,
    }
}

async fn list_events(
    State(s): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<Vec<EventSummary>>> {
    // 值班员可见全部；巡查员仅见"自己举报的"与"待复查的"事件
    let rows = if user.role == role::OFFICER {
        sqlx::query_as::<_, EventListRow>(EVENT_SELECT)
            .fetch_all(&s.db)
            .await?
    } else {
        sqlx::query_as::<_, EventListRow>(&format!(
            "{EVENT_SELECT} WHERE e.status = 'notified' \
             OR EXISTS (SELECT 1 FROM reports r WHERE r.event_id = e.id AND r.reporter_id = ?)"
        ))
        .bind(user.id)
        .fetch_all(&s.db)
        .await?
    };

    let mut events: Vec<EventSummary> = rows.into_iter().map(to_summary).collect();
    // 巡查员手机端优先展示待复查车位；值班员优先展示待核实
    let rank = |st: &str| -> u8 {
        if user.role == role::OFFICER {
            match st {
                status::PENDING_VERIFY => 0,
                status::VERIFIED => 1,
                status::NOTIFIED => 2,
                status::CLOSED => 3,
                _ => 4,
            }
        } else {
            match st {
                status::NOTIFIED => 0,
                status::PENDING_VERIFY => 1,
                status::VERIFIED => 2,
                status::CLOSED => 3,
                _ => 4,
            }
        }
    };
    events.sort_by(|a, b| rank(&a.status).cmp(&rank(&b.status)).then(b.created_at.cmp(&a.created_at)));
    Ok(Json(events))
}

// ---------- 事件详情（资料受权限控制） ----------

async fn load_event(s: &AppState, id: i64) -> ApiResult<EventRow> {
    sqlx::query_as::<_, EventRow>("SELECT * FROM events WHERE id = ?")
        .bind(id)
        .fetch_optional(&s.db)
        .await?
        .ok_or(ApiError::NotFound("事件不存在"))
}

/// 值班员可看全部；巡查员只能看自己参与举报的、或待复查（已通知）的事件
async fn check_event_visible(s: &AppState, user: &AuthUser, event: &EventRow) -> ApiResult<()> {
    if user.role == role::OFFICER || event.status == status::NOTIFIED {
        return Ok(());
    }
    let mine = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM reports WHERE event_id = ? AND reporter_id = ?",
    )
    .bind(event.id)
    .bind(user.id)
    .fetch_one(&s.db)
    .await?;
    if mine > 0 {
        Ok(())
    } else {
        Err(ApiError::Forbidden("无权查看该事件资料"))
    }
}

async fn get_event(
    State(s): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<EventDetail>> {
    let event = load_event(&s, id).await?;
    check_event_visible(&s, &user, &event).await?;

    let reports = sqlx::query_as::<_, ReportRowJoined>(
        "SELECT r.id, u.name AS reporter, r.plate, r.occurred_at, r.recognized_plate, \
                r.recognition_confidence, r.needs_manual_review, r.created_at \
         FROM reports r JOIN users u ON u.id = r.reporter_id \
         WHERE r.event_id = ? ORDER BY r.id",
    )
    .bind(id)
    .fetch_all(&s.db)
    .await?;

    let notifications = sqlx::query_as::<_, NotificationRowJoined>(
        "SELECT n.id, u.name AS sender, n.plate, n.phone, n.content, n.status, n.created_at \
         FROM notifications n JOIN users u ON u.id = n.sent_by \
         WHERE n.event_id = ? ORDER BY n.id",
    )
    .bind(id)
    .fetch_all(&s.db)
    .await?;

    let rechecks = sqlx::query_as::<_, RecheckRowJoined>(
        "SELECT c.id, u.name AS inspector, c.result, c.note, c.created_at \
         FROM rechecks c JOIN users u ON u.id = c.inspector_id \
         WHERE c.event_id = ? ORDER BY c.id",
    )
    .bind(id)
    .fetch_all(&s.db)
    .await?;

    let created_by_name = user_name(&s, event.created_by).await;
    let verified_by_name = match event.verified_by {
        Some(uid) => Some(user_name(&s, uid).await),
        None => None,
    };
    let closed_by_name = match event.closed_by {
        Some(uid) => Some(user_name(&s, uid).await),
        None => None,
    };

    let left_after_notify = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM rechecks WHERE event_id = ? AND result = 'left' \
         AND (? IS NULL OR created_at > ?)",
    )
    .bind(id)
    .bind(event.last_notified_at.clone())
    .bind(event.last_notified_at.clone())
    .fetch_one(&s.db)
    .await?;

    let notified = event.status == status::NOTIFIED;
    Ok(Json(EventDetail {
        id: event.id,
        spot_number: event.spot_number,
        plate_reported: event.plate_reported,
        plate_confirmed: event.plate_confirmed,
        status: event.status,
        created_at: event.created_at,
        created_by_name,
        verified_by_name,
        verified_at: event.verified_at,
        notified_count: event.notified_count,
        last_notified_at: event.last_notified_at,
        closed_by_name,
        closed_at: event.closed_at,
        close_note: event.close_note,
        pending_recheck: notified && left_after_notify == 0,
        can_close: notified && left_after_notify > 0,
        reports: reports
            .into_iter()
            .map(|r| ReportView {
                id: r.id,
                reporter: r.reporter,
                plate: r.plate,
                occurred_at: r.occurred_at,
                recognized_plate: r.recognized_plate,
                recognition_confidence: r.recognition_confidence,
                needs_manual_review: r.needs_manual_review != 0,
                created_at: r.created_at,
            })
            .collect(),
        notifications: notifications
            .into_iter()
            .map(|n| NotificationView {
                id: n.id,
                sender: n.sender,
                plate: n.plate,
                phone: n.phone,
                content: n.content,
                status: n.status,
                created_at: n.created_at,
            })
            .collect(),
        rechecks: rechecks
            .into_iter()
            .map(|c| RecheckView {
                id: c.id,
                inspector: c.inspector,
                result: c.result,
                note: c.note,
                created_at: c.created_at,
            })
            .collect(),
    }))
}

async fn user_name(s: &AppState, id: i64) -> String {
    sqlx::query_scalar::<_, String>("SELECT name FROM users WHERE id = ?")
        .bind(id)
        .fetch_optional(&s.db)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| "未知".to_string())
}

#[derive(sqlx::FromRow)]
struct ReportRowJoined {
    id: i64,
    reporter: String,
    plate: String,
    occurred_at: String,
    recognized_plate: Option<String>,
    recognition_confidence: Option<f64>,
    needs_manual_review: i64,
    created_at: String,
}

#[derive(sqlx::FromRow)]
struct NotificationRowJoined {
    id: i64,
    sender: String,
    plate: String,
    phone: String,
    content: String,
    status: String,
    created_at: String,
}

#[derive(sqlx::FromRow)]
struct RecheckRowJoined {
    id: i64,
    inspector: String,
    result: String,
    note: Option<String>,
    created_at: String,
}

// ---------- 值班员：核实 / 驳回 / 通知 / 结案 ----------

#[derive(Deserialize)]
struct VerifyReq {
    confirmed_plate: Option<String>,
}

async fn verify_event(
    State(s): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
    Json(req): Json<VerifyReq>,
) -> ApiResult<Json<Value>> {
    user.require_officer()?;
    // 车牌必须由值班员人工确认后填写，识别结果只作辅助，不能直接采用
    let plate = req
        .confirmed_plate
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .ok_or_else(|| ApiError::BadRequest("核实须由值班员人工确认车牌号（识别结果仅作辅助）".into()))?;

    let event = load_event(&s, id).await?;
    if event.status != status::PENDING_VERIFY {
        return Err(ApiError::Conflict("仅待核实事件可执行核实".into()));
    }

    sqlx::query(
        "UPDATE events SET status = 'verified', plate_confirmed = ?, verified_by = ?, verified_at = ? \
         WHERE id = ? AND status = 'pending_verify'",
    )
    .bind(&plate)
    .bind(user.id)
    .bind(now())
    .bind(id)
    .execute(&s.db)
    .await?;

    Ok(Json(json!({ "ok": true, "status": "verified", "plate_confirmed": plate })))
}

#[derive(Deserialize)]
struct RejectReq {
    reason: Option<String>,
}

async fn reject_event(
    State(s): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
    Json(req): Json<RejectReq>,
) -> ApiResult<Json<Value>> {
    user.require_officer()?;
    let event = load_event(&s, id).await?;
    if event.status != status::PENDING_VERIFY {
        return Err(ApiError::Conflict("仅待核实事件可驳回".into()));
    }
    sqlx::query(
        "UPDATE events SET status = 'rejected', closed_by = ?, closed_at = ?, close_note = ? \
         WHERE id = ? AND status = 'pending_verify'",
    )
    .bind(user.id)
    .bind(now())
    .bind(req.reason.unwrap_or_else(|| "证据不足，驳回".into()))
    .bind(id)
    .execute(&s.db)
    .await?;
    Ok(Json(json!({ "ok": true, "status": "rejected" })))
}

async fn notify_event(
    State(s): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    user.require_officer()?;
    let event = load_event(&s, id).await?;
    if event.status != status::VERIFIED && event.status != status::NOTIFIED {
        return Err(ApiError::Conflict("事件须先核实才能通知车主".into()));
    }
    // 只使用值班员人工确认的车牌，绝不直接采用识别结果
    let plate = event
        .plate_confirmed
        .ok_or_else(|| ApiError::Conflict("缺少已确认车牌，无法通知车主".into()))?;

    let phone = mocks::lookup_phone(&plate);
    let content = format!(
        "【新能源专用车位】您的车辆{plate}占用新能源专用车位{}（{}），请尽快驶离。",
        event.spot_number, event.created_at
    );
    let sms_status = mocks::send_sms(&phone, &content).map_err(ApiError::BadRequest)?;

    let now = now();
    let mut tx = s.db.begin().await?;
    sqlx::query(
        "INSERT INTO notifications (event_id, sent_by, plate, phone, content, status, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(user.id)
    .bind(&plate)
    .bind(&phone)
    .bind(&content)
    .bind(&sms_status)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE events SET status = 'notified', notified_count = notified_count + 1, last_notified_at = ? \
         WHERE id = ?",
    )
    .bind(&now)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(Json(json!({
        "ok": true,
        "status": "notified",
        "phone": phone,
        "content": content,
        "sms_status": sms_status,
        "hint": "短信发送成功不代表车辆已驶离，仍须巡查员复查确认后才能结案"
    })))
}

// ---------- 巡查员：复查 ----------

#[derive(Deserialize)]
struct RecheckReq {
    result: String,
    note: Option<String>,
}

async fn create_recheck(
    State(s): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
    Json(req): Json<RecheckReq>,
) -> ApiResult<Json<Value>> {
    user.require_inspector()?;
    if req.result != "left" && req.result != "still_occupied" {
        return Err(ApiError::BadRequest("复查结果须为 left(已离开) 或 still_occupied(仍占用)".into()));
    }
    let event = load_event(&s, id).await?;
    if event.status != status::NOTIFIED {
        return Err(ApiError::Conflict("仅已通知待复查的事件可登记复查".into()));
    }

    sqlx::query(
        "INSERT INTO rechecks (event_id, inspector_id, result, note, created_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(user.id)
    .bind(&req.result)
    .bind(req.note)
    .bind(now())
    .execute(&s.db)
    .await?;

    Ok(Json(json!({ "ok": true, "result": req.result })))
}

// ---------- 值班员：结案（必须复查确认离开，短信成功不算数） ----------

#[derive(Deserialize)]
struct CloseReq {
    note: Option<String>,
}

async fn close_event(
    State(s): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
    Json(req): Json<CloseReq>,
) -> ApiResult<Json<Value>> {
    user.require_officer()?;
    let event = load_event(&s, id).await?;
    if event.status != status::NOTIFIED {
        return Err(ApiError::Conflict("仅已通知待复查的事件可结案".into()));
    }

    // 关键规则：最后一次通知之后必须有"已离开"复查记录；短信发送成功不能代替复查
    let left_after_notify = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM rechecks WHERE event_id = ? AND result = 'left' AND created_at > ?",
    )
    .bind(id)
    .bind(event.last_notified_at.clone().unwrap_or_default())
    .fetch_one(&s.db)
    .await?;
    if left_after_notify == 0 {
        return Err(ApiError::Conflict(
            "复查确认车辆已离开后才能结案（短信发送成功不能代替复查）".into(),
        ));
    }

    sqlx::query(
        "UPDATE events SET status = 'closed', closed_by = ?, closed_at = ?, close_note = ? \
         WHERE id = ? AND status = 'notified'",
    )
    .bind(user.id)
    .bind(now())
    .bind(req.note)
    .bind(id)
    .execute(&s.db)
    .await?;

    Ok(Json(json!({ "ok": true, "status": "closed" })))
}

// ---------- 照片（随事件资料一并受权限控制） ----------

async fn report_photo(
    State(s): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> ApiResult<Response> {
    #[derive(sqlx::FromRow)]
    struct PhotoRow {
        photo_path: String,
        event_id: i64,
    }
    let row = sqlx::query_as::<_, PhotoRow>("SELECT photo_path, event_id FROM reports WHERE id = ?")
        .bind(id)
        .fetch_optional(&s.db)
        .await?
        .ok_or(ApiError::NotFound("照片不存在"))?;

    let event = load_event(&s, row.event_id).await?;
    check_event_visible(&s, &user, &event).await?;

    let bytes = tokio::fs::read(s.upload_dir.join(&row.photo_path)).await?;
    let mime = match row.photo_path.rsplit('.').next() {
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        _ => "image/jpeg",
    };
    Ok(([(header::CONTENT_TYPE, mime)], bytes).into_response())
}
