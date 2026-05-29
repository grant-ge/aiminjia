# 权限治理与 Ask/Remember 语义统一（Plan-U2）

> **For agentic workers:** REQUIRED SUB-SKILL: `superpowers:test-driven-development` — permission pipeline、前端交互、存储格式都要先锁回归。 REQUIRED SUB-SKILL: `superpowers:verification-before-completion` — 关闭任务前必须同时验证 Rust 与前端链路。

**Goal:** 把 lotus 当前 `Default/DontAsk + tool:scope` 的最小权限模型升级为本地多层规则 + 完整 Ask / remember / destination 语义，让主线程、MCP、subagent 共享同一权限控制面。

**Architecture:** 对标 `claude-code-best/docs/safety/permission-model.mdx` 的核心思想，但只做本地桌面版本：`session / workspace / user` 三层规则来源，`default / plan / dontAsk` 三种模式，先支持工具名 + scope + 最小路径/命令匹配，不引入云端托管策略与远程同步。

**Tech Stack:** Rust, Tauri v2, React/TypeScript, Zustand

**Worktree branch:** pzc

---

## 背景与现状

| 文件 | 现状 |
|---|---|
| `src-tauri/src/runtime/tools/permission.rs` | `PermissionMode` 只有 `Default` / `DontAsk`；unknown scope 只会 `Ask` 或 `Deny`，没有规则来源与 remember 目的地 |
| `src-tauri/src/runtime/store/permission_store.rs` | 只有 `session + persistent` 两层，key 仍是扁平 `tool:scope`；无法表达 workspace / user 分层 |
| `src/components/common/PermissionAskDialog.tsx` | UI 只有允许 / 拒绝 / 关闭三种动作，没有“仅本次 / 记住到工作区 / 记住到用户级”的差异 |
| `src/App.tsx` / `src/stores/streamingStore.ts` | 前端 pending ask payload 太薄，承载不了 remember / mode / destination |

### 当前缺口的实际影响

- 用户授权一次之后，不知道授权落在什么范围，也没法控制是“仅这次”还是“以后都别再问”。
- subagent、MCP、request-scoped tool 对同一个动作可能出现不同判定，体验割裂。
- `dontAsk` 目前更像临时 deny 开关，而不是完整 permission mode。

## 范围

- 纳入：
  - `session / workspace / user` 三层本地权限规则
  - `default / plan / dontAsk` 三种 mode
  - Ask payload 的 remember / destination / suggestions 扩展
  - Bash / 文件工具的最小命令/路径匹配
- 不纳入：
  - 企业托管策略
  - 远程同步、共享权限配置
  - 完整复刻 `claude-code-best` 的所有模式与策略来源

## 任务拆分

### U2-1：把权限数据模型从扁平 key 升级为规则集

- [ ] 新建 `PermissionRule` / `PermissionSource` / `PermissionMatch` 结构，至少支持 `tool_name`、`scope`、`path_glob`、`command_pattern`。
- [ ] `PermissionStore` 改成 `session / workspace / user` 三层读取与合并；workspace 规则落在本地工作区，user 规则落在应用数据目录。
- [ ] 旧的 `tool:scope -> PolicyDecision` 数据提供兼容迁移与 fallback 读取。

### U2-2：把 mode 语义补齐到控制面

- [ ] 把 `PermissionMode` 扩成 `default / plan / dontAsk`，并明确每种模式如何处理 `Ask`、写操作、未知 scope。
- [ ] `apply_permission_mode()` 不再只做 `dontAsk => deny` 单点变换，而是统一通过 mode 规则表收口。
- [ ] 主线程与 subagent 必须读取同一 mode 语义，不能各自兜底。

### U2-3：把 Ask payload 与前端交互升级成完整语义

- [ ] `PermissionAskRequired` 事件携带 `destination options`、`remember labels`、`mode context`，不再只传 message + suggestions。
- [ ] `PermissionAskDialog` 支持“仅本次允许 / 记住到工作区 / 记住到用户级 / 拒绝”的操作。
- [ ] `approve_permission_request` / `deny_permission_request` 增加 remember 与 destination 参数，并把结果正确写入对应规则层。

### U2-4：把匹配维度补到真实使用层

- [ ] Bash 工具支持最小命令模式匹配，而不是只看抽象 scope。
- [ ] 文件类工具支持路径匹配，不再把整个工作区写权限看成一个粗粒度开关。
- [ ] MCP 工具继续走统一 permission pipeline，不保留绕路 special case。

### U2-5：回归测试与 review 约束

- [ ] Rust 侧覆盖：mode 变换、规则优先级、workspace/user 合并、unknown scope Ask、Bash/路径匹配。
- [ ] 前端覆盖：不同 remember 选项的交互、pending ask 清理、mode 切换后的 UI 文案。
- [ ] 增加 review test，防止未来再把 remember 语义压回单个 `Allow/Deny/Cancel` 三按钮模型。

## 验收标准

- 用户可以明确选择“只允许这次”或“记住到某一层”。
- 同一条权限规则对主线程、MCP、subagent 的结果一致。
- `dontAsk`、`plan` 不再是零散特判，而是可回归测试的 mode 语义。
- 权限模型仍保持本地-only，不引入任何远程托管前提。
