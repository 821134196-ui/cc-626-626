// 端到端集成测试:在临时 SQLite + 临时 uploads 上跑真实 HTTP 路由。
// 覆盖:重复举报归并、离开后重新立案、岗位权限、资料越权访问、
// ANPR 仅辅助(必须人工核实)、短信成功不能代替复查。

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::Router;
use ev_parking_evidence::{router, test_state};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

const INSPECTOR1: &str = "tok-inspector-1";
const INSPECTOR2: &str = "tok-inspector-2";
const DUTY: &str = "tok-duty-1";

struct App {
    router: Router,
}

impl App {
    async fn new() -> (Self, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let state = test_state(dir.path()).await;
        (App { router: router(state) }, dir)
    }

    async fn call(&self, req: Request<Body>) -> (StatusCode, Value) {
        let (status, bytes) = self.bytes(req).await;
        let v: Value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                panic!("非 JSON 响应({e}): {}", String::from_utf8_lossy(&bytes))
            })
        };
        (status, v)
    }

    async fn bytes(&self, req: Request<Body>) -> (StatusCode, axum::body::Bytes) {
        let res = self.router.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (status, bytes)
    }
}

fn multipart(fields: &[(&str, &str)], photo: Option<&[u8]>) -> (String, Vec<u8>) {
    let boundary = "----evtestboundary7MA4YWxkTrZu0gW";
    let mut body: Vec<u8> = Vec::new();
    for (name, value) in fields {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
        );
        body.extend_from_slice(value.as_bytes());
        body.extend_from_slice(b"\r\n");
    }
    if let Some(bytes) = photo {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            b"Content-Disposition: form-data; name=\"photo\"; filename=\"shot.jpg\"\r\n",
        );
        body.extend_from_slice(b"Content-Type: image/jpeg\r\n\r\n");
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

fn req(method: &str, path: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

fn json_req(method: &str, path: &str, token: &str, json: Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json.to_string()))
        .unwrap()
}

fn form_req(
    path: &str,
    token: &str,
    fields: &[(&str, &str)],
    photo: Option<&[u8]>,
) -> Request<Body> {
    let (ct, body) = multipart(fields, photo);
    Request::builder()
        .method("POST")
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, ct)
        .body(Body::from(body))
        .unwrap()
}

fn clear_photo() -> Vec<u8> {
    b"CLEAR-CLEAR-CLEAR-CLEAR-CLEAR-CLEAR-CLEAR-CLEAR1234".to_vec()
}
fn blurry_photo() -> Vec<u8> {
    b"xxBLURxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx".to_vec()
}

async fn submit_report(
    app: &App,
    token: &str,
    spot: &str,
    plate: &str,
    photo: Vec<u8>,
) -> (StatusCode, Value) {
    let occurred = chrono::Utc::now().to_rfc3339();
    app.call(form_req(
        "/api/reports",
        token,
        &[
            ("spot_no", spot),
            ("plate", plate),
            ("occurred_at", &occurred),
        ],
        Some(&photo),
    ))
    .await
}

async fn drive_to_pending_recheck(
    app: &App,
    event_id: i64,
    inspector: &str,
    confirmed_plate: &str,
    photo: Vec<u8>,
) -> Value {
    // 巡查员举报
    let (st, rep) = submit_report(app, inspector, "A-013", "沪A11111", photo).await;
    assert_eq!(st, StatusCode::CREATED, "举报失败: {rep}");
    assert_eq!(rep["event"]["id"], event_id);
    // 值班员人工核实(车牌可以与登记/识别不同,体现人工核实的决定性)
    let (st, v) = app
        .call(json_req(
            "POST",
            &format!("/api/events/{event_id}/verify"),
            DUTY,
            serde_json::json!({ "plate": confirmed_plate, "note": "对照照片人工确认" }),
        ))
        .await;
    assert_eq!(st, StatusCode::OK, "核实失败: {v}");
    assert_eq!(v["event"]["status"], "verified");
    // 通知车主
    let (st, n) = app
        .call(json_req(
            "POST",
            &format!("/api/events/{event_id}/notify"),
            DUTY,
            serde_json::json!({}),
        ))
        .await;
    assert_eq!(st, StatusCode::OK, "通知失败: {n}");
    n
}

// ---------- 1. 重复举报归并 ----------

#[tokio::test]
async fn repeated_reports_same_spot_are_merged() {
    let (app, _d) = App::new().await;

    let (st, r1) = submit_report(&app, INSPECTOR1, "A-013", "沪A11111", clear_photo()).await;
    assert_eq!(st, StatusCode::CREATED);
    assert_eq!(r1["merged"], false, "首次举报应立新事件");
    let eid = r1["event"]["id"].as_i64().unwrap();
    assert_eq!(r1["event"]["status"], "pending_review");

    // 间隔一小段时间后再次举报同车位 → 归并
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let (st, r2) =
        submit_report(&app, INSPECTOR2, "A-013", "沪A11111", clear_photo()).await;
    assert_eq!(st, StatusCode::CREATED);
    assert_eq!(r2["merged"], true, "连续占用期间的重复举报必须归并");
    assert_eq!(r2["event"]["id"], eid, "必须归入同一事件");

    let (st, detail) = app.call(req("GET", &format!("/api/events/{eid}"), DUTY)).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(detail["event"]["report_count"], 2, "事件下应有两条举证");
    assert_eq!(detail["reports"].as_array().unwrap().len(), 2);
    // 不同车位当然是不同事件
    let (st, other) = submit_report(&app, INSPECTOR1, "B-202", "沪C22222", clear_photo()).await;
    assert_eq!(st, StatusCode::CREATED);
    assert_ne!(other["event"]["id"], eid);
}

// ---------- 2. 再次占用重新立案 ----------

#[tokio::test]
async fn reoccupancy_after_leaving_opens_new_event() {
    let (app, _d) = App::new().await;

    let n =
        drive_to_pending_recheck(&app, 1, INSPECTOR1, "沪B99999", blurry_photo()).await;
    assert_eq!(n["event"]["status"], "pending_recheck");
    assert_eq!(n["sms"]["status"], "sent");
    // 通知车牌必须是人工核实车牌,而非登记/识别车牌
    assert_eq!(n["event"]["confirmed_plate"], "沪B99999");

    // 复查:车辆已离开 → 关闭
    let (st, rc) = app
        .call(form_req(
            "/api/events/1/rechecks",
            INSPECTOR1,
            &[("vehicle_left", "true"), ("note", "现场已空")],
            None,
        ))
        .await;
    assert_eq!(st, StatusCode::CREATED, "复查提交失败: {rc}");
    assert_eq!(rc["event"]["status"], "closed");
    assert!(rc["event"]["closed_at"].is_string());

    // 车辆再次驶入,同车位新举报 → 必须新立案
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let (st, again) = submit_report(&app, INSPECTOR1, "A-013", "沪B99999", clear_photo()).await;
    assert_eq!(st, StatusCode::CREATED);
    assert_eq!(again["merged"], false, "离开后再次占用必须新建事件");
    let new_id = again["event"]["id"].as_i64().unwrap();
    assert_ne!(new_id, 1);
    assert_eq!(again["event"]["status"], "pending_review");
    assert_eq!(again["event"]["report_count"], 1, "新事件举证从 1 开始");
}

// ---------- 3. 短信成功不等于复查:未确认离开前事件保持待处理 ----------

#[tokio::test]
async fn sms_success_keeps_event_pending_recheck_until_vehicle_leaves() {
    let (app, _d) = App::new().await;

    let n =
        drive_to_pending_recheck(&app, 1, INSPECTOR1, "沪B99999", blurry_photo()).await;
    assert_eq!(n["event"]["status"], "pending_recheck");

    // 值班员再次催办(再发短信),状态仍不能变成已关闭
    let (_, n2) = app
        .call(json_req(
            "POST",
            "/api/events/1/notify",
            DUTY,
            serde_json::json!({}),
        ))
        .await;
    assert_eq!(n2["event"]["status"], "pending_recheck");

    // 复查:车辆仍占用 → 事件保持待复查、待处理
    let (st, rc) = app
        .call(form_req(
            "/api/events/1/rechecks",
            INSPECTOR1,
            &[("vehicle_left", "false"), ("note", "车还在")],
            None,
        ))
        .await;
    assert_eq!(st, StatusCode::CREATED);
    assert_eq!(
        rc["event"]["status"],
        "pending_recheck",
        "车辆未离开时必须仍显示待处理"
    );
    assert!(rc["event"]["closed_at"].is_null());

    // 巡查员列表中待复查事件必须排在最前
    let (_, list) = app.call(req("GET", "/api/events", INSPECTOR1)).await;
    let arr = list["events"].as_array().unwrap();
    assert_eq!(arr[0]["status"], "pending_recheck");
}

// ---------- 4. 岗位权限:举证/核实/复查/关闭不可越权 ----------

#[tokio::test]
async fn role_endpoints_are_enforced() {
    let (app, _d) = App::new().await;

    // 值班员不能提交举报
    let (st, _) = app
        .call(form_req(
            "/api/reports",
            DUTY,
            &[("spot_no", "A-013"), ("plate", "沪A11111")],
            Some(&clear_photo()),
        ))
        .await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    // 先立一个事件
    let (_, r) = submit_report(&app, INSPECTOR1, "A-013", "沪A11111", clear_photo()).await;
    let eid = r["event"]["id"].as_i64().unwrap();

    // 巡查员不能核实、不能通知
    let (st, _) = app
        .call(json_req(
            "POST",
            &format!("/api/events/{eid}/verify"),
            INSPECTOR1,
            serde_json::json!({"plate":"沪A11111"}),
        ))
        .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = app
        .call(json_req(
            "POST",
            &format!("/api/events/{eid}/notify"),
            INSPECTOR1,
            serde_json::json!({}),
        ))
        .await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    // 不存在通用的"修改事件状态/直接结案"端点
    let (st, _) = app
        .call(json_req(
            "PATCH",
            &format!("/api/events/{eid}"),
            DUTY,
            serde_json::json!({"status":"closed"}),
        ))
        .await;
    assert_eq!(st, StatusCode::METHOD_NOT_ALLOWED, "不能凭修改请求越权结案");

    // 值班员核实+通知后,值班员自己也不能复查结案
    let (_, _) = app
        .call(json_req(
            "POST",
            &format!("/api/events/{eid}/verify"),
            DUTY,
            serde_json::json!({"plate":"沪A11111"}),
        ))
        .await;
    let (_, _) = app
        .call(json_req(
            "POST",
            &format!("/api/events/{eid}/notify"),
            DUTY,
            serde_json::json!({}),
        ))
        .await;
    let (st, _) = app
        .call(form_req(
            &format!("/api/events/{eid}/rechecks"),
            DUTY,
            &[("vehicle_left", "true")],
            None,
        ))
        .await;
    assert_eq!(st, StatusCode::FORBIDDEN, "值班员不能自行复查结案");

    // 未复查时任何途径都无法关闭:状态仍是 pending_recheck
    let (_, detail) = app.call(req("GET", &format!("/api/events/{eid}"), DUTY)).await;
    assert_eq!(detail["event"]["status"], "pending_recheck");
}

// ---------- 5. 事件资料的权限控制 ----------

#[tokio::test]
async fn evidence_materials_are_access_controlled() {
    let (app, _d) = App::new().await;

    // inspector1 在 C-301 举报(模糊照片 → 转人工)
    let (_, r) = submit_report(&app, INSPECTOR1, "C-301", "沪D55555", blurry_photo()).await;
    let eid = r["event"]["id"].as_i64().unwrap();
    let (_, detail1) = app
        .call(req("GET", &format!("/api/events/{eid}"), INSPECTOR1))
        .await;
    let photo_name = detail1["reports"][0]["photo_path"].as_str().unwrap().to_string();
    assert_eq!(detail1["reports"][0]["needs_manual"], true);

    // 未参与的 inspector2 在事件待核实时不能查看详情与照片
    let (st, _) = app
        .call(req("GET", &format!("/api/events/{eid}"), INSPECTOR2))
        .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = app
        .call(req("GET", &format!("/api/photos/{photo_name}"), INSPECTOR2))
        .await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    // 无令牌不能访问照片
    let (st, _) = app
        .call(req("GET", &format!("/api/photos/{photo_name}"), "wrong-token"))
        .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    // 路径穿越被拒
    let (st, _) = app
        .call(req("GET", "/api/photos/..%2F..%2Fevidence.db", DUTY))
        .await;
    assert!(st == StatusCode::BAD_REQUEST || st == StatusCode::NOT_FOUND);

    // 值班员核实并通知后,事件变为待复查 → 任何巡查员因职责需要都可见、可复查
    let (_, _) = app
        .call(json_req(
            "POST",
            &format!("/api/events/{eid}/verify"),
            DUTY,
            serde_json::json!({"plate":"沪D55555"}),
        ))
        .await;
    let (_, _) = app
        .call(json_req(
            "POST",
            &format!("/api/events/{eid}/notify"),
            DUTY,
            serde_json::json!({}),
        ))
        .await;
    let (st, _) = app
        .call(req("GET", &format!("/api/events/{eid}"), INSPECTOR2))
        .await;
    assert_eq!(st, StatusCode::OK, "待复查事件对巡查员应可见");
    let (st, bytes) = app
        .bytes(req("GET", &format!("/api/photos/{photo_name}"), INSPECTOR2))
        .await;
    assert_eq!(st, StatusCode::OK, "待复查事件的举证照片应对巡查员可见");
    assert_eq!(&bytes[..4], b"xxBL", "应返回原始照片字节");
    let (st, rc) = app
        .call(form_req(
            &format!("/api/events/{eid}/rechecks"),
            INSPECTOR2,
            &[("vehicle_left", "true")],
            None,
        ))
        .await;
    assert_eq!(st, StatusCode::CREATED, "巡查员可对可见的待复查事件复查: {rc}");
    assert_eq!(rc["event"]["status"], "closed");
}

// ---------- 6. ANPR 仅辅助:未人工核实不得通知;核实前状态机拦截 ----------

#[tokio::test]
async fn anpr_is_advisory_and_notify_requires_manual_verification() {
    let (app, _d) = App::new().await;

    // 模糊照片:识别低置信 → needs_manual
    let (st, r) = submit_report(&app, INSPECTOR1, "A-013", "沪A11111", blurry_photo()).await;
    assert_eq!(st, StatusCode::CREATED);
    assert_eq!(r["recognition"]["needs_manual"], true);
    assert!(
        r["recognition"]["confidence"].as_f64().unwrap() < 0.85,
        "模糊照片置信度必须低于人工阈值"
    );

    // 清晰照片也不允许跳过人工核实
    let (st, _) = submit_report(&app, INSPECTOR1, "D-009", "沪E77777", clear_photo()).await;
    assert_eq!(st, StatusCode::CREATED);
    let (st, body) = app
        .call(json_req(
            "POST",
            "/api/events/2/notify",
            DUTY,
            serde_json::json!({}),
        ))
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "未经人工核实不能通知车主");
    assert!(body["error"].as_str().unwrap().contains("核实"));

    // 核实必须带人工确认车牌
    let (st, body) = app
        .call(json_req(
            "POST",
            "/api/events/1/verify",
            DUTY,
            serde_json::json!({ "plate": "" }),
        ))
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("车牌"));

    // 复查只能在通知之后:刚举报的事件直接复查被拒
    let (st, body) = app
        .call(form_req(
            "/api/events/2/rechecks",
            INSPECTOR1,
            &[("vehicle_left", "true")],
            None,
        ))
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("待复查"));
}
