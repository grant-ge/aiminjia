# Mandatory Checkpoint Extraction 设计文档

> 日期：2026-03-01
> 状态：Implemented
> 目标：解决步骤间上下文传递对 LLM 自觉调用 save_analysis_note 的依赖问题

## 问题分析

当前系统通过两层机制保留步骤间上下文：
1. **save_analysis_note**（LLM 主动调用）— 质量高但依赖 LLM 合规性
2. **auto_capture_step_context**（系统自动捕获）— 始终执行但质量有限（机械截取，无语义理解）

核心问题：如果 LLM 在分析过程中忘记调用 save_analysis_note，auto_capture 只能机械截取最后几条消息和工具输出，丢失大量关键信息。

### 薄弱环节

| 问题 | 严重程度 | 说明 |
|------|---------|------|
| LLM 不调用 save_analysis_note | 🔴 致命 | auto_capture 质量不可控，只抓最后 2 条消息 |
| 无"不保存就不放行"机制 | 🔴 致命 | auto_capture 失败后步骤照样推进，上下文可能完全丢失 |
| Context 随步骤推进退化 | 🟡 严重 | 到 Step 5 时 Step 1 被压缩到 2000 字符 |
| save_analysis_note key 命名无约束 | 🟡 严重 | LLM 可能用任意 key，压缩策略失效 |

## 方案：独立 LLM 提取调用

把"保存分析结论"从 LLM 自觉行为变成系统强制行为。

### 架构

```
step N agent_loop 结束
  │
  ├── ① checkpoint_extract()     ← 新增：非流式 LLM 调用，提取结构化 JSON
  │     ├── 成功 → 保存 step{N}_checkpoint
  │     └── 失败 → log::warn，降级到 auto_capture
  │
  ├── ② auto_capture_step_context()  ← 保留：始终执行，兜底
  │
  ├── ③ advance_step() 标记完成
  ├── ④ 清空消息历史
  └── ⑤ 启动 step N+1（system prompt 注入所有 checkpoint/notes）
```

### 三层保障体系

```
Layer 1: Checkpoint Extraction（系统强制，最高优先级）
  ↓ 失败时
Layer 2: save_analysis_note（LLM 主动保存，补充信息）
  ↓ 都没有时
Layer 3: auto_capture_step_context（机械兜底）
```

## 数据结构

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepCheckpoint {
    pub step: u32,
    pub summary: String,              // 必填：200-500 字步骤总结
    pub key_findings: Vec<String>,    // 必填：关键发现列表（至少 1 条）
    pub data_artifacts: Option<String>, // 可选：数据产出（表格、统计数字等）
    pub decisions: Option<Vec<String>>, // 可选：决策及原因
    pub next_step_input: String,      // 必填：下一步需要的输入摘要
}
```

### 存储

- Key: `note:{conversation_id}:step{N}_checkpoint`
- Value: JSON 序列化字符串
- Source: `"checkpoint_extract"`
- 存储位置：enterprise memory JSONL（与现有 notes 相同）

## 提取流程

### 函数签名

```rust
async fn checkpoint_extract(
    gateway: &LlmGateway,
    settings: &AppSettings,
    conversation_id: &str,
    step_num: u32,
    messages: &[ChatMessage],
    db: &AppStorage,
) -> Option<StepCheckpoint>
```

### 执行过程

1. 加载通用提取基座 prompt + 步骤专属提取 prompt
2. 从消息历史中过滤出 assistant + tool 角色消息
3. 调用 `gateway.send_message()`（非流式，无工具，30 秒超时）
4. 解析 JSON 输出（支持 ```json 包裹和裸 JSON）
5. 验证必填字段（summary 非空、key_findings 非空、next_step_input 非空）
6. 保存到 enterprise memory
7. 失败任何步骤 → 返回 None，降级到 auto_capture

### 超时控制

```rust
tokio::time::timeout(Duration::from_secs(30), do_checkpoint_extract(...))
```

## 注入策略

### 优先级

```
step{N}_checkpoint 存在 → 用它（结构化，最完整）
step{N}_summary 存在   → 用它（LLM 主动保存的）
step{N}_auto_context 存在 → 用它（机械兜底）
```

### 字段级衰减规则

| 字段 | 当前步骤-1（近） | 更早步骤（远） |
|------|-----------------|---------------|
| summary | 完整 | 完整（永不截断） |
| key_findings | 完整 | 完整（永不截断） |
| next_step_input | 完整 | 完整（永不截断） |
| data_artifacts | 完整 | 截断到 2000 字符 |
| decisions | 完整 | 保留前 3 条 |

核心改变：summary/key_findings/next_step_input 永不截断，只对大体量的 data_artifacts 做距离衰减。

### 注入格式

```markdown
[前序分析记录]

## 第 1 步：数据清洗 (checkpoint)
### 总结
{summary}
### 关键发现
- {finding_1}
- {finding_2}
### 传递给下一步的信息
{next_step_input}
### 数据产出
{data_artifacts}
### 决策
- {decision_1}
```

## 提取 Prompt 设计

### 文件组织

```
code/src-tauri/prompts/extract/
├── base_extract.md           ← 通用提取指令
├── extract_step0.md          ← Step 0 专属提取要求
├── extract_step1.md
├── extract_step2.md
├── extract_step3.md
├── extract_step4.md
└── extract_step5.md
```

### 通用提取基座

指令 LLM 从对话记录中提取 JSON 格式的结构化检查点，包含格式说明和规则（具体数据、不要模糊表述、只输出 JSON）。

### 步骤专属 prompt

每个步骤定义该步应提取的具体内容，例如 Step 1 要求 summary 包含分析人数/排除人数、key_findings 包含字段映射和薪酬结构等。

## 改动清单

### 新增文件

| 文件 | 内容 | 预估行数 |
|------|------|---------|
| `src-tauri/src/llm/checkpoint.rs` | StepCheckpoint + checkpoint_extract() + JSON 解析 + prompt 加载 | ~150 |
| `src-tauri/prompts/extract/base_extract.md` | 通用提取指令 | ~25 |
| `src-tauri/prompts/extract/extract_step0.md` ~ `extract_step5.md` | 步骤专属提取 prompt | 每个 ~15 |

### 修改文件

| 文件 | 改动 | 预估行数 |
|------|------|---------|
| `src-tauri/src/commands/chat.rs` | 步骤切换处插入 checkpoint_extract() 调用 | ~15 |
| `src-tauri/src/commands/chat.rs` | analysis_notes_context() 优先使用 checkpoint + 结构化格式化 | ~60 |
| `src-tauri/src/llm/mod.rs` | 加 `pub mod checkpoint;` | 1 |

### 不改动

- gateway.rs（复用 send_message）
- orchestrator.rs（步骤流转不变）
- tool_executor.rs（save_analysis_note 保留）
- tools.rs（工具定义不变）
- notes.rs（存储 API 不变）
- 前端（完全不变）
- comp_analysis.rs（Skill 逻辑不变）

## 向后兼容

- 现有 save_analysis_note + auto_capture 全部保留
- checkpoint 是新增的第三层，不影响旧数据
- 没有 checkpoint 的旧会话照常工作（注入时自动降级）
