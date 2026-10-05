-- 新能源车位占用举证系统 初始 schema
-- 事件状态机:
--   pending_review   巡查员已举报,等待值班员核实
--   verified         值班员已核实(车牌已人工确认),可通知车主
--   pending_recheck  短信已发出,等待巡查员现场复查
--   closed           复查确认车辆离开,事件关闭
--
-- 归并规则:同一车位存在未关闭事件,且该事件最后一次"确认车辆离开"之后
-- 的新举报,归入当前事件(增加一条 reports 记录);复查确认离开后的举报
-- 不再命中归并,从而新建事件(再次占用重新立案)。

CREATE TABLE users (
    id            INTEGER PRIMARY KEY,
    username      TEXT NOT NULL UNIQUE,
    display_name  TEXT NOT NULL,
    role          TEXT NOT NULL CHECK (role IN ('inspector', 'duty')),
    token         TEXT NOT NULL UNIQUE           -- 演示用固定登录令牌
);

CREATE TABLE events (
    id                  INTEGER PRIMARY KEY,
    spot_no             TEXT NOT NULL,           -- 车位号,如 A-013
    -- 值班员人工核实确认的车牌(举报时填写的车牌为待核实信息)
    confirmed_plate     TEXT,
    status              TEXT NOT NULL DEFAULT 'pending_review'
                        CHECK (status IN ('pending_review','verified','pending_recheck','closed')),
    first_reported_at   TEXT NOT NULL,           -- ISO8601
    last_report_at      TEXT NOT NULL,
    closed_at           TEXT,
    -- 最近一次经复查确认车辆离开的时间;归并判定的分界线
    last_left_at        TEXT,
    created_by          INTEGER NOT NULL REFERENCES users(id)
);

-- 归并查询的关键索引:找"该车位未关闭且未确认离开"的进行中事件
CREATE INDEX idx_events_open ON events(spot_no, status) WHERE status != 'closed';

CREATE TABLE reports (
    id              INTEGER PRIMARY KEY,
    event_id        INTEGER NOT NULL REFERENCES events(id) ON DELETE CASCADE,
    inspector_id    INTEGER NOT NULL REFERENCES users(id),
    plate_entered   TEXT NOT NULL,               -- 巡查员登记的车牌
    occurred_at     TEXT NOT NULL,               -- 发生时间
    photo_path      TEXT NOT NULL,               -- uploads 下的相对文件名
    created_at      TEXT NOT NULL
);
CREATE INDEX idx_reports_event ON reports(event_id);

-- 车牌识别(ANPR)仅作辅助:每次识别独立留痕,结果不直接用于发通知
CREATE TABLE recognitions (
    id              INTEGER PRIMARY KEY,
    report_id       INTEGER NOT NULL REFERENCES reports(id) ON DELETE CASCADE,
    suggested_plate TEXT,                        -- 识别建议(可为空)
    confidence      REAL NOT NULL,               -- 0~1
    needs_manual    INTEGER NOT NULL,            -- 1 = 照片不清,转人工核实
    created_at      TEXT NOT NULL
);

-- 值班员核实记录:必须由值班员人工确认车牌
CREATE TABLE verifications (
    id              INTEGER PRIMARY KEY,
    event_id        INTEGER NOT NULL UNIQUE REFERENCES events(id),
    duty_id         INTEGER NOT NULL REFERENCES users(id),
    confirmed_plate TEXT NOT NULL,
    note            TEXT,
    created_at      TEXT NOT NULL
);

-- 短信通知(模拟):发送成功不代表车辆离开
CREATE TABLE notifications (
    id              INTEGER PRIMARY KEY,
    event_id        INTEGER NOT NULL REFERENCES events(id),
    duty_id         INTEGER NOT NULL REFERENCES users(id),
    plate           TEXT NOT NULL,
    message         TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'sent' CHECK (status IN ('sent','failed')),
    created_at      TEXT NOT NULL
);
CREATE INDEX idx_notifications_event ON notifications(event_id);

-- 复查记录:巡查员现场确认车辆是否仍在
CREATE TABLE rechecks (
    id              INTEGER PRIMARY KEY,
    event_id        INTEGER NOT NULL REFERENCES events(id),
    inspector_id    INTEGER NOT NULL REFERENCES users(id),
    vehicle_left    INTEGER NOT NULL,            -- 1=已离开 0=仍占用
    photo_path      TEXT,                        -- 复查照片(可选)
    note            TEXT,
    created_at      TEXT NOT NULL
);
CREATE INDEX idx_rechecks_event ON rechecks(event_id);

-- 种子账号
INSERT INTO users (id, username, display_name, role, token) VALUES
 (1, 'inspector1', '巡查员甲', 'inspector', 'tok-inspector-1'),
 (2, 'inspector2', '巡查员乙', 'inspector', 'tok-inspector-2'),
 (3, 'duty1',      '值班员甲', 'duty',      'tok-duty-1');
