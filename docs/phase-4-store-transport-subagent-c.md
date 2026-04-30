# 第 4 期：Store 领域拆分 + Transport 解耦 + SubAgent 阶段 C

> 目标：把持久化按领域拆分，把 Tauri 真正降级为 adapter，并完成 SubAgent 阶段 C（恢复 / worktree / team）
> 关键原则：本期是把前 1-3 期建立的 runtime 彻底从宿主中解耦，而不是再增加一层 façade

---

## 一、本期目标

完成以下四件事：

1. State Store 按领域正式拆分
2. Tauri transport 完全适配化，核心 runtime 不再感知宿主
3. 完成 SubAgent 阶段 C：resume / worktree / team 协作
4. 清理前 1-3 期遗留兼容层与旧入口

### 本期解决的挑战
- 完成 C7 阶段 C
- 兑现“ Tauri 只是 adapter ”不是口号
- 收尾并消除前期过渡层

---

## 二、核心设计

### 2.1 领域化 State Store

第 2 期的最小 store 只是桥接。本期正式拆为：

```text
src-tauri/src/runtime/store/
├── session_store.rs
├── run_store.rs
├── task_store.rs
├── tool_call_store.rs
├── agent_store.rs
├── settings_store.rs
├── memory_store.rs
└── audit_store.rs
```

要求：
- 每个 store 只承载一个领域
- Runtime 只依赖 store trait，不依赖 file 实现
- `storage/file_store/mod.rs` 不再作为一体化 DAO 被上层直接访问

### 2.2 Repository / Store 层次

拍板：
- Store：领域状态的持久化边界
- Repository trait：Runtime 访问接口
- file-based impl：当前默认底层实现

后续如果换 SQLite/远程存储，只替换 impl。

### 2.3 Transport 解耦

本期要把 Transport 层结构明确为：

```text
src-tauri/src/transport/
├── mod.rs
├── tauri_commands/
│   ├── chat.rs
│   ├── auth.rs
│   ├── file.rs
│   └── ...
├── tauri_event_adapter.rs
└── tauri_runtime_host.rs
```

要求：
- `runtime/` 下不再 import `tauri::*`
- 所有 command 只是 adapter
- Runtime 通过 trait 接收 EventSink / HostOps

### 2.4 SubAgent 阶段 C

在 A/B 基础上增加：
- resume：支持中断后恢复 child run / task
- worktree：为隔离执行预留工作目录上下文
- continue-message：可向已存在 child run 继续发送消息
- team：允许多个 agent invocation 组成协作图

注意：
- 这是能力期，不要求一开始就做成 claude-code-best 的全部生态
- 重点是底层模型可承载，不是 feature 一次做满

### 2.5 Team/Swarm 的范围

团队协作最小模型：
- `TeamId`
- `AgentInvocation` 之间的父子/同组关系
- 共享 TaskStore
- 消息转发桥接

不要求本期就实现完整 UI 管理面板。

---

## 三、新增文件（建议）

```text
src-tauri/src/runtime/store/
├── session_store.rs
├── agent_store.rs
├── settings_store.rs
├── memory_store.rs
└── audit_store.rs

src-tauri/src/transport/
├── mod.rs
├── tauri_runtime_host.rs
├── tauri_event_adapter.rs
└── tauri_commands/
    ├── chat.rs
    ├── auth.rs
    ├── file.rs
    ├── plugin.rs
    └── ...

src-tauri/src/runtime/agent/
├── resume.rs                # child run 恢复
├── worktree.rs              # worktree context
├── continue_message.rs      # 向已存在 child run 继续发消息
└── team.rs                  # team / swarm 最小模型
```

迁移涉及旧文件：

```text
src-tauri/src/storage/file_store/mod.rs
src-tauri/src/commands/*.rs
src-tauri/src/lib.rs
src-tauri/src/llm/sub_agent.rs   # 最终删除或仅留极薄兼容壳
```

---

## 四、迁移方式（文件级）

### 4.1 storage/file_store/mod.rs
迁移策略：
- 不再让上层直接调用一体化 DAO
- 将内部实现拆成多个领域 impl
- 最终保留一个组合根，用于初始化，不暴露给 runtime

### 4.2 commands/*.rs
迁移策略：
- 将 `src-tauri/src/commands/` 重组到 `transport/tauri_commands/`
- 所有 command 只做：输入 → 调 runtime → 输出
- 不再允许 command 内直接读写 store / emit 业务事件

### 4.3 lib.rs
角色收敛：
- 只做应用启动与依赖装配
- runtime、transport、infra 三类依赖在这里连线
- 不再出现业务流程逻辑

### 4.4 sub_agent.rs
第 4 期末：
- 要么删除
- 要么只保留极薄兼容 wrapper 并标记 deprecated

---

## 五、Compatibility Boundary

本期仍需保持：
- 用户级功能行为兼容
- 前端关键事件协议在迁移期间保持可工作
- 现有数据可迁移、可读取

本期允许做的 breaking internal change：
- 目录结构大改
- transport 层重排
- store 实现细分
- sub-agent 内部能力升级

如果需要调整前端协议，只能通过新增版本化 adapter，不能直接替换旧协议。

---

## 六、Kill List

本期末必须废掉：

1. `storage/file_store/mod.rs` 作为上层一体化 DAO 的角色
2. `src-tauri/src/commands/*.rs` 中的业务逻辑路径
3. runtime 内残余的任何 `tauri::*` 依赖
4. `sub_agent.rs` 的历史主实现
5. 第 1-2 期遗留的旧 Tool/PluginContext shim（若仍存在）

---

## 七、Truth Source

第 4 期拍板：

| 状态 | 真相源 |
|------|-------|
| session 生命周期 | `SessionStore` |
| run 生命周期 | `RunStore` |
| task 生命周期 | `TaskStore` |
| tool_call 生命周期 | `ToolCallStore` |
| agent invocation 生命周期 | `AgentStore` |
| settings | `SettingsStore` |
| memory | `MemoryStore` |
| audit / event log | `AuditStore` |

Transport / 前端只能消费这些状态的投影，不能再成为真相源。

---

## 八、Golden Trace 验收

### Trace J：恢复一个中断的 child run
要求：
- 能从 `RunStore / AgentStore / TaskStore` 重建状态
- continue-message 能继续推进 child run

### Trace K：worktree 子代理执行
要求：
- child run 具备独立 worktree context
- 输出与文件变更可追踪到 AgentInvocation

### Trace L：team/swarm 最小协作
要求：
- 主 run 拉起多个 agent invocation
- 共享 TaskStore
- 结果聚合到主 run

### Trace M：transport 解耦验证
要求：
- Runtime 单元测试不依赖 Tauri
- 可在无 Tauri AppHandle 的情况下跑核心 turn / task / agent 流程

---

## 九、Not Doing

本期明确不做：
- 不要求立刻替换成新的物理存储引擎
- 不要求一次做完所有 CLI/remote 入口
- 不要求补齐 claude-code-best 的全部平台能力
- 不要求完整团队协作 UI

本期关注的是“架构承载能力已形成”。

---

## 十、本期完成定义

第 4 期完成的标志：

1. Store 已按领域拆分
2. Runtime 已彻底摆脱 Tauri 类型依赖
3. Tauri command 全部降级为 adapter
4. 子代理支持 resume / worktree / continue-message / team 最小模型
5. 一体化 file_store DAO 不再被上层直接调用
6. 旧兼容层已清理到可控范围
7. 4 条 golden trace 回放通过

完成后，lotus-app 后端架构才算真正从“桌面宿主里的业务代码”升级为“可承载 agent/runtime/task 的核心系统”。
