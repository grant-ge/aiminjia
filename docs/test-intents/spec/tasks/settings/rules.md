# rules.md — settings（设置分层）意图测试规格

## 测试范围

覆盖设置的三层优先级（user / workspace / global）与变更生效时序：workspace 级覆盖 global 级、user 级在多账号间隔离、settings 变更后下一个 turn 立即读到新值（不需要重启）。不包含具体设置面板的 UI 控件行为。

## 待覆盖的主要场景

- 场景 1：workspace 设置某字段，global 同一字段被覆盖，当前 workspace 下该字段读取到 workspace 值
- 场景 2：workspace 没有覆盖某字段时，回退读 global 值
- 场景 3：账号 A 与账号 B 的 user 级设置互不可见，切换账号后读到自己的值
- 场景 4：在 turn 进行中修改 settings，当前 turn 仍用旧值（避免半路换配置），下一个 turn 立即读到新值
- 场景 5：settings 文件被外部破坏（JSON 解析失败）时，应用回退到默认值并给出可恢复提示，不崩
- 场景 6：删除 workspace 级设置（恢复默认）后，立刻回退读 global 值

## 待补充

> 具体意图待补全。
