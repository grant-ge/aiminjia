# 上下文压缩评测（LongMemEval）— 设计文档

> **状态**：设计稿 · 待用户 review
> **日期**：2026-06-04
> **作者**：Claude（基于与项目 owner 的头脑风暴）
> **关联工件**：
> - 压缩编排：`src-tauri/src/runtime/chat/preprocess.rs::prepare_messages_for_llm`
> - 压缩算法：`src-tauri/src/runtime/chat/compaction.rs`（`microcompact` / `compact_messages_via_llm` / `should_auto_compact`）
> - 摘要 prompt：`src-tauri/src/llm/compact_summary_client.rs::COMPACT_SYSTEM_PROMPT`
> - 阈值公式：`src-tauri/src/llm/context_decay.rs::effective_auto_compact_threshold`
> - 网关：`src-tauri/src/llm/gateway.rs::LlmGateway::send_message`
> - 装配参考：`src-tauri/src/lib.rs`（startup）、`src-tauri/tests/conversation_runtime_service_test.rs`（headless 构造）
> - 数据集：`xiaowu0162/longmemeval-cleaned`（HuggingFace），本机 `<github>/LongMemEval/data/`
> - 评测脚本参考：`<github>/LongMemEval/src/evaluation/evaluate_qa.py`

---

## 1. 背景与目标

AIjia 的 agent 在长对话 / 长任务中会自动进行上下文压缩。压缩是**有损**操作：把历史消息改写成更短的形式喂给模型。我们需要量化回答一个核心问题：

> **压缩之后，agent 该记住的关键信息丢了多少？**

LongMemEval（ICLR 2025）是测「聊天助手长期记忆」的标准 benchmark：给一段带时间戳的多 session 对话历史，问一个只有读过历史才能答对的问题，按 5 类记忆能力打分。它本身手段中立——通过「压缩前 vs 压缩后」的 A/B 对照，它能精确量化压缩对记忆的损伤。

目标：

- 用真实生产路径（lotus 网关 + 本地 JWT + 生产同款摘要 prompt）跑端到端评测，不是离线模拟。
- 输出「不压缩（天花板）」与「压缩后」两套 6 类能力正确率，差值即压缩损伤。
- 重点暴露 `knowledge-update`（知识更新被旧信息覆盖）和 `temporal-reasoning`（时序）两类——个人助手最致命、摘要最容易丢的能力。
- 评测代码不污染生产代码，仅新增一个 `#[ignore]` 集成测试入口。

---

## 2. 非目标

- 不评测模型权重压缩（量化 / 剪枝），只评测**运行时上下文压缩**。
- 不接入 AppWorld / OfficeBench / ACON 等工具型 benchmark（那是 `microcompact` 死板层的靶子，列为后续）。
- 不改动任何生产压缩逻辑；本期只做「测量」，不做「优化」。
- 不在 CI 常规流水线跑（需真实联网 + 真实 `.renlijia` 登录态 + 真实计费）。
- 不依赖 Python / OpenAI key：判分逻辑移植进 Rust，统一走 lotus 网关。

---

## 3. 被测对象：当前压缩的真实形态

### 3.1 五层流水线（`prepare_messages_for_llm`）

按执行顺序：

| # | 阶段 | 位置 | 性质 | 触发 |
|---|---|---|---|---|
| 1 | strip_images | preprocess.rs | 确定性 | 每轮 |
| 2 | tool_result_budget | preprocess.rs | 确定性 | 聚合预算 64K 字符 |
| 3 | **microcompact** | compaction.rs:140 | 确定性 | >120K 字符，抹旧 tool result |
| 4 | collapse_tool_results | preprocess.rs | 确定性 | 折叠长 tool result |
| 5 | **auto_compact 摘要** | compaction.rs:463 + compact_summary_client.rs | **LLM 有损** | 字符数 ≥ 动态阈值 |

整条流水线已封装成一个函数，且把「生成摘要」作为 `summary_fn` 闭包参数注入——这是天然的测试接缝，使评测可以在 Tauri app 之外驱动真实压缩。

### 3.2 本期主角：第 5 层 LLM 摘要

第 1–4 层是确定性的，且只动 tool 输出；**第 5 层 LLM 摘要是唯一会改写 / 丢弃语义的环节**，是 agent「忘事」的真正来源。对话型 benchmark（LongMemEval）正好测它：历史被摘要压掉后，事实还在不在。

> `microcompact`（第 3 层）只动 tool result，对话数据里几乎没有 tool result，用对话 benchmark 测它会得到「虚假满分」。它的正确靶子是工具型任务，列入 B 期之后的后续工作。

### 3.3 阈值真相（关键约束）

auto-compact 阈值（`context_decay.rs:108`）：

```
阈值(token) = context_window − 20000(摘要预留) − 13000(buffer)
阈值(char)  = 上式 × 4
```

- 生产用 Claude（200k 窗口）→ 阈值 ≈ **167k token** 才自然触发。
- `longmemeval_s` 每条仅 ~115k token → **在 200k 窗口下装得下，不会自然触发**。
- 要在 s 上测压缩，必须**强制触发**（`PreprocessTrigger::ManualCompact`）或调小 `context_window`。

这条直接决定了 A / B 两期的触发策略（见 §4）。

---

## 4. 测试集与触发策略

### 4.1 三个数据集的区别

| 数据集 | 每条历史构成 | 总量 | 与压缩的关系 |
|---|---|---|---|
| oracle | 仅证据 session，无干扰 | ~3 session | 太短，无压缩意义，仅用于链路冒烟 |
| **longmemeval_s** | 证据埋在 ~40 个干扰 session | ~115k token | A 期主集：强制触发，测摘要在干扰中的保真度 |
| longmemeval_m | 证据埋在 ~500 个干扰 session | 远超 200k | B 期主集：自然触发，真实溢出场景 |

本机已下载：`longmemeval_oracle.json`（14.7MB）、`longmemeval_s_cleaned.json`（264MB）。m 待 B 期下载。

### 4.2 A 期：longmemeval_s + 强制触发（先做）

- 触发：`ManualCompact`，保证摘要在每条上都运行。
- 对照：
  - **A 组（天花板）**：全量 115k 历史（能装进 200k）→ 答题 → `hyp_full`
  - **B 组（压缩）**：`prepare_messages_for_llm` 真实压缩 → 答题 → `hyp_compressed`
- 解读：两组 6 类正确率之差 = 压缩损伤。被测行为（压 40 个 session）100% 真实，仅触发动作是手动。

### 4.3 B 期：longmemeval_m + 自然触发（A 跑通后做）

- 触发：`Normal`，500 session 远超 200k，**真实自然触发**。
- 对照变化：全量历史**装不进上下文**，没有「不压缩」对照。改为：
  - 压缩后答题 vs `oracle` 上限（同问题在 oracle 数据上的正确率，作为「信息齐全时的天花板」）。
- 价值：测真实「上下文溢出被迫压缩」场景的端到端表现。

---

## 5. 评测架构

### 5.1 入口形态

- 文件：`src-tauri/tests/longmemeval_eval.rs`
- 标记：`#[ignore]`（真实联网 + 真实 `.renlijia` + 计费，排除常规 CI）
- 运行：`cargo test --test longmemeval_eval -- --ignored --nocapture`
- 参数（环境变量）：
  - `LME_DATA`：数据集路径（默认指向 s）
  - `LME_LIMIT`：跑前 N 条（默认小值冒烟，设大跑全量）
  - `LME_PHASE`：`a` / `b`

### 5.2 真实网关重建（headless）

复刻 `lib.rs` startup 的最小装配链：

```
AiJiaHome::from_home()                       // ~/.renlijia
  → SecureStorage::new(crypto_dir)
  → GlobalConfigStore::new(global_dir)
  → AuthManager::new(global, secure, &home).restore().await   // 读本地 JWT
  → db = AppStorage::new(临时目录)             // 网关仅用其记账，临时即可
  → RuntimeRunRegistry::new()
  → LlmGateway::new_with_registry(db, registry).with_auth_manager(auth)
settings = AppSettings::default()            // select_route 恒走 lotus 云
```

- base URL 在 provider 写死；session_key 由网关经 `AuthManager::get_session_key()` 自动注入，前提是 `.with_auth_manager(...)`。
- **前置条件**：`restore()` 成功 ≠ 一定打通，首请求可能 token 过期。**跑前需先在 app 内登录一次确保登录态新鲜**。

### 5.3 数据流（每条样本）

```
haystack_sessions ─┬─► 结构化 messages [{role, content}]（保留 session 时间戳）
                   │
   ┌───────────────┴───────────────┐
   │ A 组：构建真实多轮对话           │ B 组：prepare_messages_for_llm(messages,
   │  （复用 merge_consecutive_same_  │        summary_fn = 生产同款 COMPACT_SYSTEM_PROMPT
   │    role 处理交替约束）          │        → gateway.send_message）
   │  + question 作为末轮 user       │   → 压缩后(boundary+摘要+tail) 还原成多轮对话
   │                                 │   + question
   └───────────────┬───────────────┘
                   ▼
        gateway.send_message(默认 chat 模型) → hypothesis
                   ▼
        判分（§5.4）→ 6 类能力 yes/no
```

- 答题用**构建真实多轮对话**（不拍平成单条消息），更贴近线上；用 `provider_merge::merge_consecutive_same_role` 处理 Anthropic 的 user/assistant 交替约束。
- 摘要走生产同一段 `COMPACT_SYSTEM_PROMPT`，保证行为 = 线上。

### 5.4 判分（移植进 Rust，走 lotus）

- 把 `evaluate_qa.py::get_anscheck_prompt` 的 6 类裁判 prompt（含 abstention、temporal off-by-one 容错、knowledge-update「保留旧值也算对」等特例）原样移植成 Rust 常量。
- 裁判调用走同一个 `gateway.send_message`，输出 `yes`/`no`。
- 不需要 Python、不需要 OpenAI key。

### 5.5 答题 / 摘要 / 判分模型

统一走 `AppSettings::default()` 的默认 chat 模型，由 lotus 网关决定具体上游。

---

## 6. 指标定义

每条样本记录：

- `question_type`（6 类之一，`_abs` 为 abstention）
- `pre_chars`：压缩前全量字符数
- `post_chars`：压缩后字符数 → 压缩比 `1 − post/pre`
- `autoeval_full` / `autoeval_compressed`：两组判分 yes/no

汇总输出：

| 指标 | 含义 |
|---|---|
| 总正确率（full / compressed） | 端到端答对率 |
| 分 6 类正确率（full / compressed） | 定位丢在哪类能力 |
| **正确率差值（full − compressed）** | **压缩损伤（核心结论）** |
| 平均压缩比 | token 节省 |
| 损伤 / 压缩比 帕累托 | 「省了多少 token，掉了多少分」性价比 |

重点盯：`knowledge-update`、`temporal-reasoning` 两类的损伤。

---

## 7. 分期实施

| 期 | 内容 | 退出标准 |
|---|---|---|
| **A0** | 链路冒烟：`LME_LIMIT=3` 跑通网关重建 + 压缩 + 答题 + 判分，确认能拿到分 | 3 条端到端无报错，打印出分 |
| **A1** | longmemeval_s 全量（或大样本）强制触发 A/B | 输出 6 类损伤表 + 压缩比 |
| **B0** | 下载 longmemeval_m，改造对照为「压缩 vs oracle 上限」 | m 上自然触发跑通 |
| **B1** | longmemeval_m 大规模自然触发评测 | 输出真实溢出场景损伤 |
| （后续） | microcompact 死板层：接 SWE-bench / AppWorld 工具型任务 | 另立 spec |

---

## 8. 风险与对策

| 风险 | 对策 |
|---|---|
| `restore()` 后 token 过期，首请求 `get_session_key()` 失败 | 跑前在 app 内登录刷新；测试对 auth 错误给明确提示 |
| 全量 app_lib 编译重 / 慢 | 用 `--test longmemeval_eval --no-run` 先编译验证 |
| 需要的类型非 `pub` 不可达 | 已确认 `prepare_messages_for_llm` 及相关类型、`LlmGateway::send_message`、`AppSettings` 等均 `pub`（crate 名 `app_lib`） |
| 摘要 / 答题 / 判分共用一个模型，判分可能偏袒 | 必要时判分换更强模型；A0 先人工抽查若干条判分质量 |
| 计费成本 | 用户已确认不关心成本，优先真实性；仍用 `LME_LIMIT` 控制冒烟阶段 |
| 强制触发被质疑「不真实」 | 文档明确：A 期被测行为（压 40 session）真实，仅触发手动；B 期用 m 做完全自然触发 |

---

## 9. 待 owner 确认

1. 文档放置位置与命名是否符合规范（本文件路径）。
2. A 期先 `LME_LIMIT=3` 冒烟、再放大，是否同意。
3. B 期下载 longmemeval_m（更大）与「压缩 vs oracle 上限」的对照改造，是否认可。
4. 判分模型暂用默认 chat 模型，后续若判分质量不足再换强模型，是否接受。
