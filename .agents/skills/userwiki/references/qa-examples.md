# 真实问答验收集

这些用例用于测试 `userwiki` 的问答效果。它们不是 UI 意图测试，也不替代 `test-intents-*` skills；它们只验证项目 wiki 问答是否能命中图谱、RepoWiki、当前源码和测试。

CLI smoke 用例配置在 `references/qa-smoke-cases.json`，执行方式见 `scripts/run-userwiki-qa-smoke.mjs`。

## 验收口径

合格回答必须满足：

1. 使用中文。
2. 不主动打开浏览器。
3. 先用图谱和 RepoWiki 导航。
4. 架构事实不清楚时，说明需要用当前源码或测试确认。
5. 给出文件、模块、测试、文档和不确定点。
6. 不把图谱完整性扩展成自动审计、完整测试覆盖证明或函数级全链路 trace。

失败判据：

- 只给泛泛建议，没有落到文件或模块。
- 从旧 docs 推断当前架构，却不说明来源风险。
- 用户没有要求 dashboard 时主动操作浏览器。
- 把 `test-intents-*` skills 误判为无用内容。
- 遇到图谱缺口时不说明不确定点。

## 高价值真实用例

这些用例来自当前 lotus-app 的真实模块，优先用于 smoke 和子 agent 复核。

### Runtime 工具权限模式影响面

问题：

```text
wiki 我想新增一个 Runtime 工具权限模式，会影响哪些模块？
```

预期回答结构：按“功能影响面”8 段结构回答。

必须命中：

- `src-tauri/src/runtime/tools/permission.rs`
- `src-tauri/src/runtime/tools/dispatcher.rs`
- `src-tauri/src/runtime/query_engine.rs`
- `src-tauri/src/runtime/store/permission_store.rs`
- `src/lib/tauri.ts`
- `docs/repo-wiki/runtime-map.md`
- `PermissionDecision`
- `PermissionMode`
- `ToolPermissionContext`

失败判据：

- 只列 Rust 文件，不提前端 IPC 或测试。
- 把 legacy `llm/tool_executor` 当成新工具入口。
- 没有按 8 段结构回答。

### 技能中心 pending 状态影响面

问题：

```text
我要给技能中心新增 pending 校验状态，wiki 看看影响哪些点？
```

预期回答结构：按“功能影响面”8 段结构回答。

必须命中：

- `src/features/skill-center/SkillCenterPage.tsx`
- `src/stores/skillStore.ts`
- `src/stores/pendingStore.ts`
- `src/lib/tauri.ts`
- `src-tauri/src/commands/skills.rs`
- `.understand-anything/enhancements/frontend-skill-pending.json`

失败判据：

- 只回答 UI 卡片。
- 漏掉 Zustand、IPC 或 Rust command。
- 没有提相关 Vitest 或 RepoWiki 更新。

### 解释 userwiki skill 本身

问题：

```text
这个文件 .agents/skills/userwiki/SKILL.md 是干什么的？
```

预期回答结构：文件职责、所属 layer/module、重要入边、重要出边、相关测试/校验、什么时候应该改它、不确定点。

必须命中：

- `.agents/skills/userwiki/SKILL.md`
- `.agents/skills/userwiki/references/qa-playbook.md`
- `install.md`
- `usage.md`
- `maintenance-routing.md`
- `scripts/check-repowiki.mjs`
- `用户入口`
- `wiki-maintainer`
- `不要主动打开浏览器`

失败判据：

- 把它说成维护入口。
- 没有说明转 `wiki-maintainer` 的边界。
- 建议删除或弱化 `test-intents-*`。

### 解释 RuntimePanel

问题：

```text
wiki explain src/components/settings/panels/RuntimePanel.tsx
```

预期回答结构：文件职责、所属 layer/module、入边、出边、相关测试、修改场景、不确定点。

必须命中：

- `src/components/settings/panels/RuntimePanel.tsx`
- `src/lib/tauri.ts`
- `src/stores/runtimeStore.ts`
- `src/stores/settingsStore.ts`
- `src/components/settings/SettingsModal.tsx`
- `src/lib/tauri.runtime.test.ts`
- `bundled`
- `installed`
- `current`
- `health/diagnostics`

失败判据：

- 只解释 React UI。
- 漏掉 `runtimeStore` 或 Tauri IPC。
- 不提设置页挂载关系。

### 当前改动影响面

问题：

```text
userwiki 当前改动影响哪些模块？
```

预期回答结构：changed files 对应图谱节点、1-hop 受影响组件、受影响 layer、风险/注意点、建议测试、需要更新的 docs/wiki、图谱缺口。

必须命中：

- `git diff --name-only`
- untracked files
- `.agents/skills/userwiki/`
- `.agents/skills/wiki-maintainer/`
- `.claude/skills/userwiki/`
- `.claude/skills/wiki-maintainer/`
- `.understand-anything/`
- `docs/repo-wiki/`
- `scripts/check-repowiki.mjs`
- `scripts/apply-understand-enhancements.mjs`

失败判据：

- 只看 `git diff --name-only`，漏掉 untracked。
- 不说明这是 Wiki、图谱和 skill 系统改动。
- 建议移除或降权 `test-intents-*`。

## 通用用例 1：新增设置项影响面

问题：

```text
userwiki 我想新增一个设置项，会影响哪些点？
```

预期回答结构：

1. 可能涉及的模块/layer
2. 关键文件
3. 上游入口
4. 下游影响
5. 相关测试
6. 需要同步更新的文档/RepoWiki
7. 图谱不确定点
8. 建议下一步

必须命中：

- `src/types/settings.ts`
- `src/stores/settingsStore.ts`
- `src/components/settings/`
- `src/lib/tauri.ts`
- `src-tauri/src/models/settings.rs`
- `src-tauri/src/commands/settings.rs`
- 如果影响 Runtime，还要提到 `TurnConfig` 或 `ResolvedLlmSettings`

## 用例 2：解释 Runtime turn 配置文件

问题：

```text
userwiki 这个文件是干什么的：src-tauri/src/runtime/chat/turn_config.rs？
```

预期回答结构：

1. 文件职责
2. 所属 layer/module
3. 重要入边
4. 重要出边
5. 相关测试
6. 什么时候应该改它
7. 不确定点

必须命中：

- `src-tauri/src/runtime/chat/turn_config.rs`
- `TurnConfig`
- `ResolvedLlmSettings`
- Runtime 每 turn 设置快照
- 相关测试建议

## 用例 3：当前改动影响面

问题：

```text
userwiki 当前改动影响哪些模块？
```

预期回答结构：

1. changed files 对应的图谱节点
2. 1-hop 受影响组件
3. 受影响 layer
4. 风险或注意点
5. 建议运行的测试
6. 需要更新的 docs/wiki
7. 图谱缺口

必须命中：

- `git diff --name-only`
- untracked files 是否纳入
- `.understand-anything/knowledge-graph.json`
- `docs/repo-wiki/`

说明：这个用例依赖当前工作区状态，适合人工或子 agent 评估，不作为默认自动 smoke。

## 用例 4：图谱完整性

问题：

```text
userwiki 当前图谱完整了吗？
```

预期回答结构：

1. 先说明“完整”的口径。
2. 给出节点、边、layer、tour、enhancement 的当前状态。
3. 给出已通过或需要运行的校验命令。
4. 明确不覆盖的范围。

必须命中：

- `.understand-anything/knowledge-graph.json`
- `.understand-anything/config.json`
- `outputLanguage=zh`
- `node scripts/check-repowiki.mjs`
- 不等于自动风险审计系统
- 不等于完整测试覆盖证明
- 不等于函数级全链路 trace

## 用例 5：安装和中文初始化

问题：

```text
userwiki 怎么安装 Understand-Anything，并用中文生成图谱？
```

预期回答结构：

1. 标准安装命令
2. Codex 非交互安装命令
3. 中文生成命令
4. 重新安装路径
5. 校验方式

必须命中：

- `curl -fsSL https://raw.githubusercontent.com/Lum1104/Understand-Anything/main/install.sh | bash`
- `bash -s codex`
- `~/.understand-anything/repo`
- `--language zh`
- `node scripts/check-repowiki.mjs`

## 用例 6：新人阅读路径

问题：

```text
userwiki 新人应该先看什么？
```

预期回答结构：

1. 先给短路径。
2. 按架构、Runtime、前端、测试分组。
3. 每组给 1-3 个入口。
4. 标出图谱或 RepoWiki 的入口。

必须命中：

- `docs/repo-wiki/index.md`
- `docs/repo-wiki/architecture-map.md`
- `docs/repo-wiki/runtime-map.md`
- `docs/repo-wiki/frontend-map.md`
- `docs/repo-wiki/testing-and-commands.md`
- guided tour
