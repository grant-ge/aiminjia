# 2026-04-12 Runtime-First 后续专项问题定义

## 目的

这份文档只做 3 件事：

- 把当前已经确认的核心问题收敛成专项主题
- 为每个专项明确 `问题 / 目标 / 验收标准`
- 作为后续优先级讨论、文件级改造方案和实施计划的前置输入

这份文档**不展开**文件级改造点和分期实施计划；后续可基于此文档继续输出专项计划。

## 当前结论

当前 `lotus-app` 的主要问题，已经不只是“前后端事件有没有接好”，而是更上层的能力模型仍然偏向：

- `upload-first` 文件处理模型
- 复合型、非原子工具集合
- 由厚 prompt 承担过多运行时职责
- skill 本地安装与打包导入模型不统一

这 4 个问题会直接限制 agent runtime 的上限，优先级应高于一般的 UI 微调和文案优化。

## 专项 1：Workspace-First 文件能力模型

### 问题

当前系统的文件模型本质上是 `upload-first`，不是 `workspace-first`。

表现为：

- 用户文件先被复制到 `workspace/uploads/` 后，后续工具才围绕这些副本工作
- Python / report / chart 等能力默认围绕 workspace 子目录运行
- “选择本地目录”并没有被真正建模成一等工作对象
- 这使得 agent 更像“导入文件后再分析的助手”，而不是“可对本地目录进行连续工作的代理”

### 代码证据

- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/storage/file_manager.rs:66`
- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/storage/file_manager.rs:92`
- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/python/sandbox.rs:68`
- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/llm/tool_executor/report.rs:31`
- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/prompts/base.md:13`
- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/prompts/base.md:37`

### 目标

- 文件和目录都要升级成一等工作对象，不再只能先上传再处理
- “选择本地目录”后，agent 能在授权边界内浏览、读取、筛选、分析、导出
- `upload_file` 退化为导入方式之一，而不是主能力模型
- 系统同时支持：
  - 上传文件工作流
  - 本地目录工作流
  - 工作区生成物工作流

### 验收标准

- 用户可以在桌面端选择任意本地目录作为当前工作目录，不需要先把文件复制进 `uploads/`
- agent 至少可以对授权目录完成以下 4 类动作：
  - 列出目录内容
  - 读取指定文本或结构化文件
  - 在目录中搜索目标文件
  - 基于目录中的文件直接分析并输出结果
- 至少有 1 条完整主链路通过验收：
  - 选择本地目录 → 识别目录中的 `.txt/.csv/.xlsx` → 读取/分析 → 生成报告或导出
- Python / 文件工具不再要求源文件必须来自 `file_id + uploads/`
- 安全边界仍成立：
  - 只能访问用户明确授权的目录范围
  - 不能无界访问整个本地文件系统
- 现有上传流程不能回归：
  - `upload_file -> load_file -> execute_python` 仍可用

## 专项 2：Atomic Tool 工具体系

### 问题

当前默认工具集暴露过宽，且大量工具不是原子工具，而是复合业务工具。

表现为：

- 通用 daily 模式默认暴露大量工具
- 很多工具本身带有明显业务假设或多段动作语义
- agent 更像在猜“哪个大工具最接近需求”，而不是稳定组合基础能力
- 大量 builtin tool 仍走 legacy `ToolPlugin` + `LegacyToolAdapter`

这会直接限制：

- 工具组合能力
- 局部失败后的恢复能力
- 子 agent / task / workflow 的稳定扩展能力

### 代码证据

- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/plugin/builtin/tools/mod.rs:1`
- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/plugin/builtin/tools/mod.rs:37`
- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/plugin/context.rs:57`
- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/plugin/context.rs:74`
- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/llm/tools.rs:29`
- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:1595`

### 目标

- 把默认工具集收缩成少量、清晰、可组合的原子能力
- 复杂工作流能力不再作为“基础工具”直接暴露，而转移到 skill / workflow / orchestration 层
- 默认工具应满足：
  - 单一职责
  - 明确输入输出
  - 明确权限边界
  - 明确可观测事件
- 核心默认工具要真正进入 `RuntimeTool` 体系，而不是长期停留在 legacy bridge 上

### 验收标准

- 通用 daily 模式默认暴露的基础工具集显著收敛，不再是“默认 23 个大工具全开”
- 每个基础工具都满足单一职责，不允许一个工具同时隐含“读取 + 分析 + 生成 + 导出”多段语义
- 报告生成、复杂抽取、业务统计分析等复合能力不再作为通用基础工具直接暴露
- 默认通用链路上的核心工具迁移到 runtime-native 工具体系，不再依赖 legacy `ToolPlugin` 作为长期主路径
- 工具调用链路唯一：
  - lookup
  - permission
  - execution
  - audit / event
- 至少通过 3 类组合场景验证工具原子性：
  - 文件读取类任务
  - 本地目录处理类任务
  - 联网检索 + 本地处理的混合任务
- 当一个工具失败时，agent 可以局部恢复，而不是整条复合大动作直接报废

## 专项 3：Prompt Slimming 提示词职责回收

### 问题

当前 `base.md + daily.md` 不是轻量 prompt，而是一套操作手册。

表现为：

- prompt 同时承载身份、工具协议、文件处理规则、目录结构说明、数据传递规则、记忆策略、输出规范等多层职责
- 本该由 runtime / policy / tool contract 保证的规则，被硬写进 prompt
- prompt 实际上变成了隐藏编排层

这会带来：

- 简单对话也被厚 prompt 拉成模板化行为
- 改运行时能力时往往先得改 prompt
- 工具和文件语义与 prompt 强耦合

### 代码证据

- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/prompts/base.md:3`
- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/prompts/base.md:13`
- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/prompts/base.md:25`
- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/prompts/base.md:44`
- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/prompts/daily.md:9`
- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/prompts/daily.md:17`
- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/llm/prompts.rs:174`

### 目标

- 系统提示词只保留：
  - 核心身份 / 语气
  - 高层安全边界
  - 高层任务偏好
  - 必要的宿主约束说明
- 工具顺序、数据传递协议、文件目录结构、轮次数限制、步骤推进规则，从 prompt 回收到 runtime / tool policy / workflow 配置
- prompt 从“流程手册”回归为“行为框架”

### 验收标准

- daily 默认 system prompt 明显瘦身，不再维持当前的大块静态说明
- prompt 中不再出现以下本应由系统保证的细节协议：
  - 必须先 `load_file` 再 `execute_python`
  - 报告数据必须先写 JSON 再传工具
  - 一轮最多几次工具调用
  - 工作目录有哪些子目录
- 这些规则改由 runtime / tool schema / permission / workflow 配置保证
- 简单对话场景明显改善：
  - 用户打一声招呼，不应再被厚重提示词拉成长篇模板化介绍
- prompt 保留必要人格和风格一致性，但不再充当隐藏编排器
- prompt 变更后，工具调用正确率不能明显下降

## 专项 4：Skill 本地导入 / 打包导入模型统一

### 问题

当前本地 skill 安装只支持“源码目录”，不支持直接导入本地 `.skill` / `.aijia-skill` 文件。

这会导致：

- 用户看到选择器时不清楚到底应该选“目录”还是“文件”
- 本地开发型导入与本地分发型导入不是一个统一模型
- marketplace 安装、本地源码安装、本地打包导入之间语义不一致

### 代码证据

- `/Users/a20250311/IdeaProjects/lotus-app/src/components/settings/SkillsTab.tsx:88`
- `/Users/a20250311/IdeaProjects/lotus-app/src/components/settings/SkillsTab.tsx:120`
- `/Users/a20250311/IdeaProjects/lotus-app/src/lib/tauri.ts:930`
- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/commands/skill_management.rs:75`
- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/commands/skill_management.rs:322`

### 目标

- 本地技能导入要明确区分两种模式：
  - 从源码目录安装
  - 从打包文件安装
- 用户不需要猜“这个入口到底能选目录还是能选文件”
- 本地与 marketplace 的 skill 生命周期模型应尽量统一

### 验收标准

- Skills 页面明确支持并区分：
  - 从目录安装
  - 从打包文件安装
- `.skill` / `.aijia-skill` 类型的本地包可以被直接选择并导入
- 目录型 skill 源码安装继续保留
- UI 文案与系统行为一致，不再出现“看起来像能选文件，实际只认目录”的歧义
- 从本地包导入后，skill 能被系统识别、注册并正常显示

## 横切约束

后续任何改造方案都不应接受以下伪完成状态：

- 只改 prompt，不改 runtime / tool / file model
- 只把目录选择弹窗修好，但后端仍然只能处理 `uploads/` 副本
- 继续往默认 daily 模式里叠更多复合工具
- 新增一层兼容壳，但真实主链路仍回到 legacy chat / tool 逻辑
- 把复杂规则继续写进 prompt，伪装成“改造完成”

## 建议的下一步

建议后续讨论顺序如下：

1. 先确定这 4 个专项的优先级
2. 再为每个专项输出：
   - 影响范围
   - 文件级改造点
   - 分期计划
   - TDD / 回归路径
3. 最后再进入具体代码改造

## 与后续计划的关系

这份文档不是计划本身，而是计划前的“问题定义与目标定义”。

后续如果继续产出专项改造方案，建议直接以本文件作为输入，避免再次回到“先讨论是不是问题”的阶段。
