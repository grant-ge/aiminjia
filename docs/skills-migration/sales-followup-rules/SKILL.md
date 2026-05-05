---
name: sales-followup-rules
description: >
  按规则判定每日应跟进的客户：通过钉钉 dws CLI 读取 AI 表格中的在谈客户，按"上次联系/阶段停滞/next_action 到期"判断今日跟进列表，可选拉取群聊与外部信号补充上下文，最后生成 Markdown 跟进提醒。所有写表操作必须经用户在对话中明示确认。
when_to_use: >
  数字员工"小销"或类似客户跟进角色被派活时；resource_config 必须包含 tableId 和 fieldMapping。
allowed-tools:
  - bash
  - web_search
  - memory_save
  - memory_search
  - load_skill
  - generate_report
model: sonnet
effort: high
context: inline
user-invocable: true
disable-model-invocation: false
version: "1.0"
metadata:
  label: 客户跟进规则判定
---

# 客户跟进规则判定

## 硬性约束（开工前先复读一遍）

- **写表必须用户确认**：通过 `dws` CLI 修改 AI 表格记录之前必须在对话中清楚说明"将要把字段 X 从 A 改为 B"，等待用户回 "确认 / 同意 / 是" 之后才执行。
- 不主动给客户发消息或代回邮件——你只做"提醒该跟谁、该说什么"。
- 不预测成交概率、不打分。规则判定输出"是否进入今日跟进列表"，不输出"赢率"。

## 工具约定

- **所有钉钉能力（AI 表格 / 群聊 / 通讯录 / 待办 / 日历）必须通过 `dws` CLI 完成**：先 `load_skill('dingtalk-workspace')` 获取 dws 命令清单、登录方式、参数约定，再用 `bash` 调用具体命令。
- 不要绕过 dws 直接调 HTTP / 浏览器 / curl 钉钉接口。
- 不要假设 baseId / tableId / recordId 等标识符——必须先用 `dws` 查询拿到真实 ID 再使用。

## 输入约定

派活时通过 prompt 的"资源配置"段传入：

```json
{
  "tableId": "<钉钉 base id>",
  "fieldMapping": {
    "customerName": "<列名>",
    "stage": "<列名>",
    "lastContact": "<列名>",
    "nextAction": "<列名>",
    "nextActionDate": "<列名>",
    "owner": "<列名>",
    "notes": "<列名>"
  },
  "scope": "self"
}
```

如缺失：礼貌提示用户在 EmployeeDrawer ⚙️ 完成配置后再派活。

## 工作流程

### 0. 加载钉钉技能

第一步：`load_skill('dingtalk-workspace')` —— 学习 dws 的环境检查、登录验证、命令发现规则。

随后通过 dws 验证已登录，并定位真实 base：

```bash
dws auth status --format json
dws table list --format json
```

### 1. 拉取客户列表

通过 dws 读取 AI 表格中的在谈客户（实际命令以 `dws table --help` 或 dingtalk-workspace SKILL 中的命令清单为准）：

```bash
dws table records list --base-id <baseId> --table-id <tableId> --format json
```

按 scope 过滤：
- `self`：仅当前用户为 owner（owner 字段名取自 fieldMapping.owner）
- `department`：当前用户所在部门所有 owner

### 2. 规则判定

对每条记录按 fieldMapping 提取核心字段，判断是否进入"今日跟进":

- **R1: 上次联系超过 7 天** —— `now - lastContact >= 7d`
- **R2: 阶段停滞** —— `stage` 字段连续 ≥ 14 天未变化（用 `memory_search` 拉上次记录的 stage 比对）
- **R3: next_action 到期或过期** —— `nextActionDate <= today`

命中任一规则即入选。每条入选记录附"触发原因"。

### 3. 上下文补充（可选，最多花 5 步）

对入选客户：

1. `memory_search` 拉最近 3 次跟进笔记（namespace `sales:<customerName>`）
2. 可选用 dws 在客户群里搜最近 7 天关键词（金额、合同、技术问题等）—— 命令以 dingtalk-workspace SKILL 中的 chat search 段为准
3. 可选 `web_search`：搜索"<客户公司> 最近动态"，找 0-1 条公开信号

### 4. 生成今日跟进提醒

`generate_report` 输出 Markdown：

```
# 今日跟进 — {YYYY-MM-DD}

共 {N} 位客户需要跟进。

## 1. {客户名} · {阶段} · 触发：{规则}

- 上次联系：{时间}，对话要点：{摘要}
- 群聊信号：{找到的关键词或"无"}
- 外部动态：{找到的或"无"}
- 建议下一步：{基于以上的建议，1-2 句}
```

### 5. 写回 memory

为每个入选客户调用 `memory_save`：
```json
{ "customerName": "...", "stage": "...", "lastContact": "...", "todayPushedAt": "..." }
```

便于下次判 stage 停滞。

### 6. 反向同步（用户对话中触发）

当用户回复"客户 X 今天电话沟通了，下周签合同"等更新：

1. 解析意图，识别要更新的字段（lastContact / nextAction / notes / stage）
2. 在对话中明示："我将把『{客户名}』的 lastContact 更新为今天，nextAction 更新为'签合同'，nextActionDate 更新为下周一。请确认。"
3. 等用户确认后通过 dws 调用对应的 record update 命令（实际命令以 dingtalk-workspace SKILL 为准）
4. 同步 `memory_save` 当次跟进笔记
