//! 端到端集成测试：启动真实 HTTP 服务 + SQLite，验证核心业务规则。
use reqwest::{Client, StatusCode};
use serde_json::{json, Value};

const INSPECTOR: (&str, &str) = ("xuncha01", "xuncha123");
const INSPECTOR2: (&str, &str) = ("xuncha02", "xuncha123");
const OFFICER: (&str, &str) = ("zhiban01", "zhiban123");

/// 清晰照片（>2KB，识别置信度高）
fn clear_photo() -> Vec<u8> {
    vec![0xAB; 4096]
}
/// 模糊照片（<2KB 且文件名含 blur，触发转人工）
fn blurry_photo() -> Vec<u8> {
    vec![0x00; 128]
}

async fn spawn_app() -> String {
    let dir = std::env::temp_dir().join(format!("pe-test-{}", uuid::Uuid::new_v4()));
    let db = dir.join("test.db");
    let uploads = dir.join("uploads");
    std::fs::create_dir_all(&uploads).unwrap();
    let app = parking_evidence::build_app(db.to_str().unwrap(), uploads)
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://127.0.0.1:{port}")
}

async fn login(client: &Client, base: &str, account: (&str, &str)) -> String {
    let res = client
        .post(format!("{base}/api/login"))
        .json(&json!({ "username": account.0, "password": account.1 }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    res.json::<Value>().await.unwrap()["token"]
        .as_str()
        .unwrap()
        .to_string()
}

async fn report(
    client: &Client,
    base: &str,
    token: &str,
    spot: &str,
    plate: &str,
    photo_name: &str,
    photo: Vec<u8>,
) -> (StatusCode, Value) {
    let form = reqwest::multipart::Form::new()
        .text("spot_number", spot.to_string())
        .text("plate", plate.to_string())
        .text("occurred_at", "2026-10-05T09:30")
        .part(
            "photo",
            reqwest::multipart::Part::bytes(photo)
                .file_name(photo_name.to_string())
                .mime_str("image/jpeg")
                .unwrap(),
        );
    let res = client
        .post(format!("{base}/api/reports"))
        .bearer_auth(token)
        .multipart(form)
        .send()
        .await
        .unwrap();
    let status = res.status();
    let body = res.json::<Value>().await.unwrap_or_else(|_| json!({}));
    (status, body)
}

async fn verify(client: &Client, base: &str, token: &str, id: i64, plate: &str) -> StatusCode {
    client
        .post(format!("{base}/api/events/{id}/verify"))
        .bearer_auth(token)
        .json(&json!({ "confirmed_plate": plate }))
        .send()
        .await
        .unwrap()
        .status()
}

async fn notify(client: &Client, base: &str, token: &str, id: i64) -> (StatusCode, Value) {
    let res = client
        .post(format!("{base}/api/events/{id}/notify"))
        .bearer_auth(token)
        .send()
        .await
        .unwrap();
    let status = res.status();
    (status, res.json::<Value>().await.unwrap_or_else(|_| json!({})))
}

async fn recheck(client: &Client, base: &str, token: &str, id: i64, result: &str) -> StatusCode {
    client
        .post(format!("{base}/api/events/{id}/rechecks"))
        .bearer_auth(token)
        .json(&json!({ "result": result }))
        .send()
        .await
        .unwrap()
        .status()
}

async fn close(client: &Client, base: &str, token: &str, id: i64, body: Value) -> StatusCode {
    client
        .post(format!("{base}/api/events/{id}/close"))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .unwrap()
        .status()
}

async fn get_event(client: &Client, base: &str, token: &str, id: i64) -> (StatusCode, Value) {
    let res = client
        .get(format!("{base}/api/events/{id}"))
        .bearer_auth(token)
        .send()
        .await
        .unwrap();
    let status = res.status();
    (status, res.json::<Value>().await.unwrap_or_else(|_| json!({})))
}

async fn list_events(client: &Client, base: &str, token: &str) -> Vec<Value> {
    client
        .get(format!("{base}/api/events"))
        .bearer_auth(token)
        .send()
        .await
        .unwrap()
        .json::<Vec<Value>>()
        .await
        .unwrap()
}

/// 规则一：同一车位连续占用期间的重复举报归并到当前事件
#[tokio::test]
async fn duplicate_reports_merge_into_current_event() {
    let base = spawn_app().await;
    let client = Client::new();
    let inspector = login(&client, &base, INSPECTOR).await;
    let officer = login(&client, &base, OFFICER).await;

    let (st, first) = report(&client, &base, &inspector, "A-101", "京AD12345", "a.jpg", clear_photo()).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(first["merged"], false);
    let event_id = first["event_id"].as_i64().unwrap();

    // 同一车位再次举报（甚至换一位巡查员）→ 归并，不新建事件
    let inspector2 = login(&client, &base, INSPECTOR2).await;
    let (st, second) = report(&client, &base, &inspector2, "A-101", "京AD12345", "b.jpg", clear_photo()).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(second["merged"], true);
    assert_eq!(second["event_id"].as_i64().unwrap(), event_id);

    // 事件只有一条，举报记录有两条
    let events = list_events(&client, &base, &officer).await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["report_count"], 2);

    let (st, detail) = get_event(&client, &base, &officer, event_id).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(detail["reports"].as_array().unwrap().len(), 2);
}

/// 规则二：岗位权限 —— 各岗位只能做职责内操作，篡改请求不能越权结案
#[tokio::test]
async fn role_permissions_are_enforced() {
    let base = spawn_app().await;
    let client = Client::new();
    let inspector = login(&client, &base, INSPECTOR).await;
    let inspector2 = login(&client, &base, INSPECTOR2).await;
    let officer = login(&client, &base, OFFICER).await;

    // 值班员不能举证
    let (st, _) = report(&client, &base, &officer, "B-202", "京AD99999", "a.jpg", clear_photo()).await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    // 巡查员举证成功
    let (st, body) = report(&client, &base, &inspector, "B-202", "京AD99999", "a.jpg", clear_photo()).await;
    assert_eq!(st, StatusCode::OK);
    let event_id = body["event_id"].as_i64().unwrap();

    // 巡查员不能核实/通知/结案
    assert_eq!(verify(&client, &base, &inspector, event_id, "京AD99999").await, StatusCode::FORBIDDEN);
    assert_eq!(notify(&client, &base, &inspector, event_id).await.0, StatusCode::FORBIDDEN);
    assert_eq!(close(&client, &base, &inspector, event_id, json!({})).await, StatusCode::FORBIDDEN);

    // 篡改请求体（伪造角色/状态）不能越权结案 —— 服务端只认会话角色
    assert_eq!(
        close(&client, &base, &inspector, event_id, json!({ "role": "officer", "status": "closed", "force": true })).await,
        StatusCode::FORBIDDEN
    );
    // 事件仍是待核实
    let (_, detail) = get_event(&client, &base, &officer, event_id).await;
    assert_eq!(detail["status"], "pending_verify");

    // 未登录一律 401
    let res = client.get(format!("{base}/api/events")).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    // 事件资料权限：其他巡查员看不到非自己举报且非待复查的事件
    let (st, _) = get_event(&client, &base, &inspector2, event_id).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    // 报案人本人与值班员可见
    assert_eq!(get_event(&client, &base, &inspector, event_id).await.0, StatusCode::OK);
    assert_eq!(get_event(&client, &base, &officer, event_id).await.0, StatusCode::OK);
}

/// 规则三：车辆离开后再次驶入 → 新建事件（不归并到已结案事件）
#[tokio::test]
async fn reoccupancy_after_close_creates_new_event() {
    let base = spawn_app().await;
    let client = Client::new();
    let inspector = login(&client, &base, INSPECTOR).await;
    let officer = login(&client, &base, OFFICER).await;

    // 完整走完一次占用 → 结案
    let (_, first) = report(&client, &base, &inspector, "C-303", "京AD11111", "a.jpg", clear_photo()).await;
    let first_id = first["event_id"].as_i64().unwrap();
    assert_eq!(verify(&client, &base, &officer, first_id, "京AD11111").await, StatusCode::OK);
    assert_eq!(notify(&client, &base, &officer, first_id).await.0, StatusCode::OK);
    assert_eq!(recheck(&client, &base, &inspector, first_id, "left").await, StatusCode::OK);
    assert_eq!(close(&client, &base, &officer, first_id, json!({})).await, StatusCode::OK);

    // 同一车位再次占用 → 新建事件
    let (_, second) = report(&client, &base, &inspector, "C-303", "京AD22222", "b.jpg", clear_photo()).await;
    assert_eq!(second["merged"], false);
    let second_id = second["event_id"].as_i64().unwrap();
    assert_ne!(first_id, second_id);

    let events = list_events(&client, &base, &officer).await;
    assert_eq!(events.len(), 2);
    let old = events.iter().find(|e| e["id"] == first_id).unwrap();
    let new = events.iter().find(|e| e["id"] == second_id).unwrap();
    assert_eq!(old["status"], "closed");
    assert_eq!(new["status"], "pending_verify");
}

/// 规则四：通知发出但车辆未离开时，事件仍是待处理；
/// 短信发送成功不能代替复查结案；再次通知后须重新复查。
#[tokio::test]
async fn notified_but_not_left_stays_pending() {
    let base = spawn_app().await;
    let client = Client::new();
    let inspector = login(&client, &base, INSPECTOR).await;
    let officer = login(&client, &base, OFFICER).await;

    let (_, body) = report(&client, &base, &inspector, "D-404", "京AD55555", "a.jpg", clear_photo()).await;
    let id = body["event_id"].as_i64().unwrap();
    assert_eq!(verify(&client, &base, &officer, id, "京AD55555").await, StatusCode::OK);

    // 短信发送成功
    let (st, notify_body) = notify(&client, &base, &officer, id).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(notify_body["sms_status"], "sent");

    // 事件仍显示待处理（待复查），并未结案
    let events = list_events(&client, &base, &officer).await;
    let ev = events.iter().find(|e| e["id"] == id).unwrap();
    assert_eq!(ev["status"], "notified");
    assert_eq!(ev["pending_recheck"], true);
    assert_eq!(ev["can_close"], false);

    // 未复查直接结案 → 拒绝
    assert_eq!(close(&client, &base, &officer, id, json!({})).await, StatusCode::CONFLICT);

    // 复查：车辆仍占用 → 仍不能结案
    assert_eq!(recheck(&client, &base, &inspector, id, "still_occupied").await, StatusCode::OK);
    assert_eq!(close(&client, &base, &officer, id, json!({})).await, StatusCode::CONFLICT);

    // 复查确认离开 → 可以结案；但若再次通知（车主又被催告），旧复查失效
    assert_eq!(recheck(&client, &base, &inspector, id, "left").await, StatusCode::OK);
    assert_eq!(notify(&client, &base, &officer, id).await.0, StatusCode::OK); // 再次通知
    assert_eq!(close(&client, &base, &officer, id, json!({})).await, StatusCode::CONFLICT); // 须重新复查

    // 重新复查确认离开 → 结案成功
    assert_eq!(recheck(&client, &base, &inspector, id, "left").await, StatusCode::OK);
    assert_eq!(close(&client, &base, &officer, id, json!({ "note": "确认驶离" })).await, StatusCode::OK);

    let (_, detail) = get_event(&client, &base, &officer, id).await;
    assert_eq!(detail["status"], "closed");
}

/// 规则五：照片看不清 → 转人工核实；核实车牌必须人工确认，
/// 通知只发给人工确认车牌对应的车主，不能直接用识别结果。
#[tokio::test]
async fn unclear_photo_requires_manual_plate_confirmation() {
    let base = spawn_app().await;
    let client = Client::new();
    let inspector = login(&client, &base, INSPECTOR).await;
    let officer = login(&client, &base, OFFICER).await;

    let (st, body) = report(&client, &base, &inspector, "E-505", "京AD77777", "blur.jpg", blurry_photo()).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(body["recognition"]["needs_manual_review"], true);
    assert!(body["recognition"]["plate"].is_null());
    let id = body["event_id"].as_i64().unwrap();

    // 不提供人工确认车牌 → 核实被拒绝
    let res = client
        .post(format!("{base}/api/events/{id}/verify"))
        .bearer_auth(&officer)
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    // 未核实不能通知
    assert_eq!(notify(&client, &base, &officer, id).await.0, StatusCode::CONFLICT);

    // 值班员人工确认车牌（与报案不同，以人工核实为准）→ 核实通过
    assert_eq!(verify(&client, &base, &officer, id, "京AD77888").await, StatusCode::OK);

    // 通知发给人工确认的车牌
    let (st, notify_body) = notify(&client, &base, &officer, id).await;
    assert_eq!(st, StatusCode::OK);
    let (_, detail) = get_event(&client, &base, &officer, id).await;
    let notifications = detail["notifications"].as_array().unwrap();
    assert_eq!(notifications.len(), 1);
    assert_eq!(notifications[0]["plate"], "京AD77888");
    assert_eq!(detail["plate_confirmed"], "京AD77888");
    assert!(notify_body["phone"].as_str().unwrap().starts_with('1'));
}
