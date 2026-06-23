# find-skills Agent 市场工具设计

日期：2026-06-17

## 背景

技能中心改版后，AIjia 已经把运行时技能分成两类：

- 已安装技能：来自 `~/.renlijia/users/{scope}/skills/` 和 `~/.renlijia/skills/`，由 `SkillRegistry` 扫盘后进入对话 skill catalog。
- 市场技能：来自网关 `/v1/skill-packages`，只有用户或 Agent 明确安装后，才写入用户技能目录并进入 `SkillRegistry`。

当前要补的是 Agent 自动发现市场技能的链路。用户不会说“有没有浏览器相关技能”，而是会说“访问某网站抓数据”。如果当前已安装 skill catalog 没有覆盖这个能力，Agent 应该能通过 `find-skills` 机制去市场查找、安装合适技能，再继续完成任务。

## 目标

第一版先做专用工具方案，不引入 QoderWork 风格的通用 `query/action` key 分发。

目标是：

1. 增加一个内置 `find-skills` Skill 包，用于告诉 Agent 何时需要查找市场技能。
2. 增加两个 Agent 可调用 RuntimeTool：
   - `SkillMarketSearch`
   - `SkillMarketInstall`
3. `find-skills` 默认开启，但允许用户关闭。
4. 关闭 `find-skills` 后，不注入 `find-skills` Skill，也不暴露 `SkillMarketSearch` / `SkillMarketInstall` 工具 schema。
5. 复用现有市场安装链路，不绕过 `AuthManager`、用户目录、enablement 和 `SkillRegistry` 刷新规则。

## 非目标

第一版不做这些事：

- 不接入 MCP。
- 不实现 `builtin_aijia_query/action` 通用 key/action 体系。
- 不改技能中心 UI 主流程。
- 不假设网关 `search` 参数具备语义检索能力。
- 不维护本地 marketplace cache。
- 不把市场全量技能写进 `SkillRegistry`。
- 不返回大量市场技能描述给模型。

## 当前代码事实

UserWiki 和源码核对后的当前事实：

- 新工具应实现 `RuntimeTool`，不要新增旧 `ToolPlugin`。
- 工具定义和 JSON schema 的单一真相源是 `src-tauri/src/runtime/tools/catalog.rs`。
- request-scoped 工具由 `src-tauri/src/plugin/registry.rs` 基于 `RequestScopedRuntimeDeps` 构造。
- `Skill` / `RefreshSkills` 已经是模型侧 RuntimeTool。
- `SkillRegistry` 只表示磁盘上已安装并解析成功的技能。
- 用户启用状态存储在 `~/.renlijia/users/{scope}/skillsConfig.json`，采用 `disabledSkillIds` 默认开启模型。
- `commands/skill_management.rs::install_marketplace_skill` 已经完成：
  - `AuthManager.get_session_key()`
  - POST `/v1/skill-packages/{package_id}/download`
  - 下载 zip
  - 安装到当前用户 skills 目录
  - 清除该 skill 的 disabled override
  - refresh `SkillRegistry`
- `list_marketplace_skills` 当前会拼 `search` 参数，但这只能证明桌面端会发送参数，不能证明网关支持语义搜索。

## 方案总览

```text
用户任务
  -> 当前已安装 skill catalog 无合适技能
  -> Agent 调用 Skill("find-skills")
  -> find-skills 指令判断是否需要查市场
  -> SkillMarketSearch(task, capabilityHints?)
  -> 返回最多 3 个排序候选
  -> 高置信唯一候选：SkillMarketInstall(packageId, pluginId)
  -> 安装写入用户 skills 目录
  -> refresh SkillRegistry
  -> Agent 继续调用 Skill("<new-skill-id>")
```

这里 `find-skills` 是触发和决策规则；两个 RuntimeTool 是执行能力。

## find-skills Skill

`find-skills` 的职责不是查本地已安装技能。本地已安装技能已经通过 skill catalog 注入给模型。

它只负责市场发现：

- 当用户任务需要某类专项能力，但当前 catalog 没有明显可用技能时，调用 `SkillMarketSearch`。
- 不要让用户显式说“找技能”才触发；应根据任务本身判断。
- 如果搜索结果唯一且高置信，可以继续安装。
- 如果多个候选接近，先用 `AskUserQuestion` 让用户选。
- 如果结果低置信或无结果，不安装，说明当前没有找到合适市场技能。

`find-skills` 应加入 required builtin allowlist，并默认开启。用户可以在技能中心关闭它。

关闭后的语义：

```text
find-skills Skill 不进入 skill catalog
SkillMarketSearch 不进入工具 schema
SkillMarketInstall 不进入工具 schema
Runtime dispatcher 也不注册这两个工具
技能中心手动市场安装仍可用
```

## 工具 schema

### SkillMarketSearch

用途：根据用户原始任务搜索市场技能，内部拉市场列表并本地排序，只返回少量候选。

输入：

```json
{
  "type": "object",
  "required": ["task"],
  "properties": {
    "task": {
      "type": "string",
      "description": "用户原始任务描述，例如：访问某网站抓取价格数据"
    },
    "capabilityHints": {
      "type": "array",
      "description": "可选能力提示，由 Agent 从任务中抽取，例如 browser_automation、web_scraping、spreadsheet_analysis",
      "items": { "type": "string" },
      "maxItems": 5
    },
    "maxResults": {
      "type": "integer",
      "description": "最多返回候选数量，默认 3，最大 5",
      "minimum": 1,
      "maximum": 5,
      "default": 3
    }
  },
  "additionalProperties": false
}
```

输出：

```json
{
  "status": "matched",
  "task": "访问某网站抓取价格数据",
  "candidates": [
    {
      "packageId": 123,
      "pluginId": "browser",
      "name": "browser",
      "descriptionSnippet": "用于访问网页、浏览器自动化、抓取网页数据...",
      "category": "automation",
      "score": 92,
      "confidence": "high",
      "reasons": ["name_match", "description_match", "capability_match"]
    }
  ],
  "truncated": false
}
```

无匹配时：

```json
{
  "status": "no_match",
  "task": "访问某网站抓取价格数据",
  "candidates": [],
  "truncated": false
}
```

如果匹配项已经安装：

```json
{
  "status": "already_installed",
  "task": "访问某网站抓取价格数据",
  "installedSkillId": "browser",
  "candidates": []
}
```

### SkillMarketInstall

用途：安装 `SkillMarketSearch` 返回的候选技能。

输入：

```json
{
  "type": "object",
  "required": ["packageId", "pluginId"],
  "properties": {
    "packageId": {
      "type": "integer",
      "description": "SkillMarketSearch 返回的 packageId"
    },
    "pluginId": {
      "type": "string",
      "description": "SkillMarketSearch 返回的 pluginId / skill id"
    },
    "reason": {
      "type": "string",
      "description": "为什么安装这个技能，用于日志和调试"
    }
  },
  "additionalProperties": false
}
```

输出：

```json
{
  "installed": true,
  "pluginId": "browser",
  "skillId": "browser",
  "refreshed": true,
  "message": "Installed 'browser'",
  "nextAction": "Call Skill with skill_id=browser before using the new capability."
}
```

如果安装前发现已安装：

```json
{
  "installed": false,
  "alreadyInstalled": true,
  "pluginId": "browser",
  "skillId": "browser",
  "refreshed": false
}
```

## 搜索策略

第一版不依赖网关 `search` 参数。

`SkillMarketSearch` 调用现有网关列表能力：

```text
GET /v1/skill-packages?page=1&size=100
```

然后本地排序：

1. 规范化 `task` 和 `capabilityHints`。
2. 对市场项的 `plugin_id`、`name`、`description`、`category` 做轻量匹配。
3. 加权规则：
   - `plugin_id` / `name` 精确或强匹配：高权重。
   - `description` 命中任务关键词：中权重。
   - `category` 命中能力提示：中权重。
   - `featured`、`downloads` 可作为弱排序信号。
   - 已安装技能不作为安装候选。
4. 只返回 top 3，最多 top 5。
5. 不把市场全量名称和描述丢给模型。

分页限制：

- 第一版只拉 page=1 size=100。
- 如果 `total > 100`，返回 `truncated: true`。
- 后续可以补分页或服务端语义检索接口。

## 安装策略

`SkillMarketInstall` 不重新实现下载解压逻辑，而是抽出并复用当前 `install_marketplace_skill` 的内部业务函数。

建议把现有 Tauri command 改成薄封装：

```text
install_marketplace_skill command
  -> install_marketplace_skill_inner(app, auth, package_id, plugin_id)

SkillMarketInstall RuntimeTool
  -> install_marketplace_skill_inner(app, auth, package_id, plugin_id)
```

安装前做防幻觉校验：

1. 校验 `pluginId` 是合法 skill id。
2. 读取当前 `SkillRegistry`，如果已安装，直接返回 `alreadyInstalled`。
3. 重新拉 page=1 size=100，确认存在同一组 `packageId + pluginId`。
4. 不存在则拒绝安装，返回 `candidate_not_found`。
5. 校验通过后再走现有下载、安装、清 override、refresh 流程。

这能避免模型随口编一个 package id 触发安装。

## 工具暴露与关闭逻辑

这是第一版最容易踩坑的地方。

当前 `get_schemas_filtered` 对 request-scoped 工具有静态 catalog fallback。如果直接把 `SkillMarketSearch` / `SkillMarketInstall` 加进 request-scoped 名单和 daily whitelist，即使 `find-skills` 关闭，也可能因为 fallback 暴露 schema。

建议做法：

1. 新增“条件 request-scoped 工具”概念。
2. `SkillMarketSearch` / `SkillMarketInstall` 属于条件工具。
3. `build_request_scoped_tool_overrides` 只有在 `find-skills` 当前 enabled 时，才渲染这两个工具的 schema override。
4. `get_schemas_filtered` 对条件工具禁止静态 fallback；没有 override 就不出现。
5. `to_runtime_dispatcher` 构造 request-scoped 工具时，也检查 `find-skills` enabled；关闭时不注册。
6. 工具 `execute` 内部再做一次 enabled 检查，作为防御。

判断 `find-skills` 是否 enabled：

```text
SkillRegistry.get("find-skills") 存在
SkillEnablementStore.load_or_default().is_enabled("find-skills")
```

如果 `find-skills` 包不存在，两个市场工具也不暴露。

## 需要改的模块

### Rust

- `src-tauri/src/runtime/tools/catalog.rs`
  - 增加 `SkillMarketSearch` / `SkillMarketInstall` schema。
  - 加入 daily allowed 工具，但必须配合条件暴露，不能单独暴露。

- `src-tauri/src/runtime/tools/builtin/mod.rs`
  - 导出新的 skill market 工具模块。

- `src-tauri/src/runtime/tools/builtin/skill_market.rs`
  - 新增两个 RuntimeTool。
  - 封装搜索排序、输入校验、输出结构。

- `src-tauri/src/plugin/registry.rs`
  - request-scoped factory 增加市场工具。
  - 增加条件工具 schema fallback 规则。
  - 关闭 `find-skills` 时不构造工具。

- `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs`
  - `build_request_scoped_tool_overrides` 在 `find-skills` enabled 时渲染市场工具描述。

- `src-tauri/src/commands/skill_management.rs`
  - 抽出 `list_marketplace_skills_inner` 和 `install_marketplace_skill_inner`，供 Tauri command 和 RuntimeTool 复用。

- `src-tauri/src/plugin/skill/required_builtin.rs`
  - 增加 `find-skills` required builtin，默认 enabled。

### TypeScript

- `src/lib/skillAvailability.ts`
  - required builtin 列表增加 `find-skills`，使技能中心分类正确。

- 现有技能中心 UI 不需要主流程改造。

### 本地验证与 OPS / 服务端发布

- `find-skills` 是一个真正的 `SKILL.md` 技能包，不是 RuntimeTool。
- 第一阶段可以先把 `find-skills/SKILL.md` 放到本机技能目录验证：
  - 全局目录：`~/.renlijia/skills/find-skills/SKILL.md`
  - 或当前用户目录：`~/.renlijia/users/{scope}/skills/find-skills/SKILL.md`
- 验证通过后，由用户上传到企业后台/官方技能市场，成为 `plugin_id = find-skills` 的技能包。
- 代码侧要把 `find-skills` 加入 `REQUIRED_BUILTIN_SKILLS`，这样登录同步时按 required builtin 处理，默认安装到全局技能目录。
- 安装后默认开启；如果用户在技能中心关闭它，只写当前用户 scope 下的 `skillsConfig.json`，不删除技能包。
- 本仓库不会把 `find-skills` 的完整 `SKILL.md` 当作 Rust/前端代码内置进去。真正内容以本地验证稿和企业后台包为准。

## find-skills SKILL.md 内容要点

`find-skills` 的提示词要短，避免本身占太多上下文。

建议包含：

- 何时使用：
  - 当前已安装技能无法覆盖任务。
  - 用户任务明显需要专项能力，例如浏览器自动化、网页抓取、特定文件处理、企业系统操作。
- 何时不用：
  - 已安装技能 catalog 已有明确匹配。
  - 普通聊天或基础文件读写工具能完成。
- 搜索规则：
  - 把用户原始任务放进 `task`。
  - 可选填写 `capabilityHints`。
  - 不要用用户看不懂的技能名反问用户。
- 安装规则：
  - 高置信唯一候选可以安装。
  - 多候选或低置信先问用户。
  - 安装后先调用 `Skill(skill_id)` 加载新技能指令，再继续执行。

## 后续演进

等第一版稳定后，再迁移到 QoderWork 风格的通用入口：

```text
builtin_aijia_query
builtin_aijia_action
```

届时可以把能力组织成 key：

```text
aijia.settings.skills.market
aijia.settings.skills.{skillId}
aijia.settings.connectors.market
```

迁移时保留兼容：

- 第一阶段：两个专用工具继续存在。
- 第二阶段：专用工具内部转调通用 key/action。
- 第三阶段：隐藏专用工具，只暴露通用工具。

## 测试计划

Rust 单元测试：

- `SkillMarketSearch`：
  - 高置信候选排序正确。
  - 已安装技能不作为安装候选。
  - 无匹配返回 `no_match`。
  - `total > size` 时返回 `truncated: true`。

- `SkillMarketInstall`：
  - 缺少 `packageId` / `pluginId` 时 input validation 失败。
  - 已安装时返回 `alreadyInstalled`。
  - 搜索列表中找不到 `packageId + pluginId` 时拒绝安装。
  - 安装成功后调用现有 install inner，清 override 并 refresh registry。

- 工具暴露：
  - `find-skills` enabled 时，daily tool schema 包含 `SkillMarketSearch` / `SkillMarketInstall`。
  - `find-skills` disabled 时，daily tool schema 不包含这两个工具。
  - `find-skills` disabled 时，dispatcher 不注册这两个工具。

- catalog 合同：
  - 两个工具在 `TOOL_CATALOG` 中存在。
  - schema 与工具 id 匹配。

前端测试：

- `skillAvailability` required builtin 包含 `find-skills`。
- 技能中心内置 tab 能正确展示 `find-skills`。
- 关闭 `find-skills` 后仍可手动打开市场页安装技能。

手工/意图验证：

- 用户说“访问某网站抓数据”，当前无浏览器类技能时，Agent 能搜索并安装。
- 用户关闭 `find-skills` 后，同样请求不会暴露搜索/安装工具。
- 安装成功后无需重启，新技能能被 `Skill` 工具加载。

## 风险和未决点

1. 网关 page=1 size=100 可能不够。
   - 第一版标记 `truncated`，后续补分页或服务端语义检索。

2. 市场排序是本地轻量规则，不是语义检索。
   - 第一版先确保可控和可验证，后续再接服务端搜索能力。

3. `find-skills` skill 包需要服务端/OPS 同步发布。
   - 本仓库只负责 required builtin allowlist、默认安装/默认开启和 runtime 工具开关联动。
   - `SKILL.md` 内容先本地验证，再由企业后台技能包分发。

4. 自动安装是否需要用户确认。
   - 当前产品判断是官方/企业市场可信。第一版建议：高置信唯一候选可自动安装；多候选或低置信必须问用户。

5. 条件工具暴露需要小心处理 static fallback。
   - 这是实现重点，必须用测试锁住关闭后不占上下文。

## 建议实施顺序

1. 先写工具暴露/关闭相关测试，锁住“关闭后不进上下文”。
2. 抽出 marketplace list/install inner 函数。
3. 实现 `SkillMarketSearch` 排序和输出。
4. 实现 `SkillMarketInstall` 校验和安装复用。
5. 加 `find-skills` required builtin 和前端分类。
6. 补 `find-skills` skill package 文案，并同步到 OPS/市场。
7. 跑 Rust 聚焦测试、前端分类测试和一次手工对话验证。
