# rules.md — workspace（工作区）意图测试规格

## 测试范围

覆盖 workspace（工作区）的发现、切换与上下文隔离：用户在已有项目目录下启动对话时 workspace 自动识别、workspace 级别设置对 global 设置的覆盖关系、以及在多个 workspace 间切换时上下文（对话历史 / 记忆 / 生成物路径）严格隔离。不包含 workspace 内部具体文件结构的字段级验证。

## 待覆盖的主要场景

- 场景 1：用户在一个已经存在 workspace 标记的目录下启动对话，workspace 被自动发现并激活，不弹新建向导
- 场景 2：workspace 级别覆盖 global 设置的字段在当前 turn 立即生效，未覆盖的字段继续走 global
- 场景 3：从 workspace A 切到 workspace B 后，B 看不到 A 的对话历史 / 记忆 / uploads / reports
- 场景 4：在没有任何 workspace 标记的临时目录下启动，回退到默认 home workspace，不污染其他 workspace
- 场景 5：workspace 目录被外部删除后重新启动，应用不崩，给出可恢复的错误提示
- 场景 6：同一 workspace 被两个窗口同时打开时，对状态文件的并发写不丢数据

## 待补充

> 具体意图待补全。
