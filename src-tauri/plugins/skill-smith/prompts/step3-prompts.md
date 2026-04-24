=== 当前任务：Step 3 — 生成 prompts ===

为 workflow.toml 中每个步骤生成对应的 prompt 文件。

## 执行流程

1. 读取之前生成的 workflow.toml（通过 step2 的 save_analysis_note 了解步骤内容）
2. 为每个步骤生成 `prompts/stepN.md` 文件
3. 可选：生成 `prompts/base.md` 作为角色定位（如果 plugin.toml 里 include_app_base=true 则不需要）
4. 每个文件调用 `skill_smith_write_file(relative_path="prompts/stepN.md", content=<内容>)`
5. 全部写完后调用 `skill_smith_validate()` 验证所有文件是否存在且内容 ≥50 字节
6. 有 error → 修复对应文件 → 重新验证

## prompt 文件结构

每个 stepN.md 应包含：

```markdown
=== 当前任务：Step N — 步骤名称 ===

<1-2 句话说明本步目标>

## 执行流程

1. 具体动作（调用什么工具、做什么事）
2. ...
3. ...

## 输出要求

- 格式要求
- 内容要求

⚠️ 注意事项
```

## 写 prompt 的原则

1. **具体而非抽象**：不要写"分析数据"，要写"调用 execute_python 计算每个部门的平均薪资"
2. **指明工具**：明确告诉 LLM 用哪个工具，怎么调用
3. **角色代入**：给 LLM 一个具体角色（"你是薪酬分析师"而非"你是 AI 助手"）
4. **约束清晰**：用 ⚠️ 标注禁止事项
5. **长度适中**：200-800 字符。太短质量差（<50 字节会报 error），太长浪费 token

## 示例（数据分析 step0）

```markdown
=== 当前任务：Step 0 — 分析方向确认 ===

用户上传了数据文件。你的任务是了解数据内容并确认分析方向。

## 执行流程

1. 调用 load_file(file_id) 加载文件数据
2. 用一句话概括文件内容
3. 告知用户接下来的分析步骤
4. 询问用户是否有特别关注的方向

如果用户提供了分析方向，调用 save_analysis_note(key="direction", value="...") 保存。

⚠️ 本步只做确认，不要开始分析。
```

## 批量生成策略

- 先一次性生成所有 prompt 文件（多次调用 skill_smith_write_file）
- 然后一次 validate 检查全部
- 如有问题，只修复有问题的文件

向用户展示："所有步骤的提示词已生成完毕，请说「继续」进入校验环节。" 不需要展示 prompt 原文。

⚠️ 每个 prompt 文件不得少于 50 字节。
⚠️ workflow.toml 中引用的每个 prompt 路径都必须有对应文件。
