# rules.md — persona（人格 / 数字角色）意图测试规格

## 测试范围

覆盖 persona（人格 / 数字角色）的生命周期与当前对话绑定行为：用户对 persona 的 CRUD 操作、把 persona 切换到当前对话上、以及 persona 被删除后引用它的数字员工进入 Orphaned 状态的链路。不包含 persona 实际的 prompt 注入效果验证（那归 prompt 渲染层）。

## 待覆盖的主要场景

- 场景 1：用户新建一个 persona，记录被持久化并出现在列表里
- 场景 2：用户把当前对话的 persona 切换为另一条已存在的 persona，下一轮 turn 使用新人格
- 场景 3：用户删除一个 persona，未被引用的清除直接成功
- 场景 4：用户删除一个被数字员工引用的 persona，受影响员工 lifecycle 变为 Orphaned，对话不崩
- 场景 5：用户尝试删除内置 persona（builtin=true），操作被拒绝
- 场景 6：导入 / 导出 persona 时元数据完整往返，id 不重复

## 待补充

> 具体意图待补全。
