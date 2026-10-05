/// 模拟外部接口:车牌识别(ANPR)与短信网关。
/// 真实系统中这里会调用第三方;演示环境以确定性规则模拟,便于测试。

/// ANPR 识别结果。confidence < 0.85(或照片"模糊")时 needs_manual=true,
/// 必须转值班员人工核实,不能按识别建议直接通知车主。
#[derive(Debug, Clone)]
pub struct RecognitionResult {
    pub suggested_plate: Option<String>,
    pub confidence: f64,
    pub needs_manual: bool,
}

/// 依据上传照片字节做"识别"。演示约定:
/// - 极小文件(<32 字节)视为照片无法读取 -> 低置信、无建议;
/// - 文件内容含标记 `BLUR` 视为模糊照片 -> 低置信;
/// - 否则认为清晰;建议车牌由调用方传入的登记车牌(模拟识别通常与之一致),
///   但置信度做稳定扰动:以字节长度派生,保证可重复且不使用随机数。
pub fn recognize_plate(photo_bytes: &[u8], entered_plate: &str) -> RecognitionResult {
    if photo_bytes.len() < 32 {
        return RecognitionResult {
            suggested_plate: None,
            confidence: 0.21,
            needs_manual: true,
        };
    }
    let blurry = photo_bytes
        .windows(4)
        .any(|w| w.eq_ignore_ascii_case(b"BLUR"));
    if blurry {
        return RecognitionResult {
            suggested_plate: Some(entered_plate.to_uppercase()),
            confidence: 0.58,
            needs_manual: true,
        };
    }
    // 清晰照片:0.90~0.99 之间的确定性置信度
    let confidence = 0.90 + ((photo_bytes.len() % 10) as f64 / 100.0);
    RecognitionResult {
        suggested_plate: Some(entered_plate.trim().to_uppercase()),
        confidence,
        needs_manual: false,
    }
}

/// 模拟短信发送。统一返回成功;网关不提供"车辆是否离开"的任何信息,
/// 因此发送成功绝不能代替现场复查。
#[derive(Debug)]
pub struct SmsReceipt {
    pub provider_msg_id: String,
    pub status: &'static str,
}

pub fn send_sms(plate: &str, spot: &str, message: &str) -> SmsReceipt {
    // 以内容生成稳定回执号(不使用随机数/时间,保证测试可重复)
    let digest: u64 = message
        .bytes()
        .chain(plate.bytes())
        .chain(spot.bytes())
        .fold(1469598103934665603u64, |h, b| {
            (h ^ b as u64).wrapping_mul(1099511628211)
        });
    SmsReceipt {
        provider_msg_id: format!("SIM-{:016X}", digest),
        status: "sent",
    }
}
