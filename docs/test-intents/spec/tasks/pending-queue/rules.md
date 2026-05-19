# rules.md — pending-queue（待办队列）意图测试规格

## 测试范围

覆盖用户在 AI 还在执行当前 turn 时继续发消息的队列化行为：消息正确进入 pending 队列、当前 turn 结束后队列被自动 drain 成下一轮 turn 的输入、用户取消时队列被清空且不残留。不包含队列消息的具体渲染样式或动画。

## 待覆盖的主要场景

- 场景 1：当前 turn 处于 Running 状态时用户发消息，消息进入 pending 队列而非立刻起新 turn
- 场景 2：当前 turn 完成（AgentIdle）后，pending 队列里的消息被合并/按序 drain 出来，触发下一轮 turn
- 场景 3：pending 队列里多条消息按到达顺序合并，不丢、不乱序
- 场景 4：用户在 Running 状态点取消，pending 队列被清空，cancel 之后再起的 turn 不会读到旧 pending
- 场景 5：应用崩溃 / 重启后，pending 队列里未消费的消息按设计选择"恢复"或"丢弃"（与持久化策略一致）
- 场景 6：pending 队列空时 AgentIdle 不会触发空 turn

## 待补充

> 具体意图待补全。
