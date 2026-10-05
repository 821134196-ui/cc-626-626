//! 模拟外部服务：车牌识别、车主通讯录、短信网关。
//! 生产环境替换为真实 HTTP 调用即可，接口签名保持不变。

use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Serialize)]
pub struct Recognition {
    pub plate: Option<String>,
    pub confidence: f32,
    pub needs_manual_review: bool,
}

/// 模拟车牌识别（仅作辅助）。
/// 照片过小或文件名含 "blur" 视为"看不清"：置信度低、不给出识别车牌，
/// 必须转人工核实，禁止直接按识别结果通知车主。
pub fn recognize_plate(photo: &[u8], filename: &str, reported_plate: &str) -> Recognition {
    let unclear = photo.len() < 2048 || filename.to_ascii_lowercase().contains("blur");
    if unclear {
        Recognition {
            plate: None,
            confidence: 0.31,
            needs_manual_review: true,
        }
    } else {
        Recognition {
            plate: Some(reported_plate.to_string()),
            confidence: 0.96,
            needs_manual_review: false,
        }
    }
}

/// 模拟车主通讯录：由车牌确定性生成手机号。
pub fn lookup_phone(plate: &str) -> String {
    let digest = Sha256::digest(plate.as_bytes());
    let prefixes = ['3', '5', '7', '8', '9'];
    let mut phone = String::from("1");
    phone.push(prefixes[(digest[0] % 5) as usize]);
    for b in &digest[1..10] {
        phone.push((b'0' + b % 10) as char);
    }
    phone
}

/// 模拟短信网关：记录日志并返回成功。
/// 注意：发送成功仅表示短信已提交网关，不代表车主已驶离，结案仍须复查。
pub fn send_sms(phone: &str, content: &str) -> Result<String, String> {
    tracing::info!(target: "sms-gateway", "模拟发送短信 -> {phone}: {content}");
    Ok("sent".to_string())
}
