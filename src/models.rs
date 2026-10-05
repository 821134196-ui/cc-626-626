use serde::Serialize;

/// 事件状态机：
/// pending_verify(待核实) --核实--> verified(待通知) --通知--> notified(待复查)
///      |                                                          |
///      +--驳回--> rejected(已驳回)          复查确认离开+结案 <------+
/// notified 状态下复查仍为占用可再次通知；只有最后一次通知之后
/// 出现"已离开"复查记录，值班员才能结案 --> closed。
pub mod status {
    pub const PENDING_VERIFY: &str = "pending_verify";
    pub const VERIFIED: &str = "verified";
    pub const NOTIFIED: &str = "notified";
    pub const CLOSED: &str = "closed";
    pub const REJECTED: &str = "rejected";
}

pub mod role {
    pub const INSPECTOR: &str = "inspector"; // 巡查员：举证、复查
    pub const OFFICER: &str = "officer"; // 值班员：核实、通知、结案
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct EventRow {
    pub id: i64,
    pub spot_number: String,
    pub plate_reported: String,
    pub plate_confirmed: Option<String>,
    pub status: String,
    pub created_by: i64,
    pub created_at: String,
    pub verified_by: Option<i64>,
    pub verified_at: Option<String>,
    pub notified_count: i64,
    pub last_notified_at: Option<String>,
    pub closed_by: Option<i64>,
    pub closed_at: Option<String>,
    pub close_note: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct EventSummary {
    pub id: i64,
    pub spot_number: String,
    pub plate_reported: String,
    pub plate_confirmed: Option<String>,
    pub status: String,
    pub created_at: String,
    pub notified_count: i64,
    pub last_notified_at: Option<String>,
    pub report_count: i64,
    pub needs_manual_review: bool,
    /// 已通知但最后一次通知后尚无"已离开"复查 —— 手机端优先展示的待复查对象
    pub pending_recheck: bool,
    /// 值班员可否结案（已通知且最后通知后已有"已离开"复查）
    pub can_close: bool,
}

#[derive(Debug, Serialize)]
pub struct ReportView {
    pub id: i64,
    pub reporter: String,
    pub plate: String,
    pub occurred_at: String,
    pub recognized_plate: Option<String>,
    pub recognition_confidence: Option<f64>,
    pub needs_manual_review: bool,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct NotificationView {
    pub id: i64,
    pub sender: String,
    pub plate: String,
    pub phone: String,
    pub content: String,
    pub status: String,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct RecheckView {
    pub id: i64,
    pub inspector: String,
    pub result: String,
    pub note: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct EventDetail {
    pub id: i64,
    pub spot_number: String,
    pub plate_reported: String,
    pub plate_confirmed: Option<String>,
    pub status: String,
    pub created_at: String,
    pub created_by_name: String,
    pub verified_by_name: Option<String>,
    pub verified_at: Option<String>,
    pub notified_count: i64,
    pub last_notified_at: Option<String>,
    pub closed_by_name: Option<String>,
    pub closed_at: Option<String>,
    pub close_note: Option<String>,
    pub pending_recheck: bool,
    pub can_close: bool,
    pub reports: Vec<ReportView>,
    pub notifications: Vec<NotificationView>,
    pub rechecks: Vec<RecheckView>,
}
