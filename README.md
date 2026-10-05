# 新能源车位占用举证系统

公共停车场新能源专用车位被占用时的**举证、核实、复查、结案**系统。
技术栈:**Rust + Axum + SQLx + SQLite**(后端)、**Svelte + Vite**(移动端优先前端),
照片保存在本地,车牌识别(ANPR)与短信网关均为**模拟接口**。

## 一条命令启动

```bash
./run.sh
```

脚本会自动安装/构建前端并编译启动后端,浏览器打开 <http://localhost:8080>。
(需要本机已安装 Rust 与 Node.js;可用 `PORT=9000 ./run.sh` 改端口,`DATA_DIR` 改数据目录。)

演示账号(登录无需密码):

| 账号 | 岗位 | 职责 |
| --- | --- | --- |
| `inspector1` / `inspector2` | 巡查员 | 登记举报、上传现场照片、现场复查 |
| `duty1` | 值班员 | 人工核实车牌、通知车主(不可直接结案) |

## 业务流程与状态机

```
巡查员举证 ──► 待核实 pending_review
                    │ 值班员对照照片【人工核实】车牌
                    ▼
              已核实待通知 verified
                    │ 值班员发送挪车短信(模拟)
                    ▼
              待复查 pending_recheck  ◄───── 车辆仍占用:复查保持此状态,可再次催办
                    │ 巡查员现场复查
                    ├─ 车辆仍在 → 保持待复查(继续待处理)
                    └─ 确认离开 → 已关闭 closed
```

关键规则:

1. **重复举报归并**:同一车位存在未关闭事件时,新举报归入当前事件(事件内增加一条举证),
   不重复立案。
2. **再次占用重新立案**:巡查员复查确认车辆离开会记录 `last_left_at`;此后同车位再被举报
   自动新建事件。
3. **短信成功 ≠ 车辆离开**:短信只把事件推进到「待复查」;未由巡查员现场确认离开前,
   事件在手机页面始终显示为待处理,且排在列表最前。
4. **ANPR 仅辅助**:识别结果与置信度只作预填参考;照片模糊(低置信)时标记「转人工核实」;
   系统**不存在**按识别结果直接发通知的路径,通知车牌只能取值班员人工确认的车牌。
5. **岗位权限**:
   - 巡查员:`POST /api/reports`、`POST /api/events/:id/rechecks`;
   - 值班员:`POST /api/events/:id/verify`、`POST /api/events/:id/notify`;
   - **没有**通用的事件状态修改/删除接口,值班员无法靠修改请求越权结案;
   - 事件资料(详情、照片)按可见性授权:值班员可见全部;巡查员仅可见本人参与或
     「待复查」的事件(职责需要到场)。照片接口校验归属后才返回文件,并阻止路径穿越。
6. **照片本地存储**:保存在 `data/uploads/`,文件名随机,经鉴权接口 `/api/photos/:name`
   读取(前端用带令牌的 Blob 请求渲染,不直接暴露 URL)。

## 页面说明

- **巡查员手机页**:顶部固定「待复查车位」区块;可展开举报表单(支持调起手机相机);
  事件详情内现场选择「车辆仍占用 / 确认车辆已离开」,可补拍复查照片。
- **值班员工作台**:列表待复查优先;详情页分三步(人工核实 → 发短信 → 等待复查),
  第三步明确提示值班员无权直接结案。

## 验证(自动化测试)

```bash
cargo test
```

6 个端到端集成测试(真实 HTTP 路由 + 临时 SQLite/照片目录):

| 测试 | 验证内容 |
| --- | --- |
| `repeated_reports_same_spot_are_merged` | 同车位重复举报 `merged=true`、归入同一事件、举证计数 +1 |
| `reoccupancy_after_leaving_opens_new_event` | 复查离开并关闭后,同车位新举报 `merged=false`、新事件 |
| `sms_success_keeps_event_pending_recheck_until_vehicle_leaves` | 短信发送/催办成功后仍 `pending_recheck`;复查"仍占用"也保持待处理 |
| `role_endpoints_are_enforced` | 跨岗位操作均 403;`PATCH /api/events/:id` 返回 405,不能越权结案;值班员不能自行复查 |
| `evidence_materials_are_access_controlled` | 未参与巡查员详情/照片 403;待复查后放行;无令牌 404;路径穿越被拒 |
| `anpr_is_advisory_and_notify_requires_manual_verification` | 模糊照片 `needs_manual`;未人工核实通知被 400 拒绝;未通知不能复查 |

## API 摘要

| 方法 路径 | 岗位 | 说明 |
| --- | --- | --- |
| `POST /api/login` | - | 账号换令牌 |
| `GET /api/me` | 已登录 | 当前身份 |
| `GET /api/events` | 已登录 | 事件列表(待复查优先,按岗位过滤) |
| `POST /api/reports` | 巡查员 | multipart:车位号、车牌、发生时间、照片;返回是否归并 + ANPR 建议 |
| `GET /api/events/:id` | 授权可见 | 事件详情(举证、识别、核实、通知、复查) |
| `POST /api/events/:id/verify` | 值班员 | 人工确认车牌 → verified |
| `POST /api/events/:id/notify` | 值班员 | 模拟短信 → pending_recheck(可重复催办) |
| `POST /api/events/:id/rechecks` | 巡查员 | multipart:vehicle_left、备注、可选照片;仅 left=true 关闭事件 |
| `GET /api/photos/:filename` | 授权可见 | 读取本地照片(鉴权 + 防穿越) |

## 数据与重置

删除 `./data` 即可清空全部事件与照片;下次启动自动重建 SQLite 与种子账号。
