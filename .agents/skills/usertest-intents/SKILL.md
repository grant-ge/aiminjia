---
name: usertest-intents
description: Use when the user mentions intent tests for the AIjia desktop app at any level — asking what they are, running them, or modifying their specs. Triggers include "意图测试", "usertest-intents", "AEIT", "跑意图测试", "跑一下 X 这个 task", "跑 意图-XXX-NNN", "写意图", "加一条意图", "改 X task 的 rules", "意图测试怎么用".
license: Internal
---

# usertest-intents

## 是什么（What this is）

**意图测试体系的用户级入口 skill**。任何关于 AIjia 意图测试的对话，agent 应该先命中本 skill，从这里得到 3 件事：

1. 一句话搞清「意图测试是什么」（避免 agent 凭直觉乱跑）
2. 当前有哪些 task 可以跑（15 个，下面 §3 列出）
3. 该去哪个**实现 skill** 真正干活（runner 跑意图、author 写意图）

本 skill 是路由 + 概览，**不含完整方法论**。完整内容在 `test-intents-runner` / `test-intents-author` 两个 skill 里。

## 为什么需要它（Why）

如果只有 author 和 runner 两个实现 skill：
- 用户敲「意图测试怎么用」→ Claude Code 不知道该加载哪个（两个 description 都不太匹配纯咨询场景）
- 用户敲一个意图模糊的话（「帮我看下日程」）→ 容易误命中错误的实现 skill
- 新同事不知道有几个 task、不知道该读哪份 rules.md

`usertest-intents` 解决这三件事：**纯解释场景**用本 skill 回答；**写或跑**场景由本 skill 路由到对应实现 skill。

## 怎么用（How，给 agent 看的）

### 路由规则

| 用户输入 | agent 应该做 |
|---|---|
| 「跑一下 X 这个 task」/「跑 意图-XXX-NNN」/「跑全部意图测试」 | 调用 Skill 工具加载 `test-intents-runner`，��它执行 |
| 「给 X 加一条意图」/「改 X task 的 rules」/「写意图」/「删 意图-XXX-NNN」/「拆这条复合意图」 | 调用 Skill 工具加载 `test-intents-author`，按它写 |
| 「意图测试是什么」/「意图测试怎么用」/「介绍下 AEIT」（**纯咨询**） | 按本 skill §1-§4 回答，**不分发**到其它 skill |
| 用户意图模糊（如「帮我看下日程」）| **先反问清楚**：「你是想跑日程意图测试，还是改日程 rules.md？」再分发 |

### 实操示例

**例 1：用户说「跑一下日程这个 task」**
- 命中本 skill → 路由到 `test-intents-runner`
- runner 接管：读 `docs/test-intents/spec/tasks/日程/rules.md` → 按操作步骤跑 → 出报告

**例 2：用户说「日程 task 想加一条新意图：员工权限不足时不允许创建日程」**
- 命中本 skill → 路由到 `test-intents-author`
- author 接管：和用户确认承诺方向 → 按字段集 / ID / 措辞规则写出 rules.md diff → 对话 review → commit

**例 3：用户说「意图测试是干啥的？」**
- 命中本 skill → 直接用 §1-§4 回答
- 不加载 runner / author（它们体量大，纯咨询用不上）

## 1. 一句话定义

意图测试 = **AEIT**（Agent-Executed Intent Test）= 在真实开发机的真实账号下，agent 跑一遍真实操作，留下真实痕迹。

- 仅在 **L4** 跑（tauri-pilot e2e + 真应用 + 真 LLM + 真磁盘），**不**在 cargo / vitest 跑
- 一条意图 = 一条产品承诺（PM 视角微观）
- agent 自己跑、自己判 PASS/FAIL、自己在对话里写报告

权威设计：`docs/superpowers/specs/2026-05-20-intent-test-redefinition-design.md`。

## 2. 不在范畴里的（避免误用）

**Not for**：
- cargo 单测、vitest 组件测试 → 这些是 L1/L3，归 cargo / vitest 各自的工具链
- L2 review_*.rs 集成测试（架构约束回归）→ 归 cargo
- 不能用 mock LLM / 不��用 fake 文件系统 → 违反「真实环境」原则
- 不写承诺方向的判断（如"测试覆盖率多少 / 通过率多少"）→ 意图测试不为统计存在

## 3. 15 个意图测试 task

每个 task 对应一份 rules.md，位于 `docs/test-intents/spec/tasks/<中文名>/rules.md`：

| Task | 测什么承诺 |
|---|---|
| 日程 | 创建 / 触发 / 暂停 / 立即运行 / 执行历史 |
| 登录 | 账号登录 / 退出 / 切租户 / 品牌切换 |
| 对话 | 长生成 / 文件产物 / 后台任务 |
| 升级 | 更新包下载 / 重试 / 安装前版本刷新 |
| 钉钉频道 | 钉钉接入、@员工、消息双向 |
| 崩溃恢复 | 应用突然崩了再开，对话历史 / 草稿 / 任务都还在 |
| 数字员工 | 雇佣 / 派活 / 暂停 / 归档 / 模板配置 |
| 专家团队 | 多员工协作、任务分发 |
| 待办队列 | pending queue 触发 / 取消 / 完成 |
| 人格 | 人格创建与切换 |
| 项目记忆 | memory 工具 read / write / 注入 |
| 搜索 | 全文搜索 / 文件搜索 |
| 设置 | 应用设置项的优先级与持久化 |
| Runtime | Runtime 诊断 / Node 与 Python 包复用 |
| 技能 | 技能导入 / 启用 / 中心 |
| 工作空间 | workspace 选择 / 切换 / 文件可见 |

## 4. 全局约束（写 / 跑都受这套，runner / author 各自再展开）

- **环境契约**：直接在真实 `~/.renlijia/` 跑、**不**隔离、跑后**不**清理；scope 从 `tauri-pilot aijia where --json` 推断；要纯净环境去新电脑
- **不持久化**：跑完报告**只在对话里**输出，仓库内无 `progress.md`、无历史归档
- **意图自给自足**：意图之间**无顺序依赖**；意图作废 = **硬删除整段**，不留废弃标记
- **不绑 spec / plan 流程**：加新 feature 时漏加意图不算流程违规；改老功能忘改意图，下次跑 task 时 agent 跑出 FAIL 反推

## 5. 当你需要更深内容时

| 想做的事 | 加载哪个 skill |
|---|---|
| 跑意图 / 处理 FAIL / 用 `aijia` CLI / 写报告 | `test-intents-runner` |
| 写 / 改 / 删意图 / 字段格式 / 自查 lint | `test-intents-author` |
| 看设计决策原文 | `docs/superpowers/specs/2026-05-20-intent-test-redefinition-design.md` |

## Red Flags（看到自己在这样想就停下来）

| 想法 | 应该 |
|---|---|
| 「用户问意图测试是啥，我直接加载 runner 看里面的 §1 给他讲讲」 | 先用本 skill 回答；纯咨询不要加载实现 skill |
| 「用户说『日程怎么样』，他一定是想跑」 | 不一定——先反问清是跑还是写 |
| 「我这次只是改一个字，不需要走 author skill 的流程」 | 任何改 rules.md 的动作都走 author，不绕过 |
| 「我想直接读 rules.md 跑」 | 必须先加载 runner——它有 CLI 手册和 FAIL triage 规则你需要 |
