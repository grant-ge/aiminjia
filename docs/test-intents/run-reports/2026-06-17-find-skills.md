# 意图测试跑测报告 - 2026-06-17 find-skills

> 跑测人：Codex
> 应用版本：dev with pilot
> 登录态：`t_28__u_54`
> 主题：技能 task 意图 027-033，发现技能与市场安装链路

---

## 结论

本轮按真实 AIjia 桌面应用、真实登录态、真实技能中心和 `tauri-pilot aijia` 命令执行，不使用单元测试作为验收。

当前结论是：`find-skills` 链路未通过，主因不是模型执行阶段，而是数据/环境前置缺失。企业市场同步结果里没有 `find-skills` 包，市场页也没有 `find-skills-e2e-*` 测试包，因此 027 失败，028-033 只能阻塞或跳过。

同时确认 `browser` 内置技能已经存在且已安装，不受本次 `find-skills` 缺失影响。

---

## 环境探活

```bash
tauri-pilot aijia health-check --json
tauri-pilot aijia where --json
```

- `health-check.ok == true`
- `readyState == "complete"`
- `scope == "t_28__u_54"`
- 当前登录态有效，编辑器可用。

---

## 意图-技能-027：发现技能内置后，默认开启 - FAIL

**实测操作**

```bash
tauri-pilot aijia skill-center-open --json
tauri-pilot aijia sync-builtin-skills --json
tauri-pilot aijia skill-center-tab --name builtin --json
tauri-pilot aijia skill-center-list --json
tauri-pilot aijia new-task --wait-fresh --json
tauri-pilot aijia visible-tools --json
```

**现场结果**

- 技能中心内置页只有 3 个技能：
  - `browser`，title `浏览器自动化`，source `global`，enabled `true`
  - `dingtalk-workspace`，source `tenant`，enabled `true`
  - `skill-creator`，source `tenant`
- `C:\Users\Administrator\.renlijia\skills\find-skills\SKILL.md` 不存在。
- `C:\Users\Administrator\.renlijia\users\t_28__u_54\skills\find-skills\SKILL.md` 不存在。
- 新对话 `visible-tools` 返回 `count == 32`，包含 `Skill`，但不包含 `SkillMarketSearch` / `SkillMarketInstall`。
- `Skill` 工具可见技能列表里没有 `find-skills`。
- 应用日志显示同步从 `https://ai-tenant.renlijia.com` 拉到 41 条技能，去重 34 个 plugin id，其中有 `browser`，没有 `find-skills`。

**判定**

FAIL。验收要求 `find-skills` 本地存在、技能中心存在、默认开启、运行时可触发市场搜索/安装；当前全部不满足。

**附带发现**

`sync-builtin-skills` 当前在 UI 中没有找到可点击的同步动作，返回 `skill_sync_action_not_found`。不过应用启动日志已经证明真实自动同步执行过，所以本意图的主因仍是市场/内置数据没有 `find-skills` 包。

---

## 意图-技能-028：关闭发现技能后，自动发现不可用 - BLOCKED

**实测操作**

```bash
tauri-pilot aijia skill-center-toggle --id find-skills --enabled false --json
tauri-pilot aijia skill-picker-open --json
```

**现场结果**

- `skill-center-toggle` 返回 `reason == "skill_toggle_not_found"`。
- 聊天输入框技能选择器返回 4 个技能：`browser`、`dingtalk-workspace`、`ganbu-competency-case-collection`、`skill-creator`，没有 `find-skills`。

**判定**

BLOCKED。`find-skills` 没有安装，无法验证关闭后的运行时注入行为。

---

## 意图-技能-029 到 033：市场安装链路 - SKIPPED

**实测操作**

```bash
tauri-pilot aijia skill-center-open --json
tauri-pilot aijia skill-center-tab --name market --json
tauri-pilot aijia skill-market-list --include-description --json
```

**市场结果**

- 市场页加载后 `count == 34`。
- `browser` 存在，`packageId == "98"`，`installed == true`，source `global`。
- 以下测试前置包均不存在：
  - `find-skills`
  - `find-skills-e2e-web-fetch`
  - `find-skills-e2e-choice-alpha`
  - `find-skills-e2e-choice-beta`
  - `find-skills-e2e-disable-after-install`

**判定**

- 029 SKIPPED：`find-skills-e2e-web-fetch` 不存在，且 `find-skills` 未安装。
- 030 SKIPPED：`choice-alpha` / `choice-beta` 不存在。
- 031 SKIPPED：依赖 `find-skills` 默认可用，当前前置失败。
- 032 SKIPPED：依赖 `find-skills-e2e-web-fetch` 可安装，当前前置不存在。
- 033 SKIPPED：依赖 `find-skills-e2e-disable-after-install` 可安装，当前前置不存在。

---

## 需要补齐的前置

1. 企业后台上架 `find-skills` 技能包，并让它进入当前租户/公共可见市场。
2. 如果要跑完整 029-033，需要上架测试专用包：
   - `find-skills-e2e-web-fetch`
   - `find-skills-e2e-choice-alpha`
   - `find-skills-e2e-choice-beta`
   - `find-skills-e2e-disable-after-install`
3. `sync-builtin-skills` 的 UI 原子动作需要确认：如果产品已经不提供手动同步入口，rules 应改为依赖登录后的自动同步日志和技能中心快照。
