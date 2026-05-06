# test-intents — 意图规格与验证

## 1. 这是什么

把"系统对用户在产品层面应该有什么变化"用自然语言写下来，再用机器或 agent 去验证它真的成立。

目录结构：

```
docs/test-intents/
├── README.md                    ← 本文，框架总览
├── context/                     ← 写规格的规范
│   ├── how-to-write-rules.md   ← 怎么写一条意图
│   ├── how-to-test.md           ← 怎么把意图翻成可执行测试
│   ├── capabilities.md          ← 现有测试基础设施清单
│   └── context.md               ← 业务规则速查
└── spec/
    └── tasks/<feature>/
        ├── rules.md             ← 该功能的意图列表
        └── test-progress.md     ← 执行记录（通过/失败/坑）
```

## 2. 为什么这么做

写代码的人和用代码的人看到的是两件事：

- 写代码的人看到模块、函数、参数、返回值
- 用代码的人看到操作、变化、可见的反应

只测前者，会出现"代码全绿但产品坏了"——每个模块单测都过，集成起来用户用不了。
只盯后者，会出现"用户看着对但内部状态错了"——前端展示正常，文件没写、队列没清、下次重启爆。

意图规格站在第二种视角写，但不止写"用户能看到的"——它**完整列出**这个产品事件应该引发的所有系统变化（外部可见 + 内部状态）。

一份意图规格独立于实现：

- 不规定用什么技术验证（单测 / 集成测试 / agent 跑都可以）
- 不规定模块怎么组织（重构后规格不变，验证脚本可能要重写）
- 描述的是"系统该如何变化"，不是"代码该长什么样"

## 3. 一条意图长什么样

意图描述一个产品事件，由三部分组成：

1. **故事**：用户做了什么 → 系统该有什么反应
2. **前提与操作**：把故事翻成可重放的 fixture（具体字符串、具体调用、所有参数）
3. **系统应有的变化**：故事跑完后所有应该发生的状态变化（外部可见 + 内部状态）

写作规范见 `context/how-to-write-rules.md`。

关键原则：

- **产品视角**——写完读一遍，产品经理能不能看懂？看懂就行
- **字面量 fixture**——`name = "薪资分析偏好箱线图"`，不是"某条记忆"
- **断言可直接翻成 assert_eq!**——精确到字段值、字符串、数量
- **变化要列全**——外部可见的（UI、文件）和内部的（队列、store、event）都列

## 4. 两种验证方式

同一份意图规格可以被两种方式验证。两种方式各有边界，互相补充，互不替代。

### 方式一：cargo test（机器回归）

- **谁做**：把 `rules.md` 翻成 `tests/intent_*_test.rs`
- **跑在**：CI、本地 `cargo test`
- **频次**：每次提交
- **形态**：根据意图涉及的范围，可能是 lib mod 单测、`tests/*.rs` 集成测试、跨模块端到端测试。不调真 LLM、不联网，用 `TempDir` 起真实文件系统沙箱
- **适合**：算法、契约、数据持久化、状态机、跨模块协作——只要不依赖外部 LLM/网络，就走这条
- **失败信号**：编译/断言失败，行号清晰、可 `git diff`

### 方式二：agent 跑（产品验收）

- **谁做**：agent 当用户用一遍真功能，自己读 storage / 读 EventBus / 读文件，对比 `rules.md` 里的预期变化
- **跑在**：本地 / staging
- **频次**：开发期、push 前、上线前手动触发
- **形态**：启动真后端（可能含真 LLM），agent 调真入口，读真持久化文件，输出"实际 vs 预期"自然语言报告
- **适合**：跨 UI + runtime + storage 的端到端产品故事；mock 不到的真实文件/事件序列；外部依赖（LLM、文件系统、并发时序）真出问题的场景
- **失败信号**：自然语言报告，定位到文件/字段层级

### 怎么选

| 意图涉及范围 | 推荐 |
|---|---|
| 单模块算法、纯函数 | cargo test（lib mod） |
| 跨模块协作、状态机、持久化 | cargo test（集成测试） |
| 跨层端到端（UI ↔ runtime ↔ 真 storage ↔ 真 LLM） | agent 跑 |
| mock 不到的真实并发/文件时序 | agent 跑 |
| 一条意图两条通道都跑 | 合理：cargo 防回归，agent 防产品语义漂移 |

不强制——一份意图规格可以只走一条，也可以两条都跑。

## 5. 不做什么

边界清楚，避免框架膨胀：

- **agent 跑不进 CI**——不可复现、调用慢、有成本，不当回归保护用
- **不替代 unit test**——内部纯函数 / 算法分支继续走 lib mod tests
- **不验实现细节**——不断言"某函数被调用几次"、"某内部枚举变体是什么"。意图描述的是"系统该如何变化"，不是"代码该怎么写"
- **不固定测试形态**——意图本身只描述故事和变化，形态由实现者根据范围决定（单测/集成/端到端/agent 跑）
- **不在 rules.md 里写代码**——`rules.md` 是规格，翻译成代码是 `tests/*.rs` 的事

## 6. 现有文件分工

- `context/how-to-write-rules.md` — 怎么写一条意图（产品视角、字面量 fixture、可翻译的断言）
- `context/how-to-test.md` — 怎么把意图翻成测试 / 怎么跑 / 漂移如何处理
- `context/capabilities.md` — 现有测试基础设施清单（TempDir、MockLlmExecutor、ProbeExecutor）
- `context/context.md` — 业务规则速查（settings 优先级、masking 链路、skill 加载语义等）
- `spec/tasks/<feature>/rules.md` — 该功能的意图列表与变化描述
- `spec/tasks/<feature>/test-progress.md` — 执行记录（通过/失败/坑）

继续做某个功能的意图测试：先读对应 `rules.md` + `test-progress.md`，再看 `context/` 四个文件，按 `how-to-test.md` 规范执行。

新建一个功能的 `rules.md`：先读 `context/how-to-write-rules.md`，再按产品视角逐条写。

## 7. 当前状态

已就位的意图规格（位于 `spec/tasks/`）：

- `masking-level`
- `memory-service`
- `memory-tool`
- `memory-turn-injection`
- `permission-ask-flow`
- `permission-pipeline`
- `permission-store`
- `session-runtime`
- `skill-loading`
- `subagent-execution`
- `subagent-lifecycle`
- `tauri-event-adapter`

验证方式落地情况：

- **cargo test**：已在用，多数 task 有对应 `tests/*.rs`
- **agent 跑**：规格框架就位，实施方案待定
