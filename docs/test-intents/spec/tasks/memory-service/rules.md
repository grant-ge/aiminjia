# rules.md — ProjectMemoryService 服务层测试意图

## 意图 1：保存记忆时写入独立 entry 文件并重建索引

**前提**
- 使用隔离的 `app_data_dir` 和 `workspace_path`
- 构造一条合法的 `ProjectMemoryEntryDraft`

**操作**
- 调用 `ProjectMemoryService::save_memory()` 保存该记忆

**断言**
- 返回的 entry 文件真实存在，且位于该 workspace 对应的 project memory bucket 下
- entry 文件包含 frontmatter：`type`、`name`、`description`，有 `source` 时也写入 `source`
- entry 文件正文包含原始 `content`
- `MEMORY.md` 被重建，包含该 entry 的链接和 description

---

## 意图 2：不同 workspace 的记忆互相隔离

**前提**
- 使用同一个 `app_data_dir`
- 构造两个不同的 `workspace_path`
- 分别为两个 workspace 保存不同记忆

**操作**
- 分别调用两个 workspace 对应 service 的 `load_context(query)`

**断言**
- workspace A 只能 recall 自己 bucket 内的记忆
- workspace B 只能 recall 自己 bucket 内的记忆
- 两个 service 的 `memory_root()` 不相同
- 任一 workspace 的 `MEMORY.md` 不包含另一个 workspace 的 entry

---

## 意图 3：加载上下文时只返回与 query 相关的 entries

**前提**
- 同一 workspace 下保存多条不同主题的记忆
- query 只命中其中一部分 entry 的 name、description 或 content

**操作**
- 调用 `load_context(query)`
- 再调用 `render_for_prompt()`

**断言**
- `recalled_entries` 只包含命中的相关记忆
- `render_for_prompt()` 使用 `[相关记忆]` 格式输出 recalled entries
- 未命中的记忆不会出现在 prompt 渲染结果里
- 没有任何命中时，`recalled_entries` 为空，且 prompt 渲染回退为 `MEMORY.md` index 文本

---

## 意图 4：legacy core memory 被懒迁移且迁移幂等

**前提**
- `app_data_dir/shared/cognitive/mem.md` 中存在旧核心记忆内容
- 当前 workspace 尚无迁移 entry

**操作**
- 第一次调用 `load_context(query)`
- 第二次再次调用 `load_context(query)`

**断言**
- 第一次加载会生成 `entries/legacy-core-memory.md`
- 迁移 entry 的类型为 `project_constraint`，source 为 `legacy-core-memory`
- `MEMORY.md` 包含 `legacy-core-memory` 指针
- 第二次加载不会重复生成新的 legacy entry
- 迁移后的 prompt 能包含旧核心记忆内容

---

## 意图 5：distill_index 从现有 entry 文件重建 MEMORY.md

**前提**
- project memory bucket 下已有合法 entry 文件
- `MEMORY.md` 不存在或内容为空/过期

**操作**
- 调用 `ProjectMemoryService::distill_index()`

**断言**
- 返回值等于成功解析出的 entry 数量
- `MEMORY.md` 被重建
- 重建后的 index 包含所有合法 entry 的链接和 description
- 缺少 frontmatter、缺少 type/name/description、或 type 非法的 entry 不进入 index

---

## 意图 6：同一条记忆重复保存时更新而不是复制

**前提**
- 同一 workspace 下保存两条 `name` 与 `description` 完全相同、`content` 不同的 draft

**操作**
- 先保存 v1
- 再保存 v2
- 遍历 project memory bucket 的 `entries/` 目录
- 读取 `MEMORY.md`

**断言**
- `entries/` 下只存在 1 个对应 entry 文件
- entry 文件正文是 v2 内容，不再包含 v1 内容
- `MEMORY.md` 中该 entry 只出现一次

---

## 意图 7：四类 memory_type 都能正确持久化并被 recall

**前提**
- 同一 workspace 下分别保存 4 条记忆：
  - `user_preference`
  - `project_constraint`
  - `reference_info`
  - `feedback`
- 每条记忆都带唯一 query 关键词

**操作**
- 分别用每条记忆的关键词调用 `load_context(query)`

**断言**
- 每次 recall 都只返回对应那一条记忆
- 返回 entry 的 `memory_type` 与保存时一致
- entry 文件 frontmatter 中的 `type` 与保存时一致

---

## 意图 8：query 为空或过短时不做相关性召回，只回退 index

**前提**
- 同一 workspace 下已有多条合法记忆

**操作**
- 调用 `load_context("")`
- 调用 `load_context("a")`
- 调用 `load_context("我")`

**断言**
- 每次返回的 `recalled_entries` 都为空
- `render_for_prompt()` 不包含 `[相关记忆]`
- `render_for_prompt()` 返回 `MEMORY.md` index 文本

---

## 意图 9：相关性召回最多返回 5 条，且优先返回命中分更高的记忆

**前提**
- 同一 workspace 下保存 6 条都能被 query 命中的记忆
- 其中 1 条在 name、description、content 中多次命中 query token，命中分最高

**操作**
- 调用 `load_context(query)`

**断言**
- `recalled_entries.len() == 5`
- 命中分最高的记忆一定在结果中
- 第 6 条低分记忆不出现在结果中

---

## 意图 10：损坏的 entry 文件不会污染 recall 和 index

**前提**
- `entries/` 下同时存在：
  - 1 个合法 entry
  - 1 个无 frontmatter 的 entry
  - 1 个缺少 `type` 的 entry
  - 1 个 `type` 非法的 entry

**操作**
- 调用 `load_context(query)`
- 调用 `distill_index()`

**断言**
- `load_context(query)` 只返回合法 entry
- `distill_index()` 返回值只统计合法 entry
- `MEMORY.md` 只包含合法 entry 的链接和 description
- 损坏 entry 的 name/content 不进入 `MEMORY.md`，也不进入 prompt 渲染结果
