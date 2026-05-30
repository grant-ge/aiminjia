# AIjia Agent 执行约束

> 最后梳理：2026-05-29
> 当前仓库：`/Users/gezhigang/work-codeup/aijia/code`
> 服务端仓库：`/Users/gezhigang/lotus`

## 基本要求

- 全程中文回答。
- 先读当前代码和相关文档，再下结论。
- 只改本次任务相关文件；不要顺手清理无关代码或重写历史。
- 遇到已有未提交改动时，默认视为用户改动，不能回滚。
- 修改前后都要用可复现命令验证；不能在没有证据时声称完成。

## 当前权威入口

- 日常编码约束：`CLAUDE.md`
- 桌面端当前架构：`docs/architecture-blueprint.md`
- 架构决策归档：`docs/decisions/*.md`
- 发布流程：`docs/release-playbook.md`
- 服务端/网关协议：`/Users/gezhigang/lotus/CLAUDE.md`、`/Users/gezhigang/lotus/docs/gateway-behavior.md`
- 桌面端跨仓长期参考：`/Users/gezhigang/lotus/docs/desktop/`

## 执行流程

- 复杂需求先拆清楚范围、写集、验证口径。
- 修复缺陷时先复现或补最小验证，再实现。
- 涉及 Rust Runtime、Tauri IPC、工具系统、存储路径、权限、安全边界时，优先检查 `CLAUDE.md` 的强约束。
- 涉及服务端协议、网关行为、计费、鉴权时，先对齐 `~/lotus` 的当前文档和代码。
- 涉及发布时，使用 `docs/release-playbook.md`，不要依赖旧的口头流程。

## 架构对标

历史文档中出现的 `claude-code-best` 是架构参考来源，不是当前机器上的必然路径。除非本机明确存在对应目录且任务要求做架构对标，否则不要把旧路径当作执行前置条件。

## 验证建议

- 前端：`pnpm test`、`pnpm build`、聚焦 Vitest 文件。
- Rust：优先 `cd src-tauri && cargo check` 或具体 `cargo test --test <name>`；大范围 `cargo test` 视改动风险再跑。
- Tauri 开发：`pnpm tauri:dev` 会先执行 `pnpm ensure:runtime`。
