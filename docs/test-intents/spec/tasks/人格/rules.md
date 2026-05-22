# rules.md — persona（人格 / 数字角色）意图测试规格

## 测试范围

覆盖 persona（人格 / 数字角色）的生命周期与当前对话绑定行为：用户对 persona 的 CRUD 操作、把 persona 切换到当前对话上、以及 persona 被删除后引用它的 agenda（日程组织者）项进入 Orphaned 状态的链路。不包含 persona 实际的 prompt 注入效果验证（那归 prompt 渲染层）。

## 待覆盖的主要场景

- 场景 1：用户新建一个 persona，记录被持久化并出现在列表里
- 场景 2：用户把当前对话的 persona 切换为另一条已存在的 persona，下一轮 turn 使用新人格
- 场景 3：用户删除一个 persona，未被引用的清除直接成功
- 场景 4：用户删除一个被 agenda 引用的 persona，受影响 agenda item 转为 Orphaned，对话不崩
- 场景 5：用户尝试删除内置 persona（builtin=true），操作被拒绝
- 场景 6：导入 / 导出 persona 时元数据完整往返，id 不重复

---

## 意图 1：用户新建 persona 后，配置完整落盘并显示在列表里

**场景**
用户在 persona 管理页打开"新建人格"对话框，填写名字、图标、身份描述、专业领域后保存。系统应该把这条新 persona 写到磁盘，并出现在 persona 列表里供后续选择。重启应用后这条 persona 不能丢。

**前提**
- 应用已启动，已登录任意账号
- 当前 scope 下 `~/.renlijia/users/{scope}/personas/` 目录已被初始化（内置 persona 已 seed）
- 当前没有任何同名自定义 persona

**操作**
1. 打开 persona 管理页面，点击"新建人格"按钮
2. 在弹窗中填写：名称 `市场分析师`、图标 `📈`、描述 `专注 SaaS 行业市场分析`、身份 `你是一名资深的 SaaS 市场分析师`、专业领域加两条 `行业研究` / `竞品分析`
3. 点击"保存"按钮，等待保存成功提示
4. 退出应用后重新启动，重新打开 persona 管理页

**验收标准**
- `~/.renlijia/users/{scope}/personas/` 下出现一个新 `<uuid>.json` 文件（文件名不是 `default.json` 也不是其他 8 个内置 id）
- 新 json 文件内容为合法 JSON，反序列化后 `name` 字段值为 `"市场分析师"`
- 新 json 文件反序列化后 `icon` 字段值为 `"📈"`
- 新 json 文件反序列化后 `identity` 字段值为 `"你是一名资深的 SaaS 市场分析师"`
- 新 json 文件反序列化后 `builtin` 字段值为 `false`
- 新 json 文件反序列化后 `expertise` 数组长度为 `2`，包含字符串 `"行业研究"` 与 `"竞品分析"`
- 新 json 文件反序列化后 `created_at` 字段为非空 RFC3339 时间戳，`updated_at` 与 `created_at` 字段值相等
- `~/.renlijia/users/{scope}/personas/index.json` 反序列化后 `order` 数组包含新 persona 的 id
- persona 管理页列表中能看到名为 `"市场分析师"` 的条目，图标显示 `📈`
- 应用重启后，persona 管理页列表中仍然能看到该条目（说明落盘真的成功，不仅是内存态）

---

## 意图 2：用户在当前对话切换 persona，下一轮 AI 回复按新人格表现

**场景**
用户在对话顶栏点选 persona 切换器，从"通用工作助手"切到"HR 专家"。下一条用户消息发出后，AI 的回复应该按新 persona 的身份风格来；持久化层面 `index.json::active` 也要更新为新 persona id。

**前提**
- 应用已启动，配置了有效 API key
- `~/.renlijia/users/{scope}/personas/` 下至少包含两条 persona：内置 `default`（通用工作助手）和内置 `hr-expert`（HR 专家）
- 当前 active persona 为 `default`（`index.json::active` 字段值为 `"default"`）
- 已打开一个空对话（消息历史 0 条）

**操作**
1. 在对话顶栏点击 persona 切换器，选择"HR 专家"
2. 等待切换器状态更新（显示 HR 专家头像/名字）
3. 在输入框输入 `"请介绍一下你自己"`，点击发送
4. 等待 AI 完整回复��束

**验收标准**
- 切换操作完成后，`~/.renlijia/users/{scope}/personas/index.json` 反序列化后 `active` 字段值为 `"hr-expert"`，不再为 `"default"`
- 对话顶栏 persona 切换器显示 `"HR 专家"`（不是 `"通用工作助手"`）
- 消息发送后 `~/.renlijia/users/{scope}/conversations/{conv_id}/messages.jsonl` 共 2 条记录，每条合法 JSON
- 第 1 条记录 `role` 字段值为 `"user"`，`content.text` 字段值为 `"请介绍一下你自己"`
- 第 2 条记录 `role` 字段值为 `"assistant"`，`content.text` 字段值非空
- 第 2 条记录 `content.text` 字段值包含字符串 `"HR"` 或 `"人力资源"` 或 `"招聘"` 之一（说明 HR 专家 persona 的 identity 已注入 system prompt）
- `TurnCompleted` 事件中 `persona_id` 字段值（或 system prompt 段落中可见的人格段）含 `"hr-expert"`
- `TurnCompleted` 的 `outcome` 字段值为 `"Success"`

---

## 意图 3：用户删除一条未被任何 agenda 引用的自定义 persona，文件与索引同步清理

**场景**
用户在 persona 管理页右键自己之前创建的"市场分析师" persona，点"删除"。系统应该把对应文件删掉、`index.json` 里的引用也清掉，列表里立刻消失，并且因为这条 persona 没有任何 agenda item 依赖，整个删除流程一帆风顺。

**前提**
- 应用已启动
- `~/.renlijia/users/{scope}/personas/<uuid>.json` 中存在一条用户自定义 persona `"市场分析师"`（`builtin=false`）
- `~/.renlijia/users/{scope}/agenda/items/` 目录为空，或所有 `*.json` 文件中 `organizer_employee_id` 字段值均不等于该 persona 的 id

**操作**
1. 在 persona 管理页列表中找到"市场分析师"条目
2. 点击该条目右侧菜单中的"删除"按钮
3. 在确认弹窗中点击"确认删除"
4. 等待列表刷新

**验收标准**
- 该 persona 对应的 `~/.renlijia/users/{scope}/personas/<uuid>.json` 文件不再存在
- `~/.renlijia/users/{scope}/personas/index.json` 反序列化后 `order` 数组中不包含该 persona 的 id
- 若被删的 persona 之前是 active，`index.json` 反序列化后 `active` 字段值为 `"default"`；若不是 active，则 `active` 字段值保持原值不变
- persona 管理页列表中不再显示"市场分析师"条目
- `~/.renlijia/users/{scope}/agenda/items/` 下所有 `*.json` 文件中均不存在 `status` 字段值为 `"Orphaned"` 的项（说明没有误伤）
- 删除完成后再发一条对话消息 `"你好"`，对应 `TurnCompleted` 事件 `outcome` 字段值为 `"Success"`（说明 persona 列表的删除不影响对话主链路）

---

## 意图 4：删除被 agenda 引用的 persona，相关 agenda item 转 Orphaned 并在列表中标红

**场景**
用户之前用"市场分析师" persona 配置过一条周报 agenda（每周一早 9 点跑），现在他要把"市场分析师"删掉。系统不能直接把 agenda item 也一起删（避免静默丢失定时任务），而是把 item 的状态改成 Orphaned，调度器停止触发，UI 上用警示色标记，用户可以稍后重指 organizer 复活。

**前提**
- 应用已启动
- `~/.renlijia/users/{scope}/personas/<persona_uuid>.json` 中存在一条 `"市场分析师"` persona（`builtin=false`）
- `~/.renlijia/users/{scope}/agenda/items/<item_id>.json` 中存在一条 agenda item，其 `organizer_employee_id` 字段值等于上述 persona 的 id，`status` 字段值为 `"Active"`
- agenda 页能看到这条 agenda item 处于活跃状态

**操作**
1. 在 persona 管理页找到"市场分析师"，点击"删除" → 确认
2. 等待删除完成
3. 切到 agenda（日程）页查看那条 agenda item

**验收标准**
- 对应 `~/.renlijia/users/{scope}/personas/<persona_uuid>.json` 文件不再存在
- 对应 `~/.renlijia/users/{scope}/agenda/items/<item_id>.json` 文件仍然存在（不被级联删除）
- 该 agenda item json 反序列化后 `status` 字段值为 `"Orphaned"`（不再是 `"Active"`）
- 该 agenda item json 反序列化后 `organizer_employee_id` 字段值保持为原 persona id 不变（让用户可以判断是被哪条 persona 删除连带的）
- 该 agenda item json 反序列化后 `updated_at` 字段值晚于删除操作之前的值
- agenda 页该 item 仍在列表中显示，但带"孤儿 / 无主 / Orphaned"标记或警示色
- 后台调度器不再触发该 item（操作完成后等待 10 秒，`~/.renlijia/users/{scope}/agenda/occurrences/<item_id>/*.jsonl` 文件不增加新行）
- 主对话窗口可以继续发消息，`TurnCompleted` 的 `outcome` 字段值为 `"Success"`（删 persona 不连累正常对话）
