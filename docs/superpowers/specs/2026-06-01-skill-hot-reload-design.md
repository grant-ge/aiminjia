# 技能创建后无需重启即可使用 — 设计

> **日期**: 2026-06-01  
> **范围**: AIjia 桌面端 `SkillRegistry` 热刷新链路  
> **状态**: 方案待落地（spec → plan → impl）

## 问题陈述

用户在对话里通过 `小程` 数字员工 + `skill-creator` 技能创建一个新技能后，技能中心看不见、AI 也无法在 catalog 里看到、Skill 工具加载会 miss——必须重启 app 才能生效。

### 根因（已验证）

`SkillRegistry` 是 Tauri 进程内的 `Arc<Mutex<...>>`，只在三个时机刷新：

1. App 启动时 `lib.rs::run` 跑一次 `refresh_skill_registry`
2. `install_custom_skill` IPC（drag-drop 或 SkillCenter 的「+导入技能」按钮）末尾显式 refresh
3. `import_skill_package` IPC（zip 导入）末尾显式 reload

`skill-creator` 技能引导 LLM 走 `Bash(lotus_skill.py install)` 流程——这是个 Python 脚本，本质就是 `cp -r` 到 `~/.renlijia/users/{scope}/skills/<id>/`。Python 进程**无法触达 Tauri 进程内的 SkillRegistry**，所以磁盘已经有 SKILL.md，内存 registry 完全不知道。

**这跟之前清理 `skill_smith` 和 `templates_bootstrap` 的工作无关**——是 skill-creator 这条路径固有的设计缺口，2026-06 之前就存在。

### 真正的需求

> **技能创建后, AI 小家可以自动加载技能, 技能中心能看到只是他的表象。**

具体三个约束：

1. **同 turn 装+用**：用户「建一个 X 技能并马上用」，AI 同 turn 内调 `Bash(install)` → `Skill('X')`，必须成功
2. **跨 turn 自主决策**：用户下一 prompt 不明说「用 X 技能」，AI 看 catalog 自主选 → 必须含 new-skill
3. **Skill 工具 miss 隐式重试**：registry 没找到时自动 refresh-retry，不让 LLM 处理"先刷新再用"的状态

## 工业模式调研（Deep Research 2026-06-01）

| 模式 | 代表 | 触发时机 |
|---|---|---|
| 同步 install-triggered | systemd / IntelliJ / VS Code | install/activate 命令本身就是 refresh 触发点 |
| Protocol-level push notify | MCP `notifications/tools/list_changed` | server push 通知，client pull-refetch |
| File system watcher | Claude Code / Continue.dev | fs.watch 监听文件变化（有"新建顶层目录不覆盖"盲区） |

**关键发现**：

- **Claude Code（最接近的产品）** 自己用 fs watcher + `/reload-plugins` 命令兜底。但 watcher **不监听新建的顶层 skill 目录**——而我们的 `lotus_skill.py install` 主要场景就是这个。
- **systemd PID 1 拒绝做 inotify**，所有写操作命令隐式触发 daemon-reload，带外修改必须显式 `daemon-reload`。
- **MCP** 是跨进程场景的最佳协议契约（push notify + pull refetch），但我们是单进程，不需要。

结论：**显式 install-triggered + Skill miss-retry 兜底是 5 家工业大厂的主流模式**。fs watcher 不适合我们的场景（创建新目录是盲区）。

## 设计

### 5 个改动点

```
┌─────────────────────────────────────────────────────────────┐
│ 1. 暴露 refresh_skill_registry 成 Tauri command            │
│    （后端基础设施，供 2/4/5 使用）                          │
└─────────────────────────────────────────────────────────────┘
            │
            ├──→ ┌────────────────────────────────────────────┐
            │    │ 2. 新加 refresh_skills RuntimeTool         │
            │    │   LLM 可在对话里显式调用                    │
            │    └────────────────────────────────────────────┘
            │              │
            │              └──→ ┌──────────────────────────────┐
            │                   │ 3. 改 skill-creator SKILL.md │
            │                   │   step 7（install）后加      │
            │                   │   step 8（refresh_skills）   │
            │                   └──────────────────────────────┘
            │
            ├──→ ┌────────────────────────────────────────────┐
            │    │ 4. load_skill RuntimeTool miss 时          │
            │    │   隐式 refresh-then-retry（兜底）          │
            │    └────────────────────────────────────────────┘
            │
            └──→ ┌────────────────────────────────────────────┐
                 │ 5. SkillCenterPage useEffect 调一次        │
                 │   refresh IPC（非对话路径用户视觉立即反馈）│
                 └────────────────────────────────────────────┘
```

### 改动 1：`refresh_skill_registry` Tauri command

`commands/skill_management.rs` 已经有 `pub fn refresh_skill_registry(app)`。包一层暴露成 IPC：

```rust
#[tauri::command]
pub async fn refresh_skill_registry_cmd(app: AppHandle) -> Result<(), String> {
    refresh_skill_registry(&app)
}
```

`lib.rs` 加注册到 `generate_handler!`。

**影响范围**：1 文件，~6 行代码。

### 改动 2：`refresh_skills` RuntimeTool（LLM 工具）

加在 `runtime/tools/builtin/` 下。最小实现：

```rust
pub struct RefreshSkillsTool;

#[async_trait]
impl RuntimeTool for RefreshSkillsTool {
    fn name(&self) -> &str { "refresh_skills" }
    
    fn definition(&self) -> ToolDefinition { ... } // schema: {} 无参
    
    async fn execute(&self, ctx: ToolExecutionContext, _input: Value) -> ToolResult {
        // 通过 ctx.app_handle 调 refresh_skill_registry
        // 返回 { "refreshed": true, "skill_count": N }
    }
}
```

Catalog 注册：`runtime/tools/catalog.rs` 添加 entry。  
Registry 路由：`plugin/registry.rs` 在 `request_scoped_runtime_tool` 加 `"refresh_skills" => ...` 分支。

**影响范围**：1 新文件 + 2 文件改动，~50 行。

### 改动 3：`skill-creator` SKILL.md 加 step 8

修改 `~/.renlijia/skills/skill-creator/SKILL.md` 的「## 标准创建流程」段：

```diff
  ### 7. 安装到 user skills
  
  ```bash
  python3 scripts/lotus_skill.py install <skill-dir>
  ```
  
  成功后会打印目标路径，形如：
  
  ```
  /Users/<you>/.renlijia/users/t_28__u_54/skills/<skill-id>
  ```
  
+ ### 8. 通知应用刷新 registry
+ 
+ 调用 `refresh_skills` 工具（无参）让 AIjia 立即感知新技能。这一步**必须**做，否则技能不会立刻在对话和技能中心生效。
+ 
+ ### 9. 让用户验收
```

**影响范围**：skill-creator skill 内部修改。这个 skill 是从 lotus 服务端推下来的，需要同步在 OPS 端更新版本（v1.3 → v1.4）。

**注意**：本仓库不直接修服务端 OPS。实施时通过两步：
- 第一步：本地把 `~/.renlijia/skills/skill-creator/SKILL.md` 改了用于验证
- 第二步：在 lotus 服务端 OPS 提交对应版本升级

如果第二步未完成而用户 dev 期间触发了 global_sync，会被服务端的 v1.3 覆盖回旧版本。

### 改动 4：`load_skill` miss 时隐式 refresh-retry

`runtime/tools/builtin/load_skill.rs` 现有逻辑：

```rust
let skill = registry.get(&skill_id).ok_or("not found")?;
```

改成：

```rust
let skill = match registry.get(&skill_id) {
    Some(s) => s,
    None => {
        // 兜底：尝试 refresh 后重查
        let _ = refresh_skill_registry(&app);
        registry.get(&skill_id).ok_or_else(|| format!("Skill '{}' not found", skill_id))?
    }
};
```

**影响范围**：1 文件，~10 行。

### 改动 5：SkillCenterPage 主动 refresh

`src/features/skill-center/SkillCenterPage.tsx`：

```typescript
useEffect(() => {
    void invoke('refresh_skill_registry_cmd')
}, [])
```

可选：加 `visibilitychange` 监听，每次窗口回到前台都刷一次。

**影响范围**：1 文件，~5 行。

## 错误处理与边缘场景

### 1. `refresh_skill_registry` 本身失败

`refresh_skill_registry` 是 IO 操作（扫盘 + 解析 frontmatter），可能因磁盘错误失败。当前 fn 签名是 `Result<(), String>`。

- **改动 2（RuntimeTool）**：失败时返回 `{ "refreshed": false, "error": "..." }`，LLM 决定是否重试
- **改动 4（load_skill miss-retry）**：失败时 swallow 错误，继续走原有的 "not found" 路径——这样兜底不会让 Skill 工具变得更不可靠
- **改动 5（SkillCenterPage）**：失败时 silent log，不影响 UI（用户切到中心看到的是上一次 registry 状态）

### 2. LLM 跳过 step 8

兜底链：
- 同 turn 装+用：改动 4 的 load_skill miss-retry 触发
- 跨 turn 自主决策：用户切到技能中心时改动 5 触发；或者用户自己说"用 X 技能"时改动 4 触发
- 完全没人调任何 refresh：用户重启 app（startup refresh）

### 3. 并发 refresh

`refresh_skill_registry` 内部用 `registry.lock()` 串行写入。两个并发调用会顺序执行，最后一个赢——结果一致，性能稍差但可接受（10ms × 2）。

### 4. registry 跟 disk 仍有 race

理论上：refresh 期间，另一个进程在修改 user_skills_dir → refresh 可能读到中间状态。

但：
- 唯一会从外部修改的进程是 `lotus_skill.py install`，它的写操作是 `cp -r`（POSIX 上是分多个 syscall）
- 真正的 race 窗口是 `cp` 写到一半 → refresh 触发 → 读到部分文件 → frontmatter 解析失败 → skip → 用户看到"我装了但 catalog 没有"
- 缓解：refresh 链路里对 frontmatter 解析失败的 skill 做"重试一次"——超出本设计范围，作为后续 follow-up

## 测试策略

### Unit tests（Rust）

1. `refresh_skill_registry_cmd` IPC 调用后 registry 内容 == 磁盘内容
2. `load_skill` 对不存在的 id miss → 兜底 refresh → 仍 not found → 正确返回 error
3. `load_skill` 对刚装的 id miss → 兜底 refresh → 找到 → 返回 SKILL.md body

### Integration test（review_）

`review_skill_hot_reload_test.rs`：

1. 启动 registry（无 SKILL.md）→ 在 user_skills_dir 写一个 SKILL.md → 调 refresh_skill_registry → registry 含新 skill
2. 测试 load_skill miss-retry 真的触发 refresh（用 mock SkillRegistry 观察 refresh 调用次数）

### E2E（意图测试）

加 1 条意图到 `docs/test-intents/spec/tasks/技能/rules.md`：

> **意图-技能-NNN**：通过 skill-creator 创建技能后，新技能立即在 catalog 中可用，无需重启

操作：
1. 跟小程对话「造一个 hello-world 技能」，等流式完成
2. 立刻新建对话，prompt "用 hello-world 技能"
3. 验证 AI 调了 `Skill(skill_id="hello-world")` 且 tool 返回了 body

## 落地分阶

按 commit 拆：

```
commit (chore): 暴露 refresh_skill_registry IPC + load_skill miss-retry
commit (feat): refresh_skills RuntimeTool + 注册
commit (feat): SkillCenterPage useEffect refresh on mount
[lotus-skills repo, 独立] feat: skill-creator v1.4 加 step 8
```

前 3 个 commit 在本仓库，最后一个在 OPS / lotus-skills 仓库。

## 不在本设计范围

显式不做：

- **fs watcher**：Claude Code 自己证明了 watcher 在「创建新顶层目录」场景有盲区，而我们主要场景就是这个；维护跨平台 inotify/FSEvents/ReadDirectoryChangesW 边界 case 不值得
- **mtime 检测**：研究后确认是过度设计；显式触发已经覆盖所有真实场景
- **每 turn 强制 refresh**：5 家工业大厂没人这么做
- **Cross-app skill 共享**（A app 装 B app 立即看到）：当前是单进程，不需要 MCP-style protocol

## 工业模式参照

| 模式 | 我们 | 参照对象 |
|---|---|---|
| 显式 install-triggered | ✅ 改动 2 + 3 | systemd `systemctl enable` 隐式 reload |
| Pull-refetch 兜底 | ✅ 改动 4 | MCP client 收到 `list_changed` 后 re-fetch `tools/list` |
| 命令式 reload 兜底 | ✅ 改动 5 | Claude Code `/reload-plugins` |
| Lazy activation events | ❌ 不适用 | VS Code（我们的 Skill 工具本身已经是 lazy load） |

## Open Questions（不阻塞落地）

1. **`refresh_skills` 工具是否需要进 skill-creator 的 toolWhitelist 才能用**？小程 v1.2 是 `toolWhitelist: []`（空 = 全部允许），所以小程能用。但其他员工 / 默认对话也能用吗？需要确认 `toolWhitelist: []` 的语义是否包含动态注册的 builtin RuntimeTool。
2. **改动 4 的 miss-retry 是否应该有冷却时间**？比如 LLM 在一个 turn 内连续调 3 个不存在的 skill id，是否每次都 refresh？建议加 5 秒内最多 refresh 一次的 throttle。
3. **改动 5 的 visibilitychange 是否要做**？mount 一次已经覆盖了「打开技能中心」场景，visibilitychange 是为了「app 切到后台又切回来」场景，但可能太敏感（每次 alt-tab 都触发）。
