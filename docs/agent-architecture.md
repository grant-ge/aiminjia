# Agent 架构设计文档

> AI小家 — 薪酬分析 Agent 的核心架构、编排机制与安全设计

---

## 目录

1. [概述](#1-概述)
2. [模式状态机](#2-模式状态机)
3. [6 步分析流程](#3-6-步分析流程)
4. [子 Agent 架构](#4-子-agent-架构)
5. [确认机制](#5-确认机制)
6. [工具体系](#6-工具体系)
7. [系统提示词体系](#7-系统提示词体系)
8. [PII 脱敏全链路](#8-pii-脱敏全链路)
9. [多会话并发](#9-多会话并发)
10. [模型路由](#10-模型路由)
11. [流式通信协议](#11-流式通信协议)
12. [崩溃恢复](#12-崩溃恢复)
13. [Python 沙箱](#13-python-沙箱)
14. [搜索引擎](#14-搜索引擎)
15. [安全加固与资源管理](#15-安全加固与资源管理)

---

## 1. 概述

### 1.1 产品定位

AI小家是面向 HR 专业人员的 Tauri 2.x 桌面 AI Agent 应用。Agent 具备**双模式**运行能力：

- **日常咨询模式（Daily）**：通用 HR 助手，覆盖数据处理、HR 咨询、文档模板、翻译四大场景
- **薪酬分析模式（Analysis）**：结构化 6 步分析流程（Step 0 方向确认 → 数据清洗 → 岗位归一化 → 职级推断 → 公平性诊断 → 行动方案）

### 1.2 核心能力

| 能力 | 实现 | 关键文件 |
|------|------|----------|
| LLM 对话 + Tool Use | 流式请求 + Agent Loop | `gateway.rs`, `chat.rs` |
| 6 步分析编排 | 显式状态机 + 子 Agent | `orchestrator.rs` |
| Python 数据分析 | 沙箱子进程执行 | `runner.rs`, `sandbox.rs` |
| PII 脱敏 | 全链路 mask/unmask | `masking.rs` |
| 文件解析 | Python 解析器 | `python/parser.rs` |
| 报告生成 | HTML/PDF/DOCX 富内容渲染（PDF/DOCX 从 HTML 自动转换） | `tool_executor.rs` |
| 联网搜索 | Bing（免费优先）+ Tavily（付费增强/降级） | `search/bing.rs`, `search/tavily.rs` |
| 多会话并发 | HashMap 3 槽位 + RAII 守卫 | `gateway.rs`, `chat.rs` |
| 崩溃恢复 | run.lock + 孤儿检测 | `lib.rs`, `chat.rs` |

### 1.3 架构总览

```
┌─────────────────────────────────────────────────────────┐
│                      Frontend (React)                    │
│   chatStore ─── useStreaming ─── Tauri Events ──────────│
└────────────────────────┬────────────────────────────────┘
                         │ IPC (send_message / stop_streaming)
┌────────────────────────▼────────────────────────────────┐
│                   commands/chat.rs                        │
│   SkillRegistry::detect_activation → Skill routing       │
│   → agent_loop(StepConfig) → finish_agent                │
├──────────────────────────────────────────────────────────┤
│                   plugin/ (插件系统)                       │
│   ToolRegistry · SkillRegistry · PluginContext            │
│   builtin/tools/ · builtin/skills/ · declarative_skill   │
├──────────┬───────────┬───────────┬───────────────────────┤
│ gateway  │ masking   │ router    │ prompts               │
│ 流式请求 │ PII 脱敏  │ 模型路由  │ 系统提示词            │
├──────────┴───────────┴───────────┴───────────────────────┤
│                    storage/file_store/                    │
│   会话 · 消息 · 分析状态 · 企业记忆 · 审计日志          │
└──────────────────────────────────────────────────────────┘
```

---

## 2. 模式状态机

### 2.1 三种会话模式

`conv.json` 的 `mode` 字段是分析流程的**唯一真相源**。`analysis.json` 仅追踪"当前在哪一步"，不决定"是否在分析"。

| 模式 | 值 | 含义 |
|------|----|------|
| 日常 | `"daily"` | 普通对话，全工具可用 |
| 确认中 | `"confirming"` | Step 0 正在运行，等待用户确认分析方向 |
| 分析中 | `"analyzing"` | Step 1~5 正在执行 |

### 2.2 模式转换流程

```
                    detect_analysis_mode()
                    检测到分析意图
                         │
     ┌───────────────────▼──────────────────┐
     │              daily                    │
     │         （日常咨询模式）               │
     └───────────────────┬──────────────────┘
                         │ 用户上传文件 + 薪酬关键词
                         │ 或显式请求分析
                         ▼
     ┌───────────────────────────────────────┐
     │           confirming                   │
     │    （Step 0: 分析方向确认）             │
     │    · 识别文件类型和结构                 │
     │    · 概括文件内容                       │
     │    · 询问关注方向                       │
     └───────────────────┬──────────────────┘
                         │ 用户回复任意内容
                         │ (is_confirmation 或 反馈)
                         ▼
     ┌───────────────────────────────────────┐
     │            analyzing                   │
     │     （Step 1~5: 结构化分析）           │
     │     · 每步独立子 Agent                 │
     │     · 确认卡点暂停                     │
     │     · 用户确认推进/反馈重跑             │
     └───────────────────┬──────────────────┘
                         │ Step 5 确认后
                         │ finalize_analysis()
                         ▼
     ┌───────────────────────────────────────┐
     │              daily                    │
     │        （返回日常模式）                │
     └──────────────────────────────────────┘

     * 任何阶段用户说"取消/abort" → AbortAnalysis → daily
```

### 2.3 分析意图检测

`plugin/registry.rs:SkillRegistry::detect_activation()` 通过 Skill 的 `activation_keywords()` 方法检测：

**方式一：显式关键词**（中英文）
- 中文：`薪酬分析`、`薪资分析`、`薪酬公平`、`薪资公平`、`薪酬诊断`、`pay equity`…
- 匹配逻辑：消息文本包含任一关键词即触发对应 Skill

**方式二：文件上传 + 薪酬关键词**
- 消息中包含文件引用（file_id）
- 且消息文本包含薪酬相关词汇（`薪酬`、`薪资`、`工资`、`salary`…）

### 2.4 mode 与 analysis.json 的职责分离

```
conv.json                       analysis.json
─────────                       ──────────────
mode: "daily"                   (无记录或 finalized)
mode: "confirming"              current_step: 0, status: "in_progress"
mode: "analyzing"               current_step: 1~5, status: "in_progress"/"completed"/"paused"
```

---

## 3. 6 步分析流程

### 3.1 流程总览

| 步骤 | 目标 | 工具子集 | 最大迭代 | 输出物 |
|------|------|----------|----------|--------|
| Step 0 | 方向确认 | `analyze_file`, `save_analysis_note` | 5 | 文件摘要 + 方向偏好 |
| Step 1 | 数据清洗 | `analyze_file`, `execute_python`, `save_analysis_note`, `update_progress` | 15 | 字段映射、排除清单、薪酬结构、数据质量 |
| Step 2 | 岗位归一化 | `execute_python`, `web_search`, `save_analysis_note`, `update_progress` | 15 | 行业推断、岗位族方案、归一化映射表 |
| Step 3 | 职级推断 | `execute_python`, `web_search`, `save_analysis_note`, `update_progress` | 15 | 职级通道、IPE 粗定级、薪酬聚类、异常标记 |
| Step 4 | 公平性诊断 | `execute_python`, `hypothesis_test`, `detect_anomalies`, `generate_chart`, `save_analysis_note`, `update_progress` | 20 | 6 维诊断结果、根因分析、高优先级异常清单 |
| Step 5 | 行动方案 | `execute_python`, `generate_report`, `generate_chart`, `export_data`, `update_progress` | 15 | 三档调薪方案、ROI 计算、HTML 报告、Excel 明细 |

### 3.2 各步骤详解

#### Step 0 — 分析方向确认

**触发**：`detect_analysis_mode()` 检测到分析意图，`chat.rs` 将 `mode` 设为 `confirming`

**Agent 行为**：
1. 调用 `analyze_file` 识别文件类型和结构（列名、行数、样本）
2. 一句话概括文件内容（如"197 人月度薪资明细"）
3. 告知 5 步分析流程
4. 询问用户是否有特定关注方向
5. 通过 `save_analysis_note(key="analysis_direction")` 保存方向偏好

#### Step 1 — 数据清洗与理解

**8 步执行流程**：
1. 调用 `analyze_file(file_id)` 获取 `filePath`
2. Python 加载完整数据，输出 shape + 前 5 行
3. 字段语义映射（基本信息 / 薪酬字段 / 汇总字段 / 辅助字段）
4. 排除无效记录（当月入职、已离职、非全职、试用期 ≤3 月、基本工资=0）
5. 导出排除人员 Excel + 聊天预览前 15 行
6. 薪酬结构分析：固定（基本工资 + 津贴 + 补贴）vs 浮动（绩效 + 提成 + 加班 + 奖金）
7. 数据质量评估
8. `save_analysis_note(key="step1_summary")` 保存结论

#### Step 2 — 岗位归一化与岗位族构建

**5 部分执行**：
1. 行业推断（部门/岗位关键词 + 人员分布 + 公司规模）
2. 推荐岗位族方案（6 套行业 × 规模模板，如制造业 500+ 人→8 族）
3. 岗位名称归一化（去前缀、合并同义词、保留级别差异）
4. 语义聚类 + 薪酬验证（薪酬分布重叠度校验低置信度分组）
5. 输出映射表 + 低置信度分类标记

#### Step 3 — 职级框架推断

**解决"鸡生蛋"问题**：不能用薪酬定义职级、再用职级分析薪酬公平性（循环论证）。

**三阶段推断**：
- **Stage A**：非薪酬信号（管理关键词、部门规模、汇报层级）→ 粗定级（简化 IPE 模型）
- **Stage B**：薪酬聚类（Jenks/K-means）→ 子级别（如 P3a/P3b/P3c）
- **Stage C**：交叉验证（司龄独立维度：高司龄低薪→疑似偏低、低司龄高薪→疑似倒挂）

**4 套职级通道模板**：
| 模板 | 适用 | 序列 | 总级数 |
|------|------|------|--------|
| 四序列 | 制造业 500+ | P(7)/S(5)/O(4)/M(4) | 20 |
| 双通道 | 互联网/科技 | P(IC)/M(Manager) | 14 |
| 三通道 | 混合型 | T(Tech)/B(Business)/M(Manager) | 16 |
| 单序列 | <100 人 | L1~L8 | 8 |

#### Step 4 — 公平性诊断（6 维度）

| 维度 | 方法 | 异常阈值 |
|------|------|----------|
| 岗位内公平 | CV、极差比、IQR/中位数 | CV>20% 或 Max/Min>2.0 为 🔴 |
| 跨岗位公平 | 同级不同岗中位数比较 | 偏离总体中位数 >15% 为 🟡 |
| 薪酬-司龄回归 | ln(salary) = β0 + β1×级别 + β2×司龄 + ε | ±1.65 SD（90% CI）为显著异常 |
| 薪酬倒挂 | 同岗同级新老员工中位数比较 | 新员工(≤2yr) > 老员工(≥5yr) 为 🔴 |
| 薪酬结构 | 固浮比 vs 岗位族基准 | 管理/专业固定≥70%，销售浮动 40-60%，操作固定≥80% |
| 内部 CR | CR = 个人固薪 / 群体中位数 × 100% | <80% 🔴 / 80-90% 🟡 / 90-110% 🟢 / >120% 🔴 |

**根因框架**（6 类）：
1. 低入职薪 + 无调薪机制
2. 岗位职责升级但薪酬未跟进
3. 地域差异未体现
4. 外部市场溢价招聘导致倒挂
5. 部门/岗位族间系统性偏差
6. 缺乏定期岗位评估

#### Step 5 — 行动方案与报告生成

**三档调薪预算**：
| 方案 | 策略 | 调整对象 |
|------|------|----------|
| A（修复严重） | CR<80% + 严重倒挂 → 调至 P25 | 最少人 |
| B（修复中等，推荐） | CR<80%→P25, CR 80-90%→P40 | 适中 |
| C（全面对齐） | 所有 CR→90%+ | 最多人，最贵 |

**ROI 计算**：投资额 vs 避免损失（核心人才替换成本 + 士气影响 + 未来补救成本）

**HTML 报告**（9 章节）：执行摘要 → 数据概览 → 岗位体系 → 6 维诊断 → 高优异常 → 三档方案 → ROI → 实施路线图 → 制度建议

---

## 4. 子 Agent 架构

### 4.1 Agent Loop 机制

每一步分析作为独立的子 Agent 运行，由 `chat.rs` 中的 `agent_loop()` 驱动：

```
agent_loop(StepConfig)
    │
    ├─ 构建消息列表（系统提示词 + 历史消息）
    │
    └─ Loop (最多 max_iterations 次):
         │
         ├─ gateway.stream_message() → LLM 流式响应
         │
         ├─ 解析响应：
         │    ├─ 纯文本 → 输出到前端
         │    └─ Tool Call → 执行工具 → 结果追加到消息列表 → 继续循环
         │
         ├─ StopReason::EndTurn → 退出循环
         ├─ StopReason::ToolUse → 继续循环
         │
         └─ 超时检测：
              ├─ chunk 超时 (90s) → 终止流
              └─ cancel 信号 → 立即退出
```

### 4.2 上下文重置与三层保障

**步骤切换时（`StartAnalysis` / `AdvanceStep`）**，消息列表从零构建，但在清除前通过三层机制保留上下文：

```
旧步骤 (Step N)                    三层保障                         新步骤 (Step N+1)
─────────────────                  ──────────                      ─────────────────
System Prompt (Step N)             ① checkpoint_extract()           System Prompt (Step N+1)  ← 新的
User Message 1                        → Skill 提供 extract prompt   [前序分析记录]            ← 从 memory.jsonl 注入
Tool Call: analyze_file                → 非流式 LLM 调用提取结构化       (checkpoint 优先 > summary > auto)
Tool Result: {...}                     → 存为 step{N}_checkpoint
Assistant: "数据清洗完成..."        ② auto_capture_step_context()   User: "确认，继续"
User: "确认"                           → 提取最后 assistant 结论    Assistant: "前 N 步分析已完成..."
                                       → 提取 execute_python 输出
                                       → 存为 step{N}_auto_context
                                   ③ save_analysis_note (LLM 主动)
                                       → 分析过程中已保存
                                   ※ 随后消息历史清空
```

**Layer 1: checkpoint_extract** (`llm/checkpoint.rs`): 步骤切换时，使用 Skill 提供的 extract prompt（通过 `Skill::extract_prompt(step_id)` 获取）发起独立非流式 LLM 调用，提取结构化 `StepCheckpoint` JSON（summary / key_findings / data_artifacts / decisions / next_step_input）。30 秒超时，失败降级到 Layer 2/3。声明式 Skill 的 extract prompt 从插件目录 `prompts/extract/base_extract.md` + `prompts/extract/extract_{step_id}.md` 加载。如果 Skill 未提供 extract prompt（两个部分均为空），则跳过提取。

**Layer 2: auto_capture_step_context** (`chat.rs`): 在消息历史清除前，自动提取当前步骤的关键输出并保存为 `note:{conv_id}:step{N}_auto_context`（最大 6000 字符，最多 5 个工具输出各 1500 字符，2 条 assistant 消息），作为 checkpoint 失败时的安全兜底。

**Layer 3: save_analysis_note** (LLM 主动): 分析过程中 LLM 通过工具主动保存 `step{N}_summary`，作为补充。

### 4.3 记忆传递机制

跨步骤的关键结论通过**三层优先级**传递到后续步骤的系统提示词：

1. **Checkpoint 提取**（`checkpoint_extract` → `step{N}_checkpoint`）— 最高优先级，结构化 JSON（StepCheckpoint），使用字段级衰减
2. **LLM 主动保存**（`save_analysis_note` → `step{N}_summary`）— 高质量结构化摘要
3. **系统自动捕获**（`auto_capture_step_context` → `step{N}_auto_context`）— 兜底，防止 LLM 遗漏

注入时按 **checkpoint > summary > auto_context** 优先级选择（同一步骤只选最高优先级来源）。Checkpoint 的 `summary`/`key_findings`/`next_step_input` 永不截断，`data_artifacts` 对远步骤衰减到 2000 字符。步骤显示名称从 Skill 的 `workflow()` 定义获取。

```
Step 1 (数据清洗)                              memory.jsonl
    │                                          ──────────────
    ├─ save_analysis_note(                     note:{conv_id}:step1_summary
    │    key="step1_summary",                  = "有效人数 180, 排除 17 人..."
    │    content="有效人数 180..."
    │  )
    │                                          note:{conv_id}:step1_auto_context
    ├─ auto_capture_step_context()  ──────►    = "[分析结论] 数据清洗完成...
    │   (步骤切换时系统自动执行)                 │   [关键数据输出] df.shape=(180,25)..."
    │                                          │
Step 2 (岗位归一化)                             │
    │                                          │
    ├─ 系统提示词中注入:                        │
    │   "[前序分析记录]                         │
    │    ## 第 1 步记录                         │
    │    step1_summary: 有效人数 180..."  ◄─────┤
    │    step1_auto_context: ..."        ◄─────┘
    │
    ├─ save_analysis_note(
    │    key="step2_summary", ...
    │  )
    │                                          note:{conv_id}:step2_summary
    ▼                                          note:{conv_id}:step2_auto_context
Step 3 (职级推断)
    │
    ├─ 系统提示词中注入:
    │   "[前序分析记录]
    │    ## 第 1 步记录
    │    step1_summary: ...          ← 全文保留
    │    step1_auto_context: ...     ← 截断至 2000 字符（旧步骤压缩）
    │    ## 第 2 步记录
    │    step2_summary: ...          ← 全文保留
    │    step2_auto_context: ...     ← 全文保留（最近完成步骤，最多 4000 字符）
```

**压缩策略**（适用 10+ 步骤场景）：
- `step{N}_summary`（LLM 保存）：始终全文保留
- 最近完成步骤的 `auto_context`：最多 6000 字符
- 更早步骤的 `auto_context`：截断至 3000 字符
- 估算：~2K(base prompt) + ~3K/步 ≈ 17K tokens (5步) / 32K tokens (10步)，在 64K DeepSeek / 200K Claude 上下文内

**存储格式**：企业记忆使用 JSONL（`memory.jsonl`），key 格式为 `note:{conversation_id}:{name}`，last-writer-wins 语义。

### 4.4 Token 预算

| 模式 | Token 预算 | 说明 |
|------|-----------|------|
| 日常咨询 | 4096 | 轻量对话，无需长推理 |
| 分析子 Agent | 8192 | 每步独立，需要完整推理 + 工具调用 |

### 4.5 Tool 结果压缩

`compress_tool_result()` 从历史消息中剥离 `execute_python` 的冗余头部，节省 Token：

```
压缩前：                              压缩后：
[Purpose: 计算 CR]                    员工A  CR=85.2%
Exit code: 0                          员工B  CR=72.1%
Execution time: 1.2s                  员工C  CR=110.5%

员工A  CR=85.2%
员工B  CR=72.1%
员工C  CR=110.5%
```

仅保留 stdout 输出和错误信息，剥离 Purpose/Exit code/Execution time 头部。

---

## 5. 确认机制

### 5.1 设计目标

每步分析完成后，Agent 停止等待用户确认，形成"分析→确认→推进"的人机协作节奏。用户可以：
- **确认**：推进到下一步
- **反馈**：重跑当前步（带用户反馈注入提示词）
- **取消**：中止分析，回到日常模式

### 5.2 StepAction 路由枚举

`skill_trait.rs:Skill::on_step_complete()` 返回以下动作，由 `chat.rs:send_message()` 执行路由：

```rust
enum StepAction {
    AdvanceToStep(String),  // 推进到指定步骤（如 "step2"）
    WaitForUser,            // 重跑当前步（用户有反馈）
    Finish,                 // 分析完成 → 回到 daily
    Abort,                  // 用户取消 → 回到 daily
}
```

**路由逻辑**（`send_message()` 中根据 conversation mode 分派）：

```
conversation.mode
    │
    ├─ "daily" ───── skill_registry.detect_activation()
    │                 ├─ 有 Skill 激活 → 进入 confirming
    │                 └─ 无 → DailyChat
    │
    ├─ "confirming" ── Skill.on_step_complete()
    │                   ├─ AdvanceToStep → 进入 analyzing
    │                   └─ Abort → 回到 daily
    │
    └─ "analyzing" ─── Skill.on_step_complete()
                        ├─ AdvanceToStep → 推进到下一步
                        ├─ WaitForUser → 重跑当前步
                        ├─ Finish → 回到 daily
                        └─ Abort → 回到 daily
```

### 5.3 确认词精确匹配

`is_confirmation()` 使用硬化的精确匹配策略，**不做子串匹配**：

**预处理**：
1. `trim()` + 转小写
2. 去除标点符号
3. **20 字符长度截断**——超过 20 字的消息视为反馈，不可能是简单确认

**匹配列表**（约 35 个）：

```
通用确认: 好/好的/可以/确认/继续/没问题/进行/下一步/ok/yes/sure/confirm/next/go/proceed...
Step 0 专用: 开始/开始分析/开始吧/start
复合短语: 好的继续/确认继续/可以继续/没问题继续...
```

### 5.4 取消词精确匹配

`is_abort()` 使用同样的硬化策略，约 25 个取消词：

```
中文: 取消/放弃/终止/停止/中止/退出/算了/不分析了/不做了/不需要了...
英文: cancel/abort/stop/quit/exit/nevermind...
```

### 5.5 无自动推进

`agent_loop` 没有自动推进外循环。所有步骤推进通过用户消息触发：

```
用户发消息 → send_message() → next_action() → AdvanceStep → agent_loop(下一步)
```

---

## 6. 工具体系

### 6.1 10 个 Tool 定义

| # | 工具名 | 功能 | 参数 |
|---|--------|------|------|
| 1 | `web_search` | 联网搜索市场数据、法规 | `query`*, `max_results`(5) |
| 2 | `execute_python` | 执行 Python 数据分析代码 | `code`*, `purpose` |
| 3 | `analyze_file` | 解析上传文件，返回结构信息 | `file_id`* |
| 4 | `generate_report` | 生成 HTML/PDF/DOCX/Markdown 报告 | `title`*, `sections`*[], `format` |
| 5 | `generate_chart` | 数据可视化（bar/line/scatter/box/heatmap） | `chart_type`*, `title`*, `data`, `options` |
| 6 | `hypothesis_test` | 统计假设检验 | `test_type`*, `groups`*[], `data_source`, `significance_level`(0.05) |
| 7 | `detect_anomalies` | 异常值检测（Z-score/IQR/Grubbs） | `column`*, `method`, `threshold`, `group_by` |
| 8 | `save_analysis_note` | 保存分析记忆供跨步骤使用 | `key`*, `content`*, `step` |
| 9 | `export_data` | 导出数据（CSV/Excel/JSON） | `data`*, `format`*, `filename`* |
| 10 | `update_progress` | 更新分析进度指示器 | `current_step`*, `step_status`*, `summary` |

> `*` 表示必填参数

### 6.2 按步骤过滤

`tools::get_tools_for_step(step)` 返回每步可用的工具子集：

```
Step 0 (方向确认):    analyze_file, save_analysis_note
                      ── 极小工具集，防止 Agent 提前清洗数据

Step 1 (数据清洗):    analyze_file, execute_python,
                      save_analysis_note, update_progress
                      ── 需要文件解析 + Python 执行

Step 2 (岗位归一化):  execute_python, web_search,
                      save_analysis_note, update_progress
                      ── 需要搜索行业基准

Step 3 (职级推断):    execute_python, web_search,
                      save_analysis_note, update_progress
                      ── 同上

Step 4 (公平性诊断):  execute_python, hypothesis_test,
                      detect_anomalies, generate_chart,
                      save_analysis_note, update_progress
                      ── 最丰富：统计检验 + 异常检测 + 可视化

Step 5 (行动方案):    execute_python, generate_report,
                      generate_chart, export_data,
                      update_progress
                      ── 报告生成 + 数据导出

日常模式:             全部 10 个工具
```

### 6.3 Tool 执行分发

`plugin/registry.rs:ToolRegistry::execute()` 是中央分发器，根据 `tool_name` 查找注册的 `ToolPlugin` 实现并调用：

```rust
// registry.rs
pub async fn execute(&self, name: &str, ctx: &PluginContext, args: Value) -> Result<ToolOutput> {
    let tool = self.tools.read().await.get(name)?.clone();
    tool.execute(ctx, args).await
}
```

### 6.4 生成文件检测

`execute_python` handler 通过双重机制检测生成的文件：

1. **结构化标记**：Python `_export_detail()` 在 stdout 输出 `__GENERATED_FILE__:{"fileId":"...","fileName":"...","filePath":"..."}` 标记行
2. **handler 解析**：从 stdout 中提取 JSON 元数据 → 注册到 AppStorage → 清理标记行 → 追加文件信息到 Tool 结果

---

## 7. 系统提示词体系

### 7.1 组合结构

```
Skill::system_prompt(state) → String
    │
    ├─ 声明式 Skill（从插件目录加载）
    │   ├─ [app_base]           prompts/base.md（可选，include_app_base=true）
    │   ├─ [plugin_base]        plugins/{id}/base.md
    │   ├─ [step_prompt]        plugins/{id}/prompts/step{N}.md
    │   └─ [日期注入]           【当前时间】今天是 YYYY年MM月DD日（YYYY-MM-DD）
    │
    └─ Legacy 路径（仅无 Skill 匹配时）
        ├─ SYSTEM_PROMPT_BASE   (始终包含)
        │   · 角色定义：组织咨询顾问 + 智能工作助手
        │   · 核心专长：薪酬公平性分析方法论
        │   · 可用工具说明
        │   · 数据真实性铁律（6 条）
        │   · 输出格式模板
        │   · Python 环境说明
        │   · 文件命名规则
        │
        └─ 模式专属提示词 + 日期注入
             │
             ├─ step=None  → SYSTEM_PROMPT_DAILY + 【当前时间】
             │   · 4 大工作场景（数据处理/HR 咨询/文档/翻译）
             │
             └─ step=Some(N) → 由 Skill 提供
```

薪酬分析的 6 步提示词现由 `plugins/comp-analysis/prompts/step{0-5}.md` 提供，不再硬编码在 `prompts.rs` 中。

**日期注入**：所有 LLM 调用路径均注入当前日期，格式为 `【当前时间】今天是 YYYY年MM月DD日（YYYY-MM-DD）。你的回答中涉及时间时，以此日期为准。` 使用醒目的中文格式 + ISO 格式双重展示，防止 LLM 使用训练数据截止日期（如 2024 年 6 月）作为"当前时间"。注入点：
- `prompts.rs:get_system_prompt()` — 日常模式 + legacy 分析模式
- `declarative_skill.rs:system_prompt()` — 声明式 Skill
- `checkpoint.rs:do_extract()` — 检查点提取的非流式 LLM 调用

### 7.2 Extract Prompts（检查点提取专用）

```
Skill::extract_prompt(step_id) → (base, step_specific)
    │
    └─ 声明式 Skill 从插件目录加载:
        ├─ base:          plugins/{id}/prompts/extract/base_extract.md
        └─ step_specific: plugins/{id}/prompts/extract/extract_{step_id}.md
```

Extract prompts 仅在步骤切换时由 `checkpoint_extract()` 使用，引导 LLM 提取结构化 `StepCheckpoint` JSON。未提供 extract prompt 的 Skill 将跳过 checkpoint 提取。

### 7.3 数据真实性铁律

嵌入在 `SYSTEM_PROMPT_BASE` 中，适用于所有模式：

```
【数据真实性铁律】— 违反任何一条即为严重错误：

1. 绝对禁止构造/虚构任何数据（姓名、数字、统计结果、排除名单）
2. 所有数据必须来自 execute_python 的实际执行结果
3. 如果代码执行失败，如实告知用户，不得编造输出
4. 使用员工 ID 而非姓名引用人员
5. 分析结论标注为"建议"而非"确定"
6. 不得手动改写数据表格
```

### 7.4 分析方向注入

Step 0 保存的 `analysis_direction` 通过 `save_analysis_note` 写入 `memory.jsonl`，在后续步骤的系统提示词构建时作为 `[前序分析记录]` 注入：

```
[前序分析记录]
analysis_direction: 用户关注销售团队的薪酬倒挂问题
step1_summary: 有效人数 180, 排除 17 人, 销售部 45 人...
step2_summary: 行业: 制造业, 8 族, 销售族 45 人...
```

这确保后续步骤的 LLM 在分析时会重点关注用户指定的方向。

---

## 8. PII 脱敏全链路

### 8.1 设计原则

**PII 保护不可妥协**——始终使用 `MaskingLevel::Strict`，在 `chat.rs` 中硬编码，不受设置面板控制。

### 8.2 脱敏级别

| 级别 | 脱敏内容 | 使用场景 |
|------|----------|----------|
| `Strict` | 人名 + 公司名 + 邮箱 + 手机号 | **唯一使用级别** |
| `Standard` | 人名 + 公司名 | 保留备用 |
| `Relaxed` | 不脱敏 | 保留备用 |

### 8.3 4 类检测器

**1. 人名检测 (`mask_person_names`)**
- 策略：关键词后跟中文字符（2-4 字）
- 关键词：`员工`、`姓名`、`负责人`、`经理`、`主管`、`总监`、`专员`、`总经理`、`董事`…（20 个）
- 生成占位符：`[PERSON_1]`、`[PERSON_2]`…

**2. 公司名检测 (`mask_company_names`)**
- 策略：查找公司后缀（`有限公司`、`股份有限公司`、`集团`…），向前提取中文前缀
- 后缀列表：10 个（`有限责任公司`、`股份有限公司`、`有限公司`、`集团公司`、`公司`、`企业`、`工厂`、`事务所`…）
- 生成占位符：`[COMPANY_1]`、`[COMPANY_2]`…

**3. 邮箱检测 (`mask_emails`)**
- 策略：查找 `@` 符号，向前后扩展有效字符
- 仅 Strict 级别启用
- 生成占位符：`[EMAIL_1]`、`[EMAIL_2]`…

**4. 手机号检测 (`mask_phones`)**
- 策略：匹配 11 位中国手机号（`1[3-9]` 开头）
- 排除更长数字序列中的子串
- 仅 Strict 级别启用
- 生成占位符：`[PHONE_1]`、`[PHONE_2]`…

### 8.4 全链路覆盖

```
用户消息
    │
    ▼ mask_messages()
┌─────────────┐
│ MaskingContext│ ─── mask_map: {"张三" → "[PERSON_1]", ...}
└──────┬──────┘     unmask_map: {"[PERSON_1]" → "张三", ...}
       │
       ▼ 脱敏后的消息
┌─────────────┐
│  LLM 请求   │  ← LLM 看到的是 [PERSON_1]，不是真实姓名
└──────┬──────┘
       │
       ▼ LLM 响应（含 [PERSON_1]）
       │
       ├─ Tool Call 返回的数据 ── mask_text() ──→ 追加到对话历史（脱敏后）
       │
       ▼ 最终响应文本
┌─────────────┐
│   unmask()   │ ─── [PERSON_1] → "张三"
└──────┬──────┘
       │
       ▼ 还原后保存到文件存储 + 发送到前端
```

**三个脱敏点**：
1. **用户消息**：`gateway.stream_message()` 内部 `mask_messages()` 后发送给 LLM
2. **Tool 执行结果**：Tool 返回的文件数据经 `mask_text()` 脱敏后追加到对话历史
3. **最终响应**：`MaskingContext.unmask()` 还原后保存到文件存储

### 8.5 MaskingContext 生命周期

```rust
// chat.rs agent_loop 中
let (task_id, stream, masking_ctx, cancel_rx) = gateway.stream_message(...).await?;

// masking_ctx 在整个 agent_loop 生命周期内复用
// 保证同一个人名在多轮 Tool Call 中始终映射到同一个占位符
```

`MaskingContext` 支持 `merge()` 方法，将多个脱敏会话的映射合并，确保跨 Tool Call 的一致性。

---

## 9. 多会话并发

### 9.1 设计约束

最多 **3 个会话**同时运行 Agent Loop。超过限制时 `set_busy()` 返回错误。

### 9.2 实现架构

```rust
// gateway.rs
const MAX_CONCURRENT_AGENTS: usize = 3;

struct LlmGateway {
    db: Arc<AppStorage>,
    active_tasks: Arc<Mutex<HashMap<String, ActiveTask>>>,
    //                        ^^^^^^^^
    //                    conversation_id → ActiveTask
}

struct ActiveTask {
    id: String,
    conversation_id: String,
    cancel: watch::Sender<bool>,  // 取消信号
    started_at: Instant,
}
```

### 9.3 AgentGuard RAII 守卫

`chat.rs` 中为每个 Agent Loop 创建 `AgentGuard`，在完成/错误/panic 时自动清理：

```
send_message()
    │
    ├─ gateway.set_busy(conversation_id)    ← 占槽位
    │
    ├─ AgentGuard::new(conversation_id)     ← 创建守卫
    │
    ├─ agent_loop(...)                      ← 执行分析
    │
    └─ AgentGuard::clear()                  ← 正常完成
         ├─ emit streaming:done              ← 通知前端
         ├─ db.remove_active_task()          ← 清理 DB
         └─ gateway.clear_task()             ← 释放槽位

    * 如果 agent_loop panic:
      AgentGuard::Drop
         ├─ db.remove_active_task()          ← 同步执行
         └─ tokio::spawn → gateway.clear_task() ← 异步清理
```

### 9.4 事件隔离

所有 Tauri 事件均包含 `conversationId` 字段，前端按会话隔离处理：

```
streaming:delta    { conversationId, content }
streaming:done     { conversationId }
streaming:error    { conversationId, error }
tool:executing     { conversationId, toolName, toolId }
tool:completed     { conversationId, toolId }
agent:idle         { conversationId }
```

前端 `chatStore` 使用 `streamStates: Record<string, ConversationStreamState>` 按会话隔离流式状态，同时通过 `isStreaming` / `streamingContent` 遗留字段保持向后兼容（从当前活跃会话的 streamState 派生）。

---

## 10. 模型路由

### 10.1 TaskType 分类

`router.rs` 将请求分为不同任务类型：

| TaskType | 说明 | 约束 |
|----------|------|------|
| `Analysis` | 分析任务（Step 0~5） | 强制默认模型 + `use_tools: true` |
| `Reasoning` | 推理任务 | DeepSeek R1 可用 |
| `Chat` | 日常对话 | 用户选择的模型 |

### 10.2 关键约束

```
分析任务 → 必须使用默认模型 + 必须启用 Tool Use
    │
    └─ 原因: 5 步分析流程依赖 Tool Call (Python/文件解析/报告生成)
             DeepSeek R1 不支持 Tool Use → 仅限推理任务
```

### 10.3 支持的 Provider

| Provider | 模型 | Tool Use | 备注 |
|----------|------|----------|------|
| DeepSeek | V3 | ✅ | 默认模型 |
| DeepSeek | R1 | ❌ | 仅推理 |
| OpenAI | GPT-4 等 | ✅ | |
| Claude | Sonnet 等 | ✅ | |
| Volcano | 火山引擎模型 | ✅ | |
| Qwen | Qwen-Plus | ✅ | |
| Custom | 用户自定义 | ✅ | OpenAI-compatible API（Ollama、LM Studio 等） |

路由函数根据 Provider 名称 dispatch 到对应的流式/非流式实现。

---

## 11. 流式通信协议

### 11.1 事件清单

| 事件名 | 触发时机 | 载荷 | 发射者 |
|--------|----------|------|--------|
| `streaming:delta` | LLM 输出 token | `{ conversationId, content }` | `agent_loop` |
| `streaming:done` | Agent Loop 完成 | `{ conversationId }` | **仅 `AgentGuard::clear()`** |
| `streaming:error` | 请求失败 | `{ conversationId, error }` | `agent_loop` |
| `streaming:step-reset` | 步骤切换 | `{ conversationId }` | `chat.rs` |
| `tool:executing` | 开始执行工具 | `{ conversationId, toolName, toolId }` | `agent_loop` |
| `tool:completed` | 工具执行完成 | `{ conversationId, toolId }` | `agent_loop` |
| `agent:idle` | Agent 空闲 | `{ conversationId }` | `finish_agent` |
| `message:updated` | 消息保存成功 | `{ conversationId, messageId }` | `finish_agent` |
| `analysis:step-changed` | 分析步骤变更 | `{ conversationId, step, status }` | `update_progress` |

### 11.2 单点发射原则

`streaming:done` **仅由 `AgentGuard::clear()` 发射**，`finish_agent()` 不再发射。

### 11.3 步骤切换事件

`streaming:step-reset` 在 auto-advance 时清空 `streamingContent` + `toolExecutions` 但保持 `isStreaming=true`：

```
Step 1 完成 → 用户确认 → AdvanceStep(Step 2)
    │
    ├─ emit streaming:step-reset     ← 清空内容，保持流式状态
    │   └─ 前端 resetConversationStreamContent()
    │       · streamingContent = ""
    │       · toolExecutions = {}
    │       · isStreaming = true (不变)
    │
    └─ 启动 Step 2 的 agent_loop
        └─ 新的 streaming:delta 事件开始到达
```

### 11.4 流式消费机制

`agent_loop` 用 `tokio::select!` 三分支消费流：

```rust
tokio::select! {
    // 分支 1: 取消信号
    _ = cancel_rx.changed() => {
        // 即时终止
        break;
    }
    // 分支 2: chunk 超时
    _ = tokio::time::sleep(Duration::from_secs(CHUNK_TIMEOUT_SECS)) => {
        // 90 秒无数据 → 流被判定为 stalled → 终止
        break;
    }
    // 分支 3: 正常事件
    event = stream.next() => {
        // 处理 StreamEvent (Delta/ToolCallStart/ToolCallDelta/Stop/...)
    }
}
```

---

## 12. 崩溃恢复

### 12.1 run.lock 文件

```
send_message() 启动 Agent 时:
    会话目录/run.lock ← 写入当前 PID

AgentGuard::clear() 完成时:
    会话目录/run.lock ← 删除
```

### 12.2 启动时孤儿检测

`lib.rs` 应用启动时扫描所有会话目录：

```
应用启动
    │
    ▼ 扫描 conversations/*/run.lock
    │
    ├─ 文件存在？
    │    │
    │    ├─ 读取 PID → 进程是否存在？
    │    │    │
    │    │    ├─ 存在 → 正常（另一个实例在运行，理论上不应该发生）
    │    │    │
    │    │    └─ 不存在 → 孤儿锁！
    │    │         │
    │    │         ├─ 1. 读取 analysis.json
    │    │         ├─ 2. 重置 "in_progress" 步骤状态为 "paused"
    │    │         └─ 3. 删除 run.lock 文件
    │    │
    │    └─ 文件为空/读取失败 → 视为孤儿，执行清理
    │
    └─ 文件不存在 → 跳过
```

### 12.3 步骤状态恢复

孤儿检测后，`analysis.json` 中 `status: "in_progress"` 被重置为 `status: "paused"`。

用户下次发送消息时：
```
next_action()
    └─ route_analysis_step()
         └─ status=Paused → ResumeStep(StepConfig)
              └─ agent_loop 从暂停点恢复执行
```

---

## 13. Python 沙箱

### 13.1 安全边界

**三层防御**：

1. **静态检查**（`sandbox::validate_code()`）：模式匹配扫描禁止模块/函数
2. **运行时 Import Hook**（Preamble 注入）：劫持 `builtins.__import__`，在 Python 进程内实时拦截动态导入
3. **路径转义**（`parser.rs`）：所有文件路径参数使用 `py_escape()` 防注入

**禁止的模块**（9 个）：
```
subprocess, socket, http, urllib, requests, shutil, ctypes, importlib, code
```

**禁止的函数调用**（13 个）：
```
exec(), eval(), compile(), __import__(),
os.system(), os.popen(), os.exec*(), os.spawn*(),
os.kill(), os.remove(), os.rmdir(), os.unlink(), shutil.rmtree()
```

**验证方式**：`sandbox::validate_code()` 对 LLM 生成的代码进行模式匹配检查，发现禁止模块/函数即拒绝执行。运行时 import hook 作为第二道防线，捕获 `importlib.import_module()` 等绕过静态检查的动态导入。

### 13.2 执行约束

| 约束 | 值 | 说明 |
|------|----|------|
| 超时 | 30 秒 | 防止死循环/长时间计算 |
| 输出上限 | 1 MB | stdout/stderr `take()` 硬限制，防止 OOM |
| 内存限制 | 512 MB | 建议值 |
| 递归深度 | 2000 | preamble 中设置 |

### 13.3 Preamble 注入

每次执行 Python 代码前，`sandbox::preamble()` 自动注入：

**Part 1（动态）**：
```python
# -*- coding: utf-8 -*-          # 源文件编码声明
import sys, os, builtins
sys.stdout.reconfigure(encoding='utf-8')  # Windows GBK 兼容
sys.setrecursionlimit(2000)
os.chdir("{workspace_path}")

# Runtime import hook — intercepts forbidden modules at import time
_original_import = builtins.__import__
_BLOCKED = {'subprocess', 'socket', 'http', 'urllib', 'requests', 'shutil', 'ctypes', 'importlib', 'code'}
def _safe_import(name, *args, **kwargs):
    top = name.split('.')[0]
    if top in _BLOCKED:
        raise ImportError(f"Module '{name}' is blocked by security policy")
    return _original_import(name, *args, **kwargs)
builtins.__import__ = _safe_import
```

**Part 2（静态，预加载工具函数）**：

| 函数 | 功能 |
|------|------|
| `_smart_read_csv(path, **kwargs)` | 编码自动探测（UTF-8 → GBK → GB18030 → latin-1） |
| `_smart_read_data(path)` | CSV/Excel 自动分发 + 编码探测 + 列名清洗 |
| `_smart_write_csv(df, path)` | UTF-8-BOM 编码写入（Windows Excel 兼容） |
| `_load_data(path=None)` | 自动查找第一个数据文件 |
| `_print_table(headers, rows, title)` | Markdown 表格格式化输出 |
| `_export_detail(df, filename, title, preview_rows)` | DataFrame 导出 Excel + 预览 + `__GENERATED_FILE__` 标记 |
| `_find_data_file(pattern)` | Glob 查找数据文件 |

### 13.4 Windows GBK 编码兼容

五重保障：
1. Python 源文件头部 `# -*- coding: utf-8 -*-` 声明
2. Python 进程环境变量 `PYTHONIOENCODING=utf-8`
3. Python 进程环境变量 `PYTHONUTF8=1`（PEP 540 UTF-8 模式）
4. Preamble 中 `sys.stdout.reconfigure(encoding='utf-8')`
5. `_smart_read_csv` / `_smart_read_data` 编码自动探测（UTF-8 → GBK → GB18030 → latin-1）

### 13.5 Python 运行时打包

使用 `python-build-standalone`（Astral 维护）将完整 Python 3.12 + pip 依赖打包进安装包：

```
resolve_python_path()
    │
    ├─ 优先: {resource_dir}/python-runtime/bin/python3  (macOS)
    │         python-runtime/python.exe                  (Windows)
    │
    └─ Fallback: 系统 python3 (开发模式)
```

打包模式下设置 `PYTHONHOME` 并清除 `PYTHONPATH`，隔离系统 Python 环境。

---

## 14. 搜索引擎

### 14.1 搜索策略

```
用户请求联网搜索
    │
    ▼ Bing（免费，HTML 解析，默认优先）
    │
    ├─ 成功且有结果 → 返回结果
    │
    └─ 失败或空结果
         │
         ▼ Tavily API Key 已配置？
         │
         ├─ 是 → Tavily AI 搜索（付费，高质量增强）
         │         │
         │         ├─ 成功 → 返回结果
         │         └─ 失败 → 返回 ToolError（is_error=true）
         │
         └─ 否 → 返回 ToolError（is_error=true）
```

> 搜索全部失败时返回 `Err()`（`is_error=true`），LLM 会看到工具执行失败，
> 不会误将错误信息当作搜索结果使用。

### 14.2 缓存机制

搜索结果缓存在 `{base_dir}/shared/cache/{hash}.json`，TTL 7 天。相同查询在缓存有效期内直接返回，避免重复请求。

---

## 15. 安全加固与资源管理

### 15.1 路径安全

| 防护点 | 机制 | 位置 |
|--------|------|------|
| Python 代码中的文件路径 | `py_escape()` 转义单引号和反斜杠 | `parser.rs` |
| `__GENERATED_FILE__` 注册 | `canonicalize()` + `starts_with(workspace)` 阻止路径穿越 | `tool_executor.rs` |
| 文件上传 | 200 MB 大小限制 + DB 失败时回滚物理文件 | `file.rs` |

### 15.2 资源防护

| 资源 | 防护机制 | 位置 |
|------|----------|------|
| Python stdout/stderr | `take(max_output_bytes + 1KB)` 硬限制 | `runner.rs` |
| 工具结果上下文 | 8 KB/条截断，防 LLM 上下文膨胀 | `chat.rs` |
| Tavily HTTP 请求 | 30s 超时 | `tavily.rs` |
| 临时 Python 文件 | 启动时清理 `code_*.py` 残留 | `lib.rs` |
| Markdown 渲染缓存 | 200 条 LRU-like 限制 | `markdown.ts` |

### 15.3 Agent Loop 可靠性

- **事件去重**：`streaming:done` 仅由 `AgentGuard::clear()` 发出（RAII 单点），错误路径不再重复发送
- **工具间取消检查**：每个工具执行完成后检查 `cancel_rx`，避免等待所有排队工具完成
- **文件删除完整性**：`delete_file` 同时清理物理文件和数据库记录

---

## 附录 A: 关键代码路径速查

| 场景 | 代码路径 |
|------|----------|
| 用户发消息入口 | `commands/chat.rs:send_message` |
| Skill 激活检测 | `plugin/registry.rs:SkillRegistry::detect_activation` |
| 步骤动作路由 | `plugin/skill_trait.rs:Skill::on_step_complete` |
| 确认词匹配 | `llm/orchestrator.rs:is_confirmation` |
| Agent Loop 主循环 | `commands/chat.rs:agent_loop` |
| LLM 流式请求 | `llm/gateway.rs:stream_message` |
| PII 脱敏 | `llm/masking.rs:mask_text` / `unmask` |
| 模型路由 | `llm/router.rs` |
| 系统提示词组合 | `llm/prompts.rs:get_system_prompt`（legacy），`plugin/declarative_skill.rs`（声明式 Skill） |
| 步骤检查点提取 | `llm/checkpoint.rs:checkpoint_extract` |
| Extract Prompt 加载 | `plugin/declarative_skill.rs:extract_prompt`（从插件 `prompts/extract/` 目录加载） |
| 步骤显示名称 | `StepConfig.step_display_names`（从 `Skill::workflow()` 填充） |
| 工具注册与执行 | `plugin/registry.rs:ToolRegistry` |
| 工具 Schema 过滤 | `plugin/registry.rs:get_schemas_filtered` |
| 内置工具实现 | `plugin/builtin/tools/` |
| 内置 Skill 实现 | `plugin/builtin/skills/` |
| Python 代码执行 | `python/runner.rs:execute` |
| 沙箱验证 | `python/sandbox.rs:validate_code` |
| Preamble 生成 | `python/sandbox.rs:preamble` |
| HTML 报告生成 | `plugin/builtin/tools/generate_report.rs` |
| 崩溃恢复扫描 | `lib.rs`（启动时） |
| Agent 守卫清理 | `commands/chat.rs:AgentGuard::clear` / `Drop` |
| 路径穿越校验 | `llm/tool_executor.rs:handle_execute_python` (`__GENERATED_FILE__`) |
| 运行时 Import Hook | `python/sandbox.rs:preamble` (`_safe_import`) |
| 临时文件清理 | `lib.rs:cleanup_temp_dir` |
| 搜索（Bing） | `search/bing.rs` |
| 搜索（Tavily） | `search/tavily.rs` |

## 附录 B: 配置常量速查

| 常量 | 值 | 位置 | 说明 |
|------|----|------|------|
| `MAX_CONCURRENT_AGENTS` | 3 | `gateway.rs` | 最大并发会话数 |
| `MAX_TOOL_ITERATIONS` | 10 | `chat.rs` | 日常模式最大迭代 |
| `AGENT_TIMEOUT_SECS` | 900 (15min) | `chat.rs` | Agent Loop 总超时 |
| `CHUNK_TIMEOUT_SECS` | 90 | `chat.rs` | 单 chunk 超时 |
| Step 0 max_iterations | 5 | `orchestrator.rs` | 方向确认迭代上限 |
| Step 1~3 max_iterations | 15 | `orchestrator.rs` | 数据处理步骤 |
| Step 4 max_iterations | 20 | `orchestrator.rs` | 诊断步骤（最复杂） |
| Step 5 max_iterations | 15 | `orchestrator.rs` | 报告生成步骤 |
| 日常 Token 预算 | 4096 | 设计约束 | 轻量对话 |
| 分析 Token 预算 | 8192 | 设计约束 | 每步独立 |
| Python 超时 | 30s | `sandbox.rs` | 单次执行 |
| Python 输出上限 | 1MB | `sandbox.rs` | stdout 截断 |
| 搜索缓存 TTL | 7 天 | `cache.rs` | |
| 确认词长度截断 | 20 字符 | `orchestrator.rs` | 防止长消息误判 |
| 消息分片阈值 | 100 条/片 | `file_store/messages.rs` | JSONL 分片 |
| 审计日志分片 | 2MB/片 | `file_store/audit.rs` | |
| 记忆分片 | 1MB/片 | `file_store/notes.rs` | |
| `HTTP 连接超时` | 30s | Provider `build_http_client()` | |
| `MAX_TOOL_RESULT_CHARS` | 8000 | `chat.rs` | 工具结果截断，防上下文膨胀 |
| `MAX_UPLOAD_SIZE` | 200 MB | `file.rs` | 上传文件大小限制 |
| `MAX_CACHE_ENTRIES` | 200 | `markdown.ts` | Markdown→HTML 缓存条目上限 |
| `MAX_HISTORY_MESSAGES` | 30 | `chat.rs` | 滑动窗口历史消息数 |
