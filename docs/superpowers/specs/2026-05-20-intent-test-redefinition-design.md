# 意图测试重新定义 — design

> 状态：设计完工，待 user review；通过后转 writing-plans
> 日期：2026-05-20
> 主导：pzc + Claude（brainstorming）
> 上下游：替代 `docs/test-intents/README.md` + `context/` 四份方法论文档的现有叙事。最终交付包括：①本 design ②对应实施 plan ③ `docs/test-intents/` 全面改写 ④ 3 个仓库级 skill 创建 ⑤ CLAUDE.md "意图测试框架" 节整段删除。

---

## 0. 为什么重新定义

现状（详见 `MEMORY.md` 中此次 brainstorming subagent 的调研报告）：

1. **方法论文档自相矛盾**：5/14 `73156d9d` 把 `how-to-write-rules.md` 重写为"真实 e2e、禁 mock"，但 `README.md` / `context.md` / `how-to-test.md` / `capabilities.md` 前半 / `CLAUDE.md` 第 4 节仍按"cargo Mock 集成测试"叙事，5 份文档无法合在一起读。
2. **rules.md ↔ 真实测试代码无机器化连接**：靠命名约定 + 注释字符串，没 generator / runner / CI hook。
3. **29 个 task 里 12 个最近一月新填的 rules.md 的 progress.md 全是表头占位，0 条真跑过**。
4. **rules.md 自身格式漂移**：粒度、ID 体系、上游 spec 引用、"一条意图一件事"规范 5 份样本里都被违反。

这次重新定义要让"意图测试是什么、谁来跑、什么时候跑、怎么写、怎么记录"重新形成一致叙事，把 5 份分裂的方法论文档收敛成一套。

---

## 1. 核心定义（已敲定）

### 1.1 意图测试 = L4 only

仓库内 4 层测试金字塔：

| 层 | 形态 | 是否算意图测试 |
|---|---|---|
| L1 cargo lib unit test (`#[cfg(test)]` 模块) | 函数级 / 纯函数 | ❌ 否 |
| L2 cargo 集成测试 (`src-tauri/tests/review_*.rs` 等，可含 MockLlmExecutor / TempDir) | 跨模块组合 + Mock | ❌ 否 |
| L3 vitest (`src/**/__tests__/`) | 组件 / store / hook | ❌ 否 |
| **L4 tauri-pilot e2e**（真应用 + 真 UI + 真 LLM + 真磁盘） | 端到端 | ✅ **唯一形态** |

意图测试 **=** L4 tauri-pilot e2e。

落地后果：
- 现存 `src-tauri/tests/review_*.rs` + Mock 那套被改判为 **L2 集成测试**，不再算意图测试产出物。它们仍然有价值（架构约束回归测试），但 progress.md 里"对应实现 = review_*.rs"的引用关系不再成立。
- `context/capabilities.md` 前半 MockLlmExecutor / ProbeExecutor 代码片段降级为"L2 集成测试参考"，从意图测试方法论里剥离。
- `context/how-to-test.md` 整篇被新方法论替代。

### 1.2 意图边界口径 = "一条产品承诺"

一条意图 = "用户在某场景下，按某步骤操作，会看到某结果，背后磁盘/状态会变成某样子" 这种用户视角的微观承诺。

特征：
- PM 能读懂、agent 能照着做
- 一个 feature 通常对应 5-15 条
- 不要写抽象不变式（"系统永不丢消息"这种交给 L2 review 测试）
- 不要写多页面长旅程（"注册→雇佣→派活→看报告"这种拆成多条意图）

### 1.3 验收判定方式 = Agent 自评

意图测试的执行者**就是一个 agent**：

- agent 读 rules.md → 自己启动应用 → 自己用 `tauri-pilot aijia` CLI 操作 UI → 自己看磁盘 → **自己判定 PASS/FAIL** → 自己写 progress.md
- 因此 rules.md 是写给 agent 看的任务书，不是写给 cargo / pytest 看的 assertion
- 验收标准的形式 = "给 agent 的判定指引"：用产品视角自然语言描述"什么算 PASS / 什么算 FAIL / 模糊时怎么办"，不需要伪代码 / 不需要机器可解析格式
- LLM 输出内容的合理性判断由 agent 来做（agent 是有判断力的），机器不需要自动判 LLM 内容；机器需要确定性判定的事（文件落盘 / DOM 出现 / 状态机迁移）也由 agent 来核对，但形式上还是 agent 用 CLI 自己查
- 简称：**Agent-Executed Intent Test (AEIT)**

### 1.4 触发时机（已敲定）

agent 跑意图测试的三个触发时机：

- **A. 人工手动调起**（主路径）：你/同事 在对话里说「跑一下日程这个 task」，agent 拿 rules.md 就跑
- **B. 功能完成时**（备选路径）：改完一个 feature，**可以**叫 agent 跑对应 task 作为"功能验收"，但这**不是 superpowers:verification-before-completion 那条技能管的事**——AEIT 是单独的、可独立调起的能力，不强行绑进通用 verification 流程
- **C. 定时全量**（未来能力）：CI / cron 周期性调起 agent 把所有 task 跑一遍，作为防架构腐化 / 防 LLM provider 出帕的基线。当下未实施，记录为目标态。

### 1.5 与 spec / plan 流程的关系 = 完全独立

意图测试**不绑定** `docs/superpowers/specs/` 和 `docs/superpowers/plans/` 流程。

- spec 不要求带「应新增意图清单」段
- plan 不要求带「补意图」step
- 实施完代码后不强制同步改意图

理由：
- 意图测试是**产品承诺层**的事，spec / plan 是**实施流程层**的事，两层耦合会让 spec / plan 变臃肿，也会逼着架构重构类 plan 凑意图
- 谁觉得该加 / 改 / 删意图就独立向 agent 提议，agent 加载 `test-intents` skill 写一条，作者对话 review 后 agent commit（详见 §5.5）
- 实施代码和意图修改可以同一 commit 提，也可以分开 commit；选择权在作者

落地后果：
- 加新功能时漏加意图 = 可能的现象，不视为流程违规
- 改老功能承诺后忘改意图 = 下次跑该 task 时 agent 会跑出 FAIL，FAIL 后人来判「改实现 / 改意图」（详见 §2.2 关于 Source 字段的副作用明牌）
- 不靠流程钩子保证意图覆盖率，靠人主动意识 + agent 跑 FAIL 反推

---

## 2. rules.md 模板（已敲定）

### 2.1 ID 体系

格式：**`意图-<task>-<NNN>`**

- `意图` 前缀全局可 grep
- `<task>` = 该意图所属 task 中文名（如 `日程` / `技能` / `登录`）
- `<NNN>` = 三位顺序号，**新意图取当前最大序号 +1**
- **删除意图不回收 ID**——新加的意图不会复用已删除条目的编号；这会导致 ID 跳号（如 002 → 004），接受这个现象
- **rules.md 内多条意图按 ID 升序排列**——新加意图直接 append 到文件末尾，不重排；ID 跳号造成的语义跳跃靠**意图标题本身的可读性**承担，不靠物理顺序

软规则：**相关意图的标题前缀尽量保持一致**（如「创建...」「触发...」「暂停...」），这样物理排序虽然机械，但 `grep 创建` 能把所有创建相关意图捞出来。这是建议、不是硬规则。

### 2.1.1 意图作废 = 硬删除整段

意图作废时直接从 rules.md 删除整段，**不留任何痕迹**：

- 不留「已废弃」标记
- 不留标题占位
- 不留替代意图指针
- 不抽到单独的「废弃.md」文件

理由：**任何废弃痕迹都会误导 agent**——agent 看到「已废弃 意图-日程-003」可能仍会读它的描述、可能困惑该跑还是不该跑、可能让废弃语义反向污染当前意图的判断。和 §4 废除 progress.md 同一思路：要么是真相，要么不存在。

ID 跳号的解释成本由 git log 承担——同事 / 自己想知道「003 怎么没了」时去翻 git history，不靠仓库内文件保留废弃记忆。

副作用明牌：
- 跨文档讨论里出现「咱们去年那条 意图-日程-003」时，得翻 git log——接受
- 新读者看到 ID 跳号会困惑——接受，习惯就好

### 2.1.2 意图标题的命名规范

格式：**`<触发条件>，<可观察结果>`**——两段、中间用中文逗号 `，` 分隔，每段是一句短语。

| 段 | 规则 |
|---|---|
| 触发条件 | 用「X 之后」/「X 时」/「无 X 时」/「X + Y 时」等表条件的句式 |
| 可观察结果 | 用户能感知或磁盘能验的事实，不写抽象判断（如「正确」「成功」）|

**4 条硬规则**：

1. **两段、中文逗号 `，` 分隔**——超过 2 段说明是复合意图，回去拆
2. **不带技术名**——不写 React 组件名、不写 IPC 命令名、不写 DOM selector
3. **≤ 30 字（不含 `意图-<task>-NNN:` 前缀）**——超过说明信息密度低，重写
4. **不用 ✓ ✗ `/` 这类符号当连接符**——这是验收 bullet 的符号，不该出现在标题

**反例 → 正例**：

| ❌ 现行漂移 | ✅ 改正 |
|---|---|
| `日程暂停 + T0+5min 恢复后验证调度未损坏` | 拆 2 条：`日程暂停后，到点不触发` + `日程暂停恢复后，下次到点正常触发` |
| `派活后调用 employee_active_run 返回 Some` | `派活后，员工卡片显示「运行中」状态` |
| `导入 skill 草稿不存在 ✓ 正式目录出现 ✓ 技能中心可见 ✓` | 拆 3 条，各自只测一件事 |

写后自查（§2.5 Layer 2 结构层）增加一项：**意图标题超 30 字 / 含技术名 / 含符号连接 → 重写**。

### 2.2 字段集（3 段，全部必填）

| 段名 | 必填 | 内容 |
|---|---|---|
| **场景** | 是 | 1-3 句话，PM 视角描述用户在做什么、期望看到什么 |
| **操作步骤** | 是 | 编号步骤；agent 从第 1 步顺序跑到最后一步；**第一步永远是 `tauri-pilot aijia health-check` 探活**；后续步骤包含搭环境命令 + 主测操作（详见 §2.3） |
| **验收标准** | 是 | 一组 bullet，逐条列出可观察、可复验的判定项；不拆正反二分或结果二分小段；至少要有 1 条验收项（详见 §2.4） |

**关键设计决定**：
- **不要 Source 字段**——spec 是某时刻快照，会过期；意图本身被人审核通过就是 source of truth。和 TDD 思想一致：测试即规约，不锚到更上游规约。副作用明牌：意图过时靠 agent 跑出 FAIL → 人来判「改实现 / 改意图 / 删意图」，不靠对照 spec。
- **没有「前提」段**——把搭环境步骤和主测操作分开是人为切分，agent 都要顺序跑、没有区别；唯一像"前提"的健康检查作为「操作步骤」第一步固定下来即可。强行分「前提」反而引导作者写声明式「假设 X 存在」，丢掉命令式。
- **没有「判定提示」段**——它原本想塞的 3 类内容各有更好的归宿：边界容忍（如「`nextFireAt` 比 `startAt` 早 1-2 秒是合法误差」）→ 写进验收标准的容忍范围（「在 T0 ± 1 分钟内」）；通用诊断套路（如「先用 `aijia where --json` 看现场」）→ 写进 `test-intents` skill；需要排除的错误状态 → 直接写进验收标准。留判定提示段会鼓励作者偷懒、绕开「验收标准要精确」的要求。
- **「验收标准」内部不再拆小段**——正向结果和需要排除的错误状态都属于同一组判定项，直接列在 `验收标准` 下；没有反向约束时也不写「无」占位。
- **「操作步骤」每一步要么是命令式调用（aijia CLI / rm / mkdir），要么是 PM 视角的用户感知（点击 / 输入 / 等待）**——详见 §2.3 措辞规则。


### 2.3 「操作步骤」段的措辞规则

「操作步骤」是 rules.md 里最容易写偏的一段。它合并了原本的「前提」「操作」两段——agent 顺序从头跑到尾。规则三条：

**a. 一条意图一件事**

由 ID 系统强制：一条 `意图-<task>-<NNN>` 只能有一组「操作步骤」+ 一组「验收标准」。

现行违反实例（须在迁移时拆开）：
- 现 `agenda/rules.md` 意图 4：「暂停... + T0+5min 恢复后验证调度未损坏」→ 拆为 `意图-日程-004`（暂停）+ `意图-日程-005`（恢复后）
- 现 `skill/rules.md` 意图 5：「草稿仍存在 + 正式目录出现 + 技能中心可见 + 新对话 system prompt 含字符串」→ 拆 4 条

**b. 禁用技术术语 / 组件名 / DOM selector**

操作步骤是 PM / agent 都要看懂的，写「用户能感知的页面变化或动作」，不写代码世界的名字。

| ❌ 不要这样写 | ✅ 改成这样 |
|---|---|
| 等待 `AgendaItemEditor` sheet 打开 | 等新建表单展开（能看到「标题」「Prompt」「开始时间」等输入项） |
| 等待 `[data-testid="agenda-editor"]` 出现 | 等新建表单展开（能看到「标题」「Prompt」「开始时间」等输入项） |
| 通过 IPC 调用 `employee_active_run` | 点击员工卡片右上角的「派活」按钮 |
| 触发 `tauri-pilot aijia click` | 点击「保存」按钮 |

**c. 等待信号写用户能看到的 UI 文案**

需要等待某个动作完成时，断言的对象是「用户能看到的 UI 文案 / 区块」，不是技术 ID。agent 加载 `test-intents` skill 后会自己把「能看到 X 文案」翻译成 `aijia ui-message` / `aijia where` 等的具体 poll 逻辑——rules.md 不操心这层。


### 2.4 「验收标准」的书写规则

验收标准是 agent 判 PASS/FAIL 的唯一依据。写不准 = 整套意图测试失去意义。6 条硬规则：

**规则 1 — 每条 bullet 必须可机器观察**

每一条都要让 agent 通过 CLI 命令或文件读取**当场判定**，不允许写「判断成功」「感觉正常」「合理」这种主观词。

| ❌ 模糊 | ✅ 具体 |
|---|---|
| 日程被成功创建 | 日程列表出现一行标题为 `早会提醒` 的条目 + JSON 文件存在 + `status == "active"` |
| 保存成功 | 「新建日程」表单收起 + `agenda-{uuid}.json` 文件存在 |
| 字段值正确 | `title == "早会提醒"`、`status == "active"`（每个字段单独列） |

**规则 2 — 每条 bullet 是一个独立判定单元，不复合**

一条 bullet 只断言一件事。多个事项即使相关也要拆开。

```
❌ 复合：日程已创建且能被 cron 调度（说明 active）
✅ 拆开：
  - 列表出现一行标题为 `早会提醒` 的条目
  - JSON 文件 `status == "active"`
  - JSON 文件 `nextFireAt` 不为 null
```

理由：复合断言一旦 FAIL，agent 不知道是哪部分失败、报告也写不清。

**规则 3 — 用 6 种标准断言形式之一**

合法的断言形式只有这 6 种，超出就要质疑：

| 形式 | 示例 | 用途 |
|---|---|---|
| **UI 出现** | 日程列表出现一行标题为 `早会提醒` 的条目 | 检查用户能看到的页面变化 |
| **UI 消失** | 「新建日程」表单收起 | 检查页面状态切换 |
| **文件存在** | 文件 `~/.renlijia/users/{scope}/agenda/items/agenda-*.json` 存在 | 检查产物落盘 |
| **文件不存在** | `~/.renlijia/users/{scope}/agenda/items/` 下没有以 `tmp-` 开头的文件 | 检查清理 |
| **字段精确匹配** | JSON 中 `status == "active"` | 检查具体值 |
| **字段范围匹配** | JSON 中 `createdAt` 在 `T0 ± 1 分钟` 内 | 检查时间 / 数值容忍范围 |

**规则 4 — 路径用字面值或带变量的明确模式，禁用形容词**

路径必须能让 agent **直接拼出来**，不需要现场推测。

```
❌ 推测：agenda 相关目录下
❌ 推测：用户 scope 的数据目录
✅ 字面：~/.renlijia/users/{scope}/agenda/items/
✅ 模式：~/.renlijia/users/{scope}/agenda/items/agenda-*.json
```

变量只允许以下 4 个开箱可用，其它变量必须在「操作步骤」段先把它从 CLI 输出 / 环境推断出来再用：

| 变量 | 含义 |
|---|---|
| `{scope}` | 当前登录用户的 scope，形如 `t_{tenantId}__u_{userId}`，agent 由 `aijia where --json` 推断 |
| `{tenantId}` | 租户 ID |
| `{userId}` | 用户 ID |
| `T0` | 测试开始时刻（人 / agent 跑前 capture），用于时间断言基线 |

**规则 5 — 字段断言用 5 种运算符**

字段级断言写法须落到下面 5 种之一，让 agent 一眼能机械执行：

| 运算 | 示例 |
|---|---|
| `==` 精确等于 | `title == "早会提醒"` |
| `!=` 不等于 | `occurrenceCount != 0` |
| `not null` / `is null` | `nextFireAt` 不为 `null`；`rule == null` |
| `length == N` / `length >= N` | `participants.length == 1` |
| 时间/数值范围 | `createdAt` 在 `T0 ± 1 分钟` 内 |

```
❌ title 应该是 "早会提醒"（用 ==）
❌ participants 数组不为空（写 length >= 1 或 length == N）
❌ status 字段是合理的（写具体值）
```

**规则 6 — 不要写独立的失败段**

需要排除的错误状态直接作为验收标准 bullet；agent 主动检查「X 不存在 / X 没出现」：

```
✅ JSON 中不含 `personaId` 字段
✅ 日程列表中没有第二行标题为 `早会提醒` 的条目
✅ `agenda/runs/` 目录下没有以 `job-001-` 开头的子目录
```

```
❌ JSON 字段都正确（太笼统，要拆字段）
❌ 不出错（太模糊，agent 不知道查什么）
```

不要另起任何二级小段，也不要写「无」占位。


### 2.5 rules.md 写后自查清单（3 层 13 项硬伤）

§2.2 ~ §2.4 是「怎么写」的正面规则。本节是「写完怎么检」的反面 lint，作用于**整条意图**——「场景」「操作步骤」「验收标准」3 段都要按下面 13 项过一遍，命中任一即返工。

不只是验收标准，意图的任何段都可能犯这些病。

**Layer 1：语义层（每条 bullet / 每句话单独检查）**

| 硬伤 | 例子 |
|---|---|
| 代指不清晰 | 「该条目」「这个文件」——指哪个？换字面路径或字段名（如 `agenda-{uuid}.json`、`title` 字段）|
| 表述模糊 | 「成功」「正常」「合理」「正确」「应该」——agent 无法机械判定，必须落到具体值 / 路径 / 状态 |
| 标准过于宽泛 | 「字段值符合预期」——预期是什么？逐字段列出值；「操作步骤」段写「配置一下表单」——配什么？逐项列 |
| 表述拗口 | 一句话里 3 层嵌套从句——拆成两条 bullet 或重写为短句 |

**Layer 2：结构层（看整段 / 段间关系）**

| 硬伤 | 例子 |
|---|---|
| 非原子化 | 「文件存在且 `status == active`」——文件不存在和 status 错是两件事，拆开 |
| 多个独立约束硬塞一条 | 「保存按钮消失 + 列表出现新条目 + JSON 落盘」——三件事三条 bullet；「操作步骤」段「点击保存并等列表刷新」——也要拆 |
| 冗余 | ✅ 段「列表出现 1 条目」+「列表长度 == 1」——同一断言两种说法；「操作步骤」段重复写应用启动 |
| 包含关系 | 「文件 X 存在」+「文件 X 的 status == active」——后者隐含前者，但保留两条对 agent 报告更清晰；如果后者写成「字段 X 合法」就是真冗余 |
| 标准之间冲突 | ✅ 写 `status == "active"`、❌ 写 `status != "paused"`——逻辑互斥但表面不矛盾；避免冗余反向断言 |
| 格式不规范 | 字段断言不用 `==` 而用「等于」「应该是」；路径不带反引号；字段名不用 camelCase 与实现对齐；UI 文案不用「」括号 |
| 意图标题违规 | 超 30 字 / 含技术名（组件名 / IPC / selector）/ 含 ✓✗`/` 等符号连接 → 重写（详见 §2.1.2）|

**Layer 3：完备性层（看整条意图）**

| 硬伤 | 例子 |
|---|---|
| 遗漏关键约束 | 验收标准只断言 UI 出现条目、不断言 JSON 落盘——前端 mock 出来的假数据也会 PASS；「操作步骤」段没写需要先雇佣员工，实际跑会卡 |
| 包含无关约束 | 验收标准里出现「应用版本 == v0.5.26」——版本不是这条意图管的事；「操作步骤」段写「顺便打开 devtools 看看」——分散注意力 |
| 标准本身错误 | `status == "Active"`（实际枚举值是 lowercase `active`）——和实现对不上；路径写错盘符 |

**返工原则**：命中任一硬伤，**改 rules.md**，不要写「agent 自己判断一下」之类的兜底措辞。意图测试的整个体系就是靠 rules.md 的精确性维系的，模糊会传染。


### 2.6 完整模板（日程 task 示范）

```markdown
## 意图-日程-001: 创建一次性日程后，items/{id}.json 落盘且字段完整

**场景**
用户在日程页点「新建」，填写标题/prompt/组织者员工/未来执行时间，保存后该日程
立即出现在列表中，并以独立 JSON 文件持久化到用户 scope 下。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 从 `tauri-pilot aijia where --json` 推断当前 scope（形如 `t_{tenantId}__u_{userId}`）
3. 清空测试目标目录：`rm -rf ~/.renlijia/users/{scope}/agenda/items/`
4. 确认 `~/.renlijia/users/{scope}/employees/` 下有 `default` 员工 record（无则用 `tauri-pilot aijia` 创建一个）
5. 点击主侧边栏「日程」入口
6. 点击「新建日程」按钮；等新建表单展开（能看到「标题」「Prompt」「开始时间」等输入项）
7. 在「标题」输入 `早会提醒`
8. 在「Prompt」输入 `提醒我今天的三件事`
9. 在「组织者员工」下拉选择 `default`
10. 在「开始时间」选择本地时间 `T0 + 30 分钟`
11. 在「频率」选择 `一次性`
12. 「时区」保持默认 `Asia/Shanghai`
13. 点击「保存」；等表单收起（页面回到日程列表）

**验收标准**

- 表单收起后，日程列表出现一行标题为 `早会提醒` 的条目
- 目录 `~/.renlijia/users/{scope}/agenda/items/` 存在
- 该目录下有恰好 1 个文件，文件名形如 `agenda-{uuid}.json`
- 该文件是合法 JSON，且：
  - `title == "早会提醒"`
  - `prompt == "提醒我今天的三件事"`
  - `organizerEmployeeId == "default"`
  - `participants.length == 1`，第 0 项 `employeeId == organizerEmployeeId`
  - `timezone == "Asia/Shanghai"`
  - `rule == null`
  - `status == "active"`
  - `occurrenceCount == 0`
  - `nextFireAt` 不为 null，等于 `startAt`
  - `createdAt` 和 `updatedAt` 在 `T0 ± 1 分钟` 内
- JSON 含 `personaId` / `organizerPersonaId` 旧字段名（说明实现还在写老字段）
- 日程列表出现 2 条标题为 `早会提醒` 的条目（说明保存被点了两次）

```

### 2.7 现行 12 份 rules.md 的迁移策略（已敲定）

- 本次只**定模板 + 1 份示范**（agenda 改造作为参考实现），其它 11 个 task（auth / digital-employee / skill / persona / workspace / settings / channel-dingtalk / expert-team / pending-queue / project-memory-tool / search-tool）**暂不批量改**
- 后续何时改：每次有人去做某个具体 task 的意图测试执行时，顺手把该 task 的 rules.md 按新模板重写一遍
- chat-turn-boundary / cancellation-e2e / llm-provider-routing / streaming-e2e 这几份「已退化为后端事件序列断言」的，新模板下应**改判为 L2 集成测试**（见 §5.5 C 类）

---

## 3. 执行环境约定（已敲定）

### 3.1 账号 / scope 来源 = 复用当前登录用户

- agent 跑意图测试时，应用里 **已经登着** pzc / 同事自己的账号
- rules.md 「操作步骤」段从 `aijia where --json` 推断当前 scope（形如 `t_{tenantId}__u_{userId}`）
- **不约定专用测试账号**，不写共享 keychain entry

### 3.2 数据隔离策略 = 不隔离

- 意图测试**直接在真实 `~/.renlijia/` 上跑**，不重定向、不开 `AIJIA_TEST_MODE`、不改 `lib.rs` 启动逻辑
- 意图测试产生的真实对话 / 真实 LLM 调用 / 真实日程 / 真实文件**会落到你账号下**——这是有意的设计
- 要纯净测试环境，**开新电脑 / 新用户重新登**，不靠代码做 sandbox
- 命名约定（如 `e2e-test-` 前缀）是 `aijia cleanup-test-sessions` 的辅助工具，不是隔离机制

理由：
- 隔离逻辑写进 prod 代码 = 一份代码两套行为，意图测试反而失去"真实"含义
- agent 在真实环境跑，最贴近"用户实际用这个产品"
- 失败时现场就是真实工作环境，不需要 reproduce 步骤

### 3.3 跑完是否清理 = 不清理

- rules.md「操作步骤」段在**跑前清理**所需空间（例 `rm -rf ~/.renlijia/users/{scope}/agenda/items/`），跑完不动
- 失败现场保留——agent 写 progress.md 时可以引用具体文件路径，pzc / 你可以直接打开
- 下次跑同一条意图时「操作步骤」段会再清一次，幂等

### 3.4 整体含义

意图测试 = "在 pzc 真实开发机的真实账号下，agent 跑一遍真实操作，留下真实痕迹"——这是这套方法论的环境契约。任何想"为了测试不污染我的数据"的需求都不满足，要满足请去新电脑跑。

### 3.5 「操作步骤」段允许使用的命令（白/黑名单）

意图作者 / agent 在「操作步骤」段写出来的命令会**真的被执行**。看起来合理但很危险的命令很常见——必须给一份硬清单。

**白名单：以下类别允许**

| 类别 | 示例 |
|---|---|
| 删除 scope 内**具体子目录** | `rm -rf ~/.renlijia/users/{scope}/agenda/items/` |
| 删除 scope 内**具体文件 / glob** | `rm ~/.renlijia/users/{scope}/agenda/items/agenda-*.json` |
| 列目录 / 检查存在 | `ls`, `test -d`, `test -f`, `stat` |
| 创建 scope 内**具体子目录** | `mkdir -p ~/.renlijia/users/{scope}/agenda/notes/` |
| 写测试 fixture 文件到 scope 内具体路径 | `echo "test content" > ~/.renlijia/users/{scope}/agenda/notes/test.md` |
| `tauri-pilot aijia` CLI 子命令 | `tauri-pilot aijia health-check`、`tauri-pilot aijia cleanup-test-sessions --prefix e2e-test-` |

**黑名单：以下类别禁止**

| 类别 | 例子 | 理由 |
|---|---|---|
| 操作 `~/.renlijia/` 根目录 | `rm -rf ~/.renlijia/` | 毁掉整个账号数据 |
| 操作家目录 / 系统目录 | `rm -rf ~/`、`rm -rf /tmp/*` | 影响超出意图测试范畴 |
| 操作 git 仓库 | `git reset --hard`、`git clean -fd`、`git checkout .` | 可能丢未推送代码 |
| 启停应用 / 系统进程 | `killall AIjia`、`pkill -9 ...` | 应用启停由 agent 通过 health-check / 重启脚本控制，不在 rules.md 里 |
| 任何 `sudo` 命令 | `sudo rm ...`、`sudo killall ...` | rules.md 不应需要权限提升 |
| 网络操作 | `curl`、`wget`、`ssh ...` | 意图测试是本地 e2e，不允许外网调用 |
| 修改环境变量 / shell profile | `export AIJIA_TEST_MODE=1`、`source ~/.zshrc` | 与 §3.2「不隔离」冲突，且影响超出测试范畴 |

**崩溃恢复 task 的例外**

`崩溃恢复/rules.md` 需要外部 kill 进程模拟 crash，是黑名单的合法例外：

- 该 task 的「操作步骤」段允许 `kill <pid>` / `pkill AIjia` 这类进程操作命令
- 但每条用到这类命令的意图必须在「操作步骤」段开头加一行警告：
  > ⚠️ 本意图需要 kill 应用进程，agent 跑前要先确认用户当前无未保存工作（在对话里问一句）
- 其它 12 个 task 一律禁用进程操作

**违反白/黑名单 = lint 失败**

写后自查清单（§2.5）的 Layer 3 完备性硬伤新增一条：「操作步骤」段命中黑名单 → 必须改为白名单内的等价命令，或拆掉该步骤。

### 3.6 跨意图依赖 = 禁止引用，「操作步骤」段命令式自给自足

意图之间**不允许**有顺序依赖。`意图-日程-003`「暂停的日程不再触发」这种**显然需要先有一条 active 日程**的场景，处理方式是把「创建一条 active 日程」写进 `003` 自己的「操作步骤」段最前面，**不引用** `意图-日程-001`。

**禁止的写法**：

```
操作步骤：
1. 先成功跑过 意图-日程-001 的「操作步骤」段
2. 不需要清理，直接接着跑
3. ...（后续主测动作）
```

**正确的写法**：

```
操作步骤：
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断 scope（形如 `t_{tenantId}__u_{userId}`）
3. 清空 `~/.renlijia/users/{scope}/agenda/items/`：`rm -rf <path>`
4. 用 `tauri-pilot aijia` 创建一条 active 日程：标题 `测试用日程`、cron `* * * * *`、组织者 `default`
5. 等待该日程出现在日程列表里（约 1-2 秒）
6. ...（后续主测动作，如点暂停、断言不再触发）
```

理由：

- **每条意图原子可独立跑**——agent 不需要按 ID 顺序调度；FAIL 时也不会触发"前置 FAIL → 后续全 SKIP"的连锁
- **「操作步骤」段重复写「创建日程」是有意为之**——它强迫意图作者承认「测试这件事需要这个状态」；不可见的依赖链最危险
- **「操作步骤」段允许调 aijia CLI 创建数据**——CLI 也是 §3.5 白名单允许的，agent 跑得起来；rules.md 里能直接命令式描述要搭出来的状态

**副作用明牌**：

- 12 个意图都要在 active 日程下测 → 12 个意图的「操作步骤」段都重写一遍「创建 active 日程」
- 接受这个冗余；rules.md 写一次跑很多次，写作成本可以接受
- 如果某类前置（如"创建 active 日程"）真的重复到难受，未来再考虑加 `tauri-pilot aijia create-agenda --fixture` 这种"专为测试用"的 CLI 一句话搞定——这是 CLI 层的优化，rules.md 形态不变

---

## 4. 执行结果记录 = 不持久化（已敲定）

### 4.1 废除 progress.md

新方法论下**不再有** `spec/tasks/<task>/progress.md`。

agent 跑完意图测试，**直接在对话里向调起者（你 / 同事）输出结构化报告**，不落盘到仓库。

报告格式（agent 在对话里给你看的）：

```markdown
# 日程意图测试报告 — 2026-05-20 14:30

**应用版本**: v0.5.26
**登录账号**: pzc@renlijia.com（scope `t_xxx__u_yyy`）
**本轮跑了**: 意图-日程-001, 意图-日程-002, 意图-日程-003

## 意图-日程-001: 创建一次性日程后落盘 — ✅ PASS（45s）
- 操作按 rules.md 9 步执行无异常
- 验收标准：表单收起 ✓ / 列表出现条目 ✓ / `agenda-uuid.json` 字段全对 ✓ / 无旧字段名 ✓ / 列表无重复 ✓
- 备注：无

## 意图-日程-002: 一次性日程到期自动触发 — ❌ FAIL（120s）
- 操作按 rules.md 执行到第 7 步「等待 30 分钟」时 wait timeout
- 实际现象：到点 +60s 仍未触发，`agenda/runs/` 目录无新文件
- 判断：真 bug，不是误判
- 建议：开 issue 调查 agenda runner 调度逻辑

## 意图-日程-003: 循环日程多次触发 — ⏭️ SKIPPED
- 因 意图-日程-002 失败，依赖同一调度路径，跳过
```

### 4.2 不持久化的核心理由

| 顾虑 | 反方论证 |
|---|---|
| agent 想知道上次失败原因 | **反而有害**——会带预设跑这次。agent 必须每次按 rules.md 客观跑，不"参考"历史 |
| 人想查历史 | git log + 对话记录已够；多一份 progress.md = 多一份要同步的状态 |
| 团队需要权威结果记录 | 意图测试结果本就有时效性（LLM 抖动、环境差异），权威记录是错误期望——要看"现在通不通"就**现在跑一次** |

### 4.3 副作用明牌

- **同事 A 跑出 FAIL 后，同事 B 不知道**——可能再跑一遍踩同样的坑。接受这个成本；如有重大失败要协作，靠 IM / issue 走人际沟通，不靠仓库内文件
- **没法回答"agenda 这个 task 上次跑成啥样"**——回答是"不知道，要看现在跑一遍"
- **没法做"这周 vs 上周通过率"统计**——意图测试不为统计存在，统计需求另外想

### 4.4 落地后果

- 删除现行 26 个 `spec/tasks/<task>/progress.md` 文件
- `context/how-to-test.md` 里"测试结果写入 progress.md" 这条规则作废
- CLAUDE.md 第 4 节里 "test-progress.md — 执行记录（通过/失败/坑）" 这条删除

---

## 5. Task 列表（已敲定）

### 5.1 切分维度 = 按 PM 视角的产品 feature

意图测试 task = 按用户能感知到的产品 feature（顶栏 / 侧栏 / 主功能模块）切。一个 task 一个 `rules.md`。

理由：
- 意图边界已定为"产品承诺"（§1.2），天然按 feature 维度组织
- agent 接到「跑日程 task」时直接对应 UI 入口，不需要架构知识

### 5.2 新增 / 删除 / 合并 task 的规则

**一个 task = 一个用户可独立完成的产品工作面**。

**判定一个东西要不要单独立 task，看以下 2 条标准**（**全部满足**才开新 task）：

1. **它在 UI 上有独立入口**——顶栏 tab / 侧栏菜单 / 独立子页面，用户能在脑里说出「我去 X 里做 Y」
2. **它有独立的核心承诺**——背后的产品故事不会被现有 task 完全覆盖

不设「至少 N 条意图」门槛：单条意图的微型 task 也允许存在，只要它满足上面两条。

**新增 task 的触发场景**：

- 新功能发版上线，UI 上有独立入口（如未来加「AI 报表」成为独立侧栏入口）
- 现有 task 内的某子领域明显是「两件事粘在一起」，拆出去
- 跨多个现有 task 的横切承诺（如「数据导出」涉及日程/数字员工/工作空间），且独立到无法塞进任一现有 task

**删除 task 的触发场景**：

- 该 feature 被产品下线（如 `MEMORY.md` 提示的 persona 准备废弃 → 「人格」task 同步删）
- 整个 task 退化为后端事件序列断言，无 PM 视角承诺 → 整 task 砍掉（§5.5 B/C 类即此原则）

**合并 task 的触发场景**：

- 两个 task 实际是同一个用户工作面的不同切片，合并（目前无此案例）

**不按文件大小拆 task**：

`rules.md` 单文件**无大小上限**——拆 task 只看「是不是同一个用户工作面」，不看文件多大。

**集中放一份是发现问题的基础**：50 条意图都在一份 `rules.md` 里，grep / 通读 / 对比才能发现「这两条意图测的是同一件事」「这两条冲突」「这条意图覆盖了那条的子集」。拆成 5 份小文件后，同 task 内的冗余 / 冲突反而看不出——§2.5 自查清单 Layer 2「冗余 / 包含关系 / 冲突」这 3 项 lint 强依赖于「同 task 意图集中在一处」才能跑。

- 30 条意图的 `rules.md` 不需要拆
- 100 条意图的 `rules.md` 也不需要拆
- agent 跑意图时按 §5.5 流程一次只读一条意图相关内容，token 不会爆
- 如果意图涨到 100+ 条让你直觉「这 task 是不是太大」，回到 §5.2 的 2 条标准重新审「这是不是一个工作面」——是就留、不是就拆产品边界（不是拆文件）

### 5.3 命名规范

- **中文目录名**，对应用户用语（不是工程内部代号），禁用英文 task 名
- **2-5 个汉字**，能在一句话里自然嵌入（如「跑一下『日程』」）
- 优先复用 UI 上呈现的文案——UI 上叫「日程」，task 就叫「日程」，不叫「日程管理」

### 5.4 决策流程

新增 / 合并 / 删除 task 不能单方面拍板——任何人想动 task 列表时：

1. 在对话里向 agent 提议，agent 加载 `test-intents` skill 后按 §5.2 规则判定
2. 判定不通过 → agent 给出原因；通过 → agent 修改目录结构 + 在 commit message 里说明触发了哪条规则
3. 修改在对话里给作者 review，作者认可后 agent commit

### 5.5 意图的写作 / review 流程 = 人 review，不走 PR

**单条意图的新增 / 修改 / 删除**：

1. 在对话里向 agent 提议（如「给日程加一条意图：暂停后到点不触发」）
2. agent 加载 `test-intents` skill，按 §2.1.2 标题规范 + §2.4 验收书写规则 + §2.5 写后自查清单写出 rules.md diff
3. agent 在对话里把 diff 展示给作者
4. 作者 review **承诺方向**（这是产品承诺吗？该写在这个 task 吗？和现有意图冲突吗？）—— `§2.5` 形式 lint 由 agent 自己执行，作者不重复检查
5. 通过 → agent commit；不通过 → 作者反馈修改点，agent 重写循环

**不走 PR 的理由**：
- 单条意图改一行也开 PR 太重
- 形式合规由 agent 按 §2.4 / §2.5 自检即可，不需要二次机器 review
- 承诺方向只能人判，对话即时 review 比 PR 慢回复快
- 仓库本身已 git 受控，commit 一旦推上去同事拉就有，不需要 PR 做"公示"

**仅承诺方向是人 review 的核心**：作者不需要逐条核对 §2.4 6 条规则、不需要数 §2.5 13 项硬伤——agent 在 diff 之前已经过了形式自检。作者只看：「这条产品承诺写得对吗 / 该测吗 / 是否多余」。

**与 §5.4 task 层级流程的区别**：task 层级动（开新 task / 删 task / 合 task）是结构性变动，影响更大；走和意图层级一样的「对话 review + agent commit」，但触发条件更严（必须先过 §5.2 的 2 条标准判定）。

### 5.6 现行 29 个 task 处理方案（已敲定）

**A 类 / 保留为意图测试（13 个，含 1 个重写）**

| 现行目录 | 新目录 | 备注 |
|---|---|---|
| `agenda/` | `日程/` | 重命名 + 按新模板（§2.4）重排作为示范 |
| `auth/` | `登录/` | 重命名，rules.md 内容暂不动 |
| `channel-dingtalk/` | `钉钉频道/` | 重命名，rules.md 内容暂不动 |
| `digital-employee/` | `数字员工/` | 重命名，rules.md 内容暂不动 |
| `expert-team/` | `专家团队/` | 重命名，rules.md 内容暂不动 |
| `pending-queue/` | `待办队列/` | 重命名，rules.md 内容暂不动 |
| `persona/` | `人格/` | 重命名，rules.md 内容暂不动；**注**：`MEMORY.md` 提示 persona 准备废弃，若实施则同步删除本 task |
| `project-memory-tool/` | `项目记忆/` | 重命名，rules.md 内容暂不动 |
| `search-tool/` | `搜索/` | 重命名，rules.md 内容暂不动 |
| `settings/` | `设置/` | 重命名，rules.md 内容暂不动 |
| `skill/` | `技能/` | 重命名，rules.md 内容暂不动 |
| `workspace/` | `工作空间/` | 重命名，rules.md 内容暂不动 |
| `persistence-crash-recovery/` | `崩溃恢复/` | 重写为 PM 视角承诺（「应用崩了再开，对话历史 / 草稿 / 任务都还在」），L4 通过外部 kill 进程模拟 crash |

「rules.md 内容暂不动」= 内容暂保留现样，等执行时各 task 自然按新模板（§2.4）迁移。本次仅强制改造日程（示范）+ 崩溃恢复（重写）两份。

**B 类 / 删除（12 个，搬去 L2）**

`masking-level` / `memory-service` / `memory-turn-injection` / `permission-ask-flow` / `permission-pipeline` / `permission-store` / `session-runtime` / `subagent-execution` / `subagent-lifecycle` / `async-subagent-lifecycle-order` / `tool-round-concurrency` / `agent-markdown-loader`

这些是后端架构约束，不是产品承诺。删除 `docs/test-intents/spec/tasks/<name>/` 整个目录（含 rules.md / progress.md），但**保留 `src-tauri/tests/review_*.rs`** ——它们仍是有效的架构约束回归测试，跑 cargo 不变。

**C 类 / 删除（4 个，事件序列断言）**

`cancellation-e2e` / `chat-turn-boundary` / `llm-provider-routing` / `streaming-e2e`

名字像 e2e、内容是 EventBus 事件序列断言，已退化为后端测试规约。删除目录；如果对应 `src-tauri/tests/` 有测试文件，作为 L2 review 测试保留。

### 5.7 最终意图测试 task 列表（13 个）

```
docs/test-intents/spec/tasks/
├── 日程/                rules.md
├── 登录/                rules.md
├── 钉钉频道/            rules.md
├── 崩溃恢复/            rules.md   # 重写自 persistence-crash-recovery
├── 数字员工/            rules.md
├── 专家团队/            rules.md
├── 待办队列/            rules.md
├── 人格/                rules.md
├── 项目记忆/            rules.md
├── 搜索/                rules.md
├── 设置/                rules.md
├── 技能/                rules.md
└── 工作空间/            rules.md
```

每个目录只有 `rules.md`——progress.md 已废除（§4），不再有 README / context / 其它文件。

### 5.8 本次 spec 落地的迁移动作

1. 删除 16 个非 A 类 task 目录（B 类 12 + C 类 4）
2. 12 个 A 类 task 目录改中文名（见 §5.5 表格）
3. `persistence-crash-recovery/` 改名为 `崩溃恢复/`，rules.md 重写为 PM 视角
4. 12 个 A 类 task 的 progress.md 全部删除
5. `日程/rules.md` 按新模板（§2.4）重排作为示范
6. 其余 11 个 A 类 task 的 rules.md 内容**暂不动**，等执行时各 task 自然迁移

---

## 6. 已排除的方向（决策痕迹）

- ❌ 「意图测试 = L2 + L4 双轨」——增加方法论复杂度，rules.md 同一个产物被两层抢
- ❌ 「确定性 fixture LLM（mock provider 回放）」——违反「真 LLM」原则
- ❌ 「LLM-as-Judge 自动判内容」——agent 已经有判断力，不需要再加一层 judge
- ❌ 「把 rules.md 喂给 LLM 自动生成 cargo 测试」——意图测试不在 cargo 层
- ❌ 「rules.md 加 Source 字段引用 spec」——spec 是某时刻快照、会过期；测试即规约
- ❌ 「为意图测试加 AIJIA_TEST_MODE 等环境隔离开关」——不为测试改 prod 启动逻辑，要净化跑环境去新电脑
- ❌ 「保留 progress.md 服务 agent 启动读上次结果」——会让 agent 带预设跑这次，反而有害
- ❌ 「保留 progress.md 服务人查历史」——git log + 对话记录已足够，多一份要同步的状态

---

## 7. CLI 与 rules.md 的关系（已敲定）

### 7.1 rules.md 纯 PM 视角，禁止内嵌 CLI 命令

rules.md 「操作」段写产品视角的步骤（「点击新建按钮」「输入标题 X」），**禁止**写 `tauri-pilot aijia create-agenda --title="..."` 这种命令式调用。

理由：
- rules.md 是规约，不是脚本。脚本会因 CLI 接口微调而坏，规约不应该跟着抖
- 写成 PM 视角才能让 PM / 同事审核「这条意图是否真是产品承诺」
- 一条意图换 CLI 命名、加参数，rules.md 不动；反过来如果 rules.md 嵌了 CLI，每次 CLI 改名就要改 N 份 rules

### 7.2 CLI 知识 = skill，不是文档

**不再维护 `docs/test-intents/cli-reference.md` 类独立 CLI 手册**。

`tauri-pilot aijia` 的 16 个子命令、稳定性边界、错误处理约定收敛到 **`test-intents-runner`** skill 的「CLI 工具箱」章节（见 §8.4），agent 跑意图测试时**自动加载**该 skill。

skill 内容（不在本 spec 范围内具体列，由实施 plan 负责）：
- 16 个子命令清单 + 语义 + 参数 + 返回结构
- 已知边界（如 `aijia screenshot` 30s 超时绕道 `tauri-pilot screenshot` 直跑）
- 失败诊断套路（先 `aijia where --json` 看现场再判定）
- 「禁直接 click/eval，必须走 aijia 子命令」的铁则（来源：`MEMORY.md project_e2e_testing_tauri_pilot.md`）

这样的好处：
- agent 通过 skill 系统加载，不需要 rules.md / 方法论文档显式说「请先读 X 再跑」
- CLI 演进时改一份 skill，不影响 rules.md
- 业务级 CLI 子命令（如未来 `aijia create-agenda`）登记在 skill 里就够，不需要每份 rules 复述
- 同事换电脑只要拉取 skill 库就拿到完整意图测试工具知识，不用翻仓库文档

### 7.3 业务级 CLI 子命令的登记位置

未来发现 rules.md 操作步骤无法用现有 16 个 CLI 表达（如「暂停日程」没有对应 CLI），需要新增 `aijia` 子命令时：

1. 在 `tauri-plugin-pilot` 子仓加新子命令实现
2. 在 `test-intents` skill 的「CLI 工具箱」章节登记这个新命令
3. **rules.md 不变**——意图本身不变，只是 agent 现在能更顺地把 PM 步骤翻译成 CLI 调用

---

## 8. 文档与 skill 结构（已敲定）

### 8.1 核心方向：文档退场，3 个项目级 skill（入口 + author + runner）

**意图测试不再依赖任何 `how-to-*` 类方法论文档**。所有「怎么写 / 怎么跑 / 用什么 CLI 工具」的知识全部收敛到**一个仓库级 skill `test-intents`**（git 受控，跟随 clone 流动）。

理由：
- 文档需要主动读，「忘记读」是常态——agent 可能直接跳过去翻 rules.md，错过方法论
- skill 在触发场景命中时**自动加载**，是 agent 行为的默认背景知识
- 项目级 skill（仓库内 `.claude/skills/<name>/SKILL.md`，Claude Code 在该仓库 session 内自动加载）比用户级（`~/.claude/skills/`）更适合：同事 clone 仓库就拿到、与 rules.md 同步演进、有 git 历史
- **3 个 skill 分层而非单一大 skill**：入口 `usertest-intents` 极薄、只做路由 + 一句话定义；实现层拆 `test-intents-author`（写）+ `test-intents-runner`（跑）。理由：用户咨询场景下加载薄入口即可；写和跑的方法论体量大、agent 关注点不同，分开减少不相关内容污染 context。`.gitignore` 已加例外让 `.claude/skills/` 入仓
- 仓库已有先例：`docs/skills-migration/` 下放了 ~30 个 SKILL.md，路径方案直接复用

### 8.2 docs/test-intents/ 最终形态

```
docs/test-intents/
├── README.md                              # 极薄入口（≤ 50 行）：定义 + 13 个 task 一行简介 + 指向 .claude/skills/
└── spec/tasks/
    ├── 日程/rules.md
    ├── 登录/rules.md
    ├── 钉钉频道/rules.md
    ├── 崩溃恢复/rules.md
    ├── 数字员工/rules.md
    ├── 专家团队/rules.md
    ├── 待办队列/rules.md
    ├── 人格/rules.md
    ├── 项目记忆/rules.md
    ├── 搜索/rules.md
    ├── 设置/rules.md
    ├── 技能/rules.md
    └── 工作空间/rules.md
```

每个 task 目录**只**含 `rules.md`。

**注意：skill 不在 `docs/test-intents/` 下**——Claude Code 项目级 skill 的标准位置是仓库根目录的 `.claude/skills/<name>/SKILL.md`。本项目通过 `.gitignore` 例外让 `.claude/skills/` 入仓，三个 skill 实际落地：

```
.claude/skills/
├── usertest-intents/SKILL.md      # 用户级入口 + 路由
├── test-intents-author/SKILL.md   # 写意图方法论
└── test-intents-runner/SKILL.md   # 跑意图方法论 + CLI + 经验库
```

### 8.3 删除清单（5 份方法论 + 26 份 progress）

| 文件 | 处理 | 理由 |
|---|---|---|
| `docs/test-intents/context/context.md` | 删除 | 业务规则不是意图测试管的事，归宿在对应业务 spec |
| `docs/test-intents/context/capabilities.md` | 删除 | 前半 Mock 已作废；后半 CLI 进 `test-intents` skill |
| `docs/test-intents/context/how-to-test.md` | 删除 | 规定的 cargo 命名 / 执行规则属 L2 范畴 |
| `docs/test-intents/context/how-to-write-rules.md` | 删除 | 内容抽到 `test-intents` skill |
| `docs/test-intents/README.md` | 重写为极薄入口 | 现行内容已与方法论冲突 |
| `docs/test-intents/spec/tasks/*/test-progress.md` × 26 | 全删 | progress 已废除（§4） |

### 8.4 3 个 skill 的职责切分

| skill | 触发场景 | 职责 |
|---|---|---|
| `usertest-intents` | 「意图测试」/「AEIT」/ 纯咨询 / 用户意图模糊时 | 极薄入口（< 150 行）：一句话定义 + 13 个 task 清单 + 路由到 author/runner |
| `test-intents-author` | 「加一条意图」/「改 X task 的 rules」/「拆复合意图」/「删 意图-XXX-NNN」 | 写 / 改 / 删意图的完整方法论：ID 体系、标题命名、字段集、措辞规则、验收 6 条、自查 13 项、命令白黑名单、跨意图禁引用、review 流程、新建 task 判定 |
| `test-intents-runner` | 「跑一下 X 这个 task」/「跑 意图-XXX-NNN」/「FAIL 怎么处理」/「`tauri-pilot aijia ...`」 | 跑意图的完整方法论：怎么读 rules.md、执行语义、报告格式、CLI 工具箱 16 子命令、已知 quirks 经验库、环境契约 |

`usertest-intents` 是 user-facing 入口，**纯咨询场景**用它即可；**写或跑**时由它路由到对应实现 skill

下面是 runner / author 两个实现 skill 的 body 大纲（具体内容由 implementation plan 写；usertest-intents 只用 §1 + §3 task 清单 + §5）：

1. **意图测试是什么**（§1 完整搬过来）—— L4 only / 产品承诺 / Agent 自评 / 触发时机
2. **怎么读 + 跑一条意图**
   - 读 rules.md 3 字段的语义
   - 按「操作步骤 → 验收」顺序执行（操作步骤的第一步必定是 health-check）
   - 报告输出格式（§4.1）
   - 真 FAIL vs 误判：核对验收标准的 ✅/❌；遇到模糊处去查第 7 章「执行经验库」
   - **中途某步挂了的处理**：
     - **判断原则**：用产品视角现场判断「后续步骤是否还有意义」——不机械规则
       - 后续步骤的前提条件因这一步失败而不成立（如点不开侧栏 → 后续点不到任何东西） → **跳过后续步骤**
       - 后续步骤可独立（如某个等待 timeout 但实际状态可能已 OK） → **继续跑后续步骤**
     - **验收永远要跑**——挂在哪一步都要核验 ✅ / ❌ 段，看现场磁盘 / UI 状态。即便步骤都没跑到测的核心动作，验收能反映「现在系统处于什么状态」，对 triage 价值高
     - **报告强制区分 FAIL 主因**（agent 自己判，记进报告便于人 triage）：
       - `FAIL 主因 = rules/CLI 问题`：步骤本身写错 / CLI 接口变了 / selector 找不到——意味着 rules.md 或 `aijia` CLI 要改
       - `FAIL 主因 = 产品 bug`：被测应用真的卡住 / 状态错乱——意味着产品代码要改
       - 不确定时写 `FAIL 主因 = 待 triage`，请人判
   - **跑全 task（一次跑这个 task 下所有意图）的执行语义**：
     - **串行**——一次只跑一条意图，不并发
     - **失败不串联**——某条意图 FAIL 不影响后续意图继续跑；agent 必须把 task 下所有意图全部跑完才出报告
     - **每条意图独立可跑**已由 §3.6 保证（意图自给自足、无跨意图依赖），所以前一条 FAIL 不会污染后一条的前置状态
     - 报告里按 ID 升序逐条列结果（PASS/FAIL/SKIPPED），不汇总成 task 级别的「整体 PASS/FAIL」——意图测试不做总分
   - **不分失败等级**——FAIL 就是 FAIL，不分 P0 / P1 / P2；轻重由人看报告内容判断，不由 rules.md / agent 预先打标
   - **不规定 FAIL → issue 流程**——FAIL 之后开不开 issue / 改实现 / 改 rules，由人按 FAIL 主因 + 自身判断处置；意图测试只产报告，不联动外部 issue 系统
3. **怎么写 / 改一条意图**
   - 意图边界（§1.2）
   - ID 体系（§2.1）+ 不回收规则
   - 3 字段模板（§2.2）+ 「一条意图一件事」
   - 完整示范（§2.4）
   - 自查清单（≈ 10 条）
4. **CLI 工具箱：`tauri-pilot aijia` 16 子命令**
   - 命令清单 + 语义 + 参数 + 返回结构
   - 已知边界（`aijia screenshot` 30s 超时绕道 `tauri-pilot screenshot` 直跑）
   - 铁则：禁直接 click/eval，必须走 aijia 子命令
5. **环境契约**（§3）—— 不隔离 / 不清理 / 复用真账号
6. **不持久化**（§4）—— progress.md 已废除，结果在对话里输出
7. **执行经验库**（**这一章持续积累、由 PR 持续追加**）
   - **定位**：rules.md 是单条意图的精确规约；经验库是跑了很多意图后沉淀的横切知识。属于 skill 不属于 task。
   - **包含什么**：
     - 通用失败诊断套路（如：点「保存」后表单没收起 → 先用 `aijia where --json` 看是否有对话框阻塞，再判 FAIL）
     - 跨意图反复出现的边界容忍判定原则（如：跨时区时间字段的合法漂移范围）
     - LLM 抖动 / 网络抖动的常见伪 FAIL 形态及甄别方法
     - `aijia` CLI 命令在不同应用状态下的已知 quirks
   - **不包含什么**：
     - 单条意图特有的边界（写进该意图的 ✅/❌ 段）
     - 单次失败的具体诊断（写进对话里的报告，不沉淀回 skill 除非反复出现）
   - **沉淀触发**：agent 跑意图时遇到一个新陷阱 / 新诊断套路 / 新容忍判定，**在报告末尾建议「这条经验值得沉淀到 skill 第 7 章」**，由人决定 PR 加进来

### 8.5 README.md 唯一保留的仓库文档

整个 `docs/test-intents/README.md` 控制在 50 行以内，包含 4 件事：

1. **一句话定义**：意图测试 = AEIT (Agent-Executed Intent Test) = agent 在真实环境跑 L4 e2e、自评 PASS/FAIL
2. **触发**：在对话里说「跑一下 X 这个 task」/「加一条意图」即可——agent 自动加载 `test-intents` skill
3. **13 个 task 一行简介**
4. **指向 skill**（仓库内路径，不复述内容）

README 不讲方法论 / 不讲怎么写 / 不讲怎么跑——那些事归 skill。

---

## 9. CLAUDE.md 第 4 节处理（已敲定）

### 9.1 整段删除

CLAUDE.md 第 280-290 行「意图测试框架（test-intents）」节**完整删除**，不替换为新版本。

理由（贯彻 §8.1 文档退场原则到底）：
- skill 在触发场景命中时自动加载——agent 拿到「意图测试」/「跑 X task」/「加一条意图」这种触发词，会直接命中 `test-intents` skill
- CLAUDE.md 里留个指针段 = 冗余指针：方法论已经在 skill description 里有了，CLAUDE.md 再复述一遍只会增加同步成本和漂移风险
- 仓库内"意图测试 = 什么 + 怎么用"的入口已经在 `docs/test-intents/README.md`（50 行极薄入口），CLAUDE.md 不需要重复指向

### 9.2 不主动加任何替代指引

不在 CLAUDE.md 其它位置加「想跑意图测试请说 X」这种 hint——agent 通过 skill 系统自然命中，不靠 CLAUDE.md 推送。

---

## 10. 设计原则总结（决策痕迹）

把整份 brainstorm 串起来的一条主线是：**让 agent 在被调起那一刻就拿到所有需要的知识，而不是依赖人/agent 主动去读文档。**

具体在每个决策点的体现：

| §  | 决策 | 体现的原则 |
|---|---|---|
| §1.1 | 意图测试 = L4 only | 范畴边界单一，agent 接到「跑意图」就知道用 e2e 跑 |
| §1.2 | 边界 = 产品承诺 | 让 PM / agent 都能读懂，不需要架构知识 |
| §1.3 | Agent 自评 | 验收不依赖机器自动 judge，agent 现场判 |
| §2.1 | ID 体系 + 不回收 | 跨文档/git 引用稳定，agent / 人都能 grep |
| §2.2 | 不要 Source 字段 | 不锚到会过期的上游 spec |
| §3.x | 不隔离 / 不清理 / 复用真账号 | 不为测试改 prod 启动逻辑，把环境复杂度移出代码 |
| §4 | 废除 progress.md | 不让历史结果误导 agent，结果在对话里输出即够 |
| §5 | 13 个 task 中文名 | 直接对应 PM 视角，agent 不要做翻译 |
| §7 | rules.md 纯 PM + CLI 进 skill | rules 不嵌脚本，CLI 演进不打扰 rules |
| §8 | 全部走仓库级 skill | 文档退场，skill 自动加载 |
| §9 | CLAUDE.md 第 4 节整段删 | 连顶层指针都不要 |

---

## 11. 本 spec 的实施清单（给 implementation plan 用）

按本 spec 落地需要 implementation plan 安排以下 10 项：

1. **删除** `docs/test-intents/context/` 整个目录（4 份方法论文档）
2. **删除** 16 个非 A 类 task 目录（B 类 12 + C 类 4）
3. **删除** 12 个 A 类 task 的 progress.md（含全删的 26 份 progress.md）
4. **重命名** 12 个 A 类 task 目录为中文名（见 §5.6 表格）
5. **重命名** `persistence-crash-recovery/` 为 `崩溃恢复/`
6. **重写** `崩溃恢复/rules.md` 为 PM 视角（L4 调外部 kill 模拟 crash）
7. **重写** `日程/rules.md` 按 §2.6 新模板（作为 13 份 rules 的示范）
8. **创建** 3 个项目级 skill（`.gitignore` 加例外允许 `.claude/skills/` 入仓）：
   - `.claude/skills/usertest-intents/SKILL.md`（用户级入口 + 路由）
   - `.claude/skills/test-intents-author/SKILL.md`（写意图方法论）
   - `.claude/skills/test-intents-runner/SKILL.md`（跑意图方法论 + CLI + 经验库）
9. **重写** `docs/test-intents/README.md` 为 50 行极薄入口
10. **删除** CLAUDE.md 第 280-290 行"意图测试框架（test-intents）"整段

implementation plan 还需要决定：
- 11 个 A 类 task 中除日程外的 rules.md 是否在本次 plan 里全部按新模板重排（§2.7 决定「暂不动」）
- 是否在本次 plan 里给 13 个 task 各加一条 smoke 意图作为执行验证

---

（spec 完工，等待 user review）
