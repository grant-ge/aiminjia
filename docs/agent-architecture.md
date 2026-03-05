# Agent 架构文档

> 技术架构、编排机制与安全设计的权威参考。

---

## 1. 架构总览

```
Frontend (React + TS + TailwindCSS 4)
    │ Tauri IPC
    ▼
commands/chat.rs ── SkillRegistry::detect_activation → Skill routing
    │                → cloud mode gate (use_cloud + auth_manager)
    │                → agent_loop(StepConfig) → finish_agent
    ▼
plugin/ (插件系统)
    ├─ ToolRegistry · SkillRegistry · PluginContext
    ├─ builtin/tools/ · builtin/skills/
    └─ declarative_skill (TOML + Markdown)
    ▼
gateway.rs ── masking.rs ── router.rs ── prompts.rs
    ▼
auth/ · storage/ · python/ · search/
```

| 能力 | 实现 | 关键文件 |
|------|------|----------|
| LLM 对话 + Tool Use | 流式请求 + Agent Loop | `gateway.rs`, `chat.rs` |
| LLM 上下文优化 | 三层压缩：单次压缩 + 递进衰减 + 结构化画像 | `tool_executor/python.rs`, `context_decay.rs`, `analysis_context.rs` |
| 云端模式 | 登录 → use_cloud 开关 → session_key → Lotus provider | `auth/`, `providers/lotus.rs`, `router.rs` |
| 6 步分析编排 | 显式状态机 + Skill trait | `chat.rs`, `skill_trait.rs` |
| Python 数据分析 | 分析模式持久 REPL / 日常模式一次性子进程 | `session.rs`, `runner.rs` |
| PII 脱敏 | 全链路 mask/unmask | `masking.rs` |
| 文件解析 | Python 解析器（CSV/Excel/JSON/PDF/Word/PPT/HTML/Text），Excel 使用实际总行数 | `python/parser.rs` |
| 报告生成 | HTML/PDF/DOCX（PDF/DOCX 从 HTML 自动转换） | `tool_executor/report.rs` |
| 联网搜索 | SearXNG 免费优先 + Bocha/Tavily 付费降级 | `search/` |
| 多会话并发 | HashMap 3 槽位 + RAII 守卫 | `gateway.rs`, `chat.rs` |
| 崩溃恢复 | run.lock（Session UUID）+ 孤儿检测 | `lib.rs`, `chat.rs` |

---

## 2. 模式状态机

`conv.json` 的 `mode` 字段是分析流程的**唯一真相源**。

| 模式 | 值 | 含义 |
|------|----|------|
| 日常 | `"daily"` | 普通对话，6 个工具可用 |
| 确认中 | `"confirming"` | Step 0 运行中 |
| 分析中 | `"analyzing"` | Step 1~5 执行中 |

**转换规则**：

```
daily ──[Skill 激活 + has_files]──→ confirming ──[用户确认]──→ analyzing ──[Step 5 完成/取消]──→ daily
                                                      │                           │
                                                      └──[取消]──→ daily          └──[用户取消]──→ daily
```

- Skill 激活：`SkillRegistry::detect_activation()` 通过关键词匹配 + 文件检测
- 分析完成/中止后 60 秒冷却期，防止误触发
- 分析中的日常问题（`is_daily_question()`）路由到 daily agent，**不改变 mode**
- `analysis.json` 仅追踪当前步骤，不决定是否在分析

---

## 3. 6 步分析流程

| 步骤 | 工具子集 | 最大迭代 |
|------|----------|----------|
| Step 0 方向确认 | `load_file`, `save_analysis_note` | 5 |
| Step 1 数据清洗 | `load_file`, `execute_python`, `save_analysis_note`, `update_progress` | 15 |
| Step 2 岗位归一化 | `execute_python`, `web_search`, `save_analysis_note`, `update_progress` | 15 |
| Step 3 职级推断 | `execute_python`, `web_search`, `save_analysis_note`, `update_progress` | 15 |
| Step 4 公平性诊断 | `execute_python`, `hypothesis_test`, `detect_anomalies`, `generate_chart`, `save_analysis_note`, `update_progress` | 20 |
| Step 5 行动方案 | `execute_python`, `generate_report`, `generate_chart`, `export_data`, `update_progress` | 15 |

步骤 prompt 在 `plugins/comp-analysis/prompts/step{0-5}.md`，修改 prompt 不需要改 Rust 代码。

---

## 4. Agent Loop

每步分析作为独立子 Agent，由 `agent_loop()` 驱动：

```
agent_loop(AgentContext, messages, StepConfig)
    ├─ 构建消息列表（system prompt + history）
    ├─ P2: AnalysisContext.load_or_default() — 加载持久化文件画像
    ├─ system prompt += file_context + analysis_notes + AnalysisContext.format_for_prompt()
    ├─ PhaseTracker (TAOR 阶段追踪 → 前端显示"思考/执行/整理")
    └─ Loop (max_iterations):
         ├─ P1: apply_decay(messages) — 非破坏性衰减旧迭代 tool output
         ├─ LLM 流式请求 (gateway.stream_message, 用衰减后的 messages)
         ├─ 纯文本 → 输出到前端
         ├─ Tool Call → 执行（多个时 join_all 并行）→ 结果追加 → 继续
         │       ├─ P0: compact_python_output() — 压缩 pandas 表格输出
         │       └─ P2: AnalysisContext.update_from_*() + save() — 增量更新文件画像
         ├─ EndTurn → 退出
         └─ 超时(90s)/cancel → 终止
```

**Token 预算**：日常 4096，分析 8192。

**Tool 结果压缩**：`compress_tool_result()` 剥离 execute_python 的 Purpose/Exit code/Execution time 头部。

---

## 5. LLM 上下文优化（三层）

分析流程中 `execute_python` 输出会累积进 LLM 上下文，三层优化将步骤内上下文从潜在 80K 降到 ~10K chars。

### P0: 单次输出压缩 (`compact_python_output`)

| 文件 | `llm/tool_executor/python.rs` |
|------|------|
| 阈值 | 4000 chars |
| 效果 | 单次 ≤8K → ~2-3K |

超阈值时自动检测并压缩 pandas 输出模式：
- **DataFrame 表格**：保留 header + 前 3 行 + 末行 + `[...N more rows]`
- **`describe()` 输出**：保留 count/mean/std/min/max，折叠百分位到一行
- **`value_counts()`**：保留 top 5 + bottom 1 + total count
- **非表格文本**：完整保留

### P1: 步骤内递进衰减 (`apply_decay`)

| 文件 | `llm/context_decay.rs` |
|------|------|
| 效果 | 10 次迭代 ~80K → ~10K |

每次 `stream_message` 前对 messages 做**非破坏性**衰减（返回新 Vec，原始 messages 不变）：
- 最近 1 轮迭代：全量保留
- 前 1 轮迭代：截断到 2K chars
- 更早迭代：截断到 500 chars

"迭代"= assistant_with_tool_calls + 后续 tool results。仅 analysis 模式生效。

`checkpoint_extract()` 和 `auto_capture_step_context()` 仍看到全量数据。

### P2: 结构化分析上下文 (`AnalysisContext`)

| 文件 | `llm/analysis_context.rs` |
|------|------|
| 持久化 | `analysis/{conv_id}/_analysis_ctx.json`（crash-safe: tmp + rename）|
| 效果 | system prompt 增加 ~1-2K 固定画像，替代重复的文件发现 |

```rust
AnalysisContext {
    files: Vec<FileProfile>,      // 每文件: 列名/类型/null%/统计/变量名
    step_findings: Vec<Finding>,  // 当前步骤发现 (category + summary)
    data_insights: Vec<String>,   // 累积洞察
    column_mapping: Option<Value>,// 列映射结果
    current_step: u32,
}
```

更新时机：
- `load_file` tool result → `update_from_load_file()` 提取文件结构
- `execute_python` tool result → `update_from_python_output()` 提取 `__ANALYSIS_FINDING__` / `__DATA_INSIGHT__` / `__COLUMN_MAPPING__` 标记
- 每次迭代后 `save()` 写磁盘

### 综合效果

| 指标 | 优化前 | 优化后 |
|------|--------|--------|
| 单次 tool output | ≤8K chars | ≤3K chars (P0) |
| 10 次迭代累计 | ~80K chars | ~10K chars (P1) |
| 文件基础信息 | 每次重复输出 | 1-2K 固定画像 (P2) |
| 信息丢失 | 无 | 无（checkpoint/auto_capture 仍看全量）|

---

## 6. 跨步骤记忆（三层保障）

步骤切换时消息列表从零重建，通过三层机制保留上下文：

| 优先级 | 机制 | 存储 key | 说明 |
|--------|------|----------|------|
| 1（最高） | `checkpoint_extract` | `step{N}_checkpoint` | 步骤切换时 LLM 提取结构化 JSON |
| 2 | `save_analysis_note` | `step{N}_summary` | 分析过程中 LLM 主动保存 |
| 3（兜底） | `auto_capture_step_context` | `step{N}_auto_context` | 系统自动提取 assistant 结论 + Python 输出 |

同步骤只选最高优先级来源注入 `[前序分析记录]`。Checkpoint 的 summary/key_findings/next_step_input 永不截断，data_artifacts 对远步骤衰减。

**日常模式压缩**：滑动窗口 30 条，超 24K 字符时 LLM 生成摘要替换旧消息（20s 超时，失败降级）。

---

## 7. 确认机制

`Skill::on_step_complete()` 返回 `StepAction`：

| 动作 | 含义 |
|------|------|
| `AdvanceToStep(id)` | 推进到指定步骤 |
| `WaitForUser` | 重跑当前步（用户有反馈） |
| `Finish` | 分析完成 → daily |
| `Abort` | 取消 → daily |

确认/取消词精确匹配（不做子串匹配），20 字符截断。无自动推进，所有步骤推进由用户消息触发。

---

## 8. 工具体系

### 10 个内置 Tool

| 工具名 | 功能 | 关键参数 |
|--------|------|----------|
| `web_search` | 联网搜索 | `query`* |
| `execute_python` | Python 代码执行 | `code`*, `purpose` |
| `load_file` | 加载文件 → `_df`/`_text`（同文件缓存） | `file_id`* |
| `generate_report` | HTML/PDF/DOCX 报告 | `title`*, `sections`*[] |
| `generate_chart` | Plotly 可视化 | `chart_type`*, `title`*, `data` |
| `hypothesis_test` | 统计假设检验 | `test_type`*, `groups`*[] |
| `detect_anomalies` | 异常值检测 | `column`*, `method` |
| `save_analysis_note` | 跨步骤记忆 | `key`*, `content`* |
| `export_data` | 导出 CSV/Excel/JSON | `format`*, `filename`* |
| `update_progress` | 更新进度条 | `current_step`*, `step_status`* |

### 工具过滤（双层防御）

1. **Schema 层**：仅向 LLM 暴露当前步骤允许的工具 Schema
2. **执行层**：运行时拦截幻觉出的未授权工具调用

日常模式排除 4 个分析专用工具（hypothesis_test, detect_anomalies, save_analysis_note, update_progress）。

---

## 9. 系统提示词

```
声明式 Skill:  [app_base] + [plugin_base] + [step_prompt] + [日期注入]
Legacy:       SYSTEM_PROMPT_BASE + SYSTEM_PROMPT_DAILY + [日期注入]
```

- 日期注入防 LLM 使用训练截止日期
- **数据真实性铁律**：禁止构造数据、所有数据来自 execute_python 实际结果、失败如实告知
- **文件描述真实性**：描述文件时必须严格基于 load_file 返回的 columns/rowCount/sampleData，禁止根据文件名或常识推测字段
- Extract prompts（`prompts/extract/`）仅在步骤切换时用于 checkpoint 提取

---

## 10. PII 脱敏

**硬编码 Strict 级别**，不可通过设置更改。

| 检测器 | 替换格式 | 策略 |
|--------|----------|------|
| 人名 | `[PERSON_N]` | 关键词后 2-4 中文字符 |
| 公司名 | `[COMPANY_N]` | 公司后缀向前提取 |
| 邮箱 | `[EMAIL_N]` | `@` 符号扩展 |
| 手机号 | `[PHONE_N]` | 11 位 `1[3-9]` 开头 |

全链路：用户消息 mask → LLM 请求 → Tool 结果 mask → LLM 响应 unmask → 保存/前端。

---

## 11. Python 沙箱

- **分析模式**：持久 REPL 会话（`session.rs`），跨 Tool Call 保留变量
- **日常模式**：一次性子进程（`runner.rs`），执行完销毁
- **安全**：进程隔离 + 路径校验（`canonicalize + starts_with(workspace)`）+ 超时 120s + 输出截断
- **多文件**：`_dfs`/`_texts` 字典（UUID key），`_df`/`_text` 指向最后加载的文件

---

## 12. 搜索引擎

**本地模式**：三源降级 SearXNG（免费）→ Bocha（付费，中文增强）→ Tavily（付费，全球）。

**云端模式**：`use_cloud=true` 时优先通过 Lotus `/v1/search` 接口，失败时降级到本地搜索链路。`use_cloud=false` 时跳过云端搜索。

---

## 13. 云端认证

**双模式架构**：未登录使用本地 API Key，登录后通过 `use_cloud` 设置显式控制是否走 Lotus 云端网关。

```
登录流程：username/password → JWT(access+refresh) → session_key(sk-sess***)
续期链：session_key过期 → 用access_token创建新key → access_token过期 → refresh → 创建新key
全部过期 → 发射 auth:expired 事件 → 前端提示重新登录（不自动切换到本地）
```

| 组件 | 文件 | 职责 |
|------|------|------|
| AuthClient | `auth/client.rs` | Lotus HTTP API（login/refresh/session_key/models） |
| AuthManager | `auth/mod.rs` | 状态管理 + Token 自动续期 + 加密持久化 |
| CloudAuth | `auth/state.rs` | 认证状态类型定义 |
| IPC 命令 | `commands/auth.rs` | `cloud_login`/`cloud_logout`/`get_cloud_auth`/`get_cloud_models` |
| Lotus Provider | `providers/lotus.rs` | OpenAI 兼容格式，Bearer session_key 认证 |

**云端路由**：`chat.rs` 在每次 `send_message` 时检查 `settings.use_cloud`：
- `use_cloud=true` + session_key 有效 → 覆盖 `primary_model="lotus"` + `primary_api_key=session_key` → `router.rs` 路由到 Lotus provider
- `use_cloud=true` + 未登录 → 报错提示登录或切换本地
- `use_cloud=true` + auth 过期 → 发射 `auth:expired` + 报错提示切换本地或重新登录
- `use_cloud=false` → 跳过云端，使用本地 settings

搜索同理：`PluginContext.use_cloud` 控制是否走云端 `/v1/search`。

**安全边界**：
- Token TTL 校验：`expires_in <= 0` 时拒绝，防止服务端异常导致即时过期循环
- `get_auth_info()` 在 write lock 下双重检查再清除，防止并发 logout 覆盖新 login
- 解密失败时 fallback 到明文并记录 warn 日志（兼容迁移场景）
- 云端搜索复用静态 `reqwest::Client`（`once_cell::Lazy`），避免每次请求创建新连接池

---

## 14. 并发与恢复

- **多会话**：HashMap 3 槽位 + RAII 守卫
- **崩溃恢复**：`run.lock`（Session UUID）+ 孤儿检测
- **超时**：chunk 90s，cancel 立即退出

---

## 16. 更新历史

### v0.3.12 (2026-03-05)

**修复**：
- **Excel 行数统计准确性** - `python/parser.rs` 修复采样导致的行数错误，现在使用 `_ws.max_row` 获取实际总行数而非采样 DataFrame 长度（修复 1095 行被错报为 150 行的问题）
- **LLM 文件描述幻觉** - `prompts/base.md` 新增"文件描述真实性"规则，强制 LLM 严格基于 `load_file` 返回的实际字段描述文件，禁止根据文件名或常识推测不存在的字段（如工资表中不存在"姓名"字段时不能说"包含姓名"）
- **工具失败率优化** - `tool_executor/export.rs` 和 `tool_executor/report.rs` 增强参数验证和错误提示，降低 LLM 传递错误参数的概率

**CI/CD**：
- 修复 GitHub Actions 中 `python-runtime` 目录不存在导致的构建失败
- 简化构建流程，移除 GitHub Release 创建（仅构建 artifacts）
- GitHub 仓库清理：删除 `docs/` 和 `CLAUDE.md`（仅保留在 Codeup）

---

## 15. 安全加固

- AES-256-GCM 加密 API Key 和云端认证令牌（密钥存 OS Keychain）
- 云端 session_key 自动续期，过期通知前端
- 工具参数通过临时 JSON 文件传递（防注入）
- RAII 守卫管理并发槽位（防泄漏）
- 所有文件路径 canonicalize + workspace 边界校验
- 所有文件路径 canonicalize + workspace 边界校验
