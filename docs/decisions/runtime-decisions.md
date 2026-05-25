# 运行时 — 设计决策归档

> 从 CLAUDE.md 迁出的已稳定设计决策。日常编码参考 CLAUDE.md 中的摘要即可，需要细节时查本文件。

## max_tokens 按模型自动选（v0.5.8）

默认输出预算从写死的 4096/100000 改成 `llm::max_tokens::default_max_tokens_for_model(name)` 启发式查表（DeepSeek V4 = 384k、GPT-5 = 128k、GLM-4.5/4.6 = 96k、Claude Sonnet/Opus + Gemini-2 = 64k、Qwen3 / qwen-max-2025 = 16k、Qwen-max-longcontext = 30k、其他兜底 8192）。新增上限按模型加进表里。从 `chat_turn_driver` 调（`llm_settings.primary_model` 在 scope 内）；想覆盖的调用方传 `Some(value)` 即可，否则传 `None` 让 driver 走 per-model 默认。

## Shell 工具走 bundled runtime PATH + 失败信号上报（2026-05-21）

`BashTool`（Unix）和 `PowerShellTool`（Windows）spawn 子进程前必须调用 `shell_common::inject_bundled_runtime_path(&ctx, &mut cmd)`，把 `<bundled>/node/bin` 前置到子进程 PATH。**为什么**：npm/npx 是 `#!/usr/bin/env node` 的 shebang 脚本，即便用绝对路径调 `$NODE_DIR/bin/npm`，npm 内部的 postinstall(`sh -c "node install.js"`) 也走 PATH，没有这一步，`npm install -g dingtalk-workspace-cli` 之类的命令在用户机器上必现 `env: node: No such file or directory`（2026-05-21 客户截图复现）。`inject_bundled_runtime_path` 在 `ctx.capability.runtime_resolver` 缺失时静默 no-op（legacy/test 路径不受影响）。每个命令收尾都走 `emit_shell_failure_diagnostic(&ctx, tool, command, exit_code, output, semantics.is_error)` —— 把 exit_code 127/126 或 npm install 失败分类成 `runtime_install_failure / command_not_found / permission_denied / command_timeout / command_failure`，level=Error 的两类（runtime_install / command_not_found）会被服务端 lotus diagnostics handler 升级为 `client_diagnostic_alert` 推到钉钉群。所有逻辑集中在 `src-tauri/src/runtime/tools/builtin/shell_common.rs`，bash.rs / powershell.rs 各只引入两个函数 + 调用两次；6 个分类器单测钉死行为（含 npm postinstall 模式、纯 exit 127、exit 0 不上报、stderr signature 抽取等）。

## 个人版账户与消耗（2026-05-19）

当 `tenant.type === 'personal'` 时，设置面板侧栏出现"我的账户"（key `account-billing`），展示余额 / 本月消耗 / 本月调用次数 + 消耗记录流水分页。后端走 lotus gateway 新端点 `GET /v1/billing/summary` 和 `GET /v1/billing/usage-records?page=&size=`（personal 租户专属，企业租户 403）。Rust 侧：`AuthManager::get_billing_summary` / `get_billing_usage_records`（吃 session_key），`#[tauri::command] billing_summary` / `billing_usage_records` 暴露给前端；TS 侧：`useBillingStore` zustand（summary + records + pagination + loading/error），打开面板 `useEffect` 触发 `refresh()` 并发拉两个接口。`tenant.type` 由 `/v1/profile` 透传 → `AuthTenantInfo.r#type` → `TenantInfo.tenant_type`（camelCase 序列化为 `tenantType`）→ `CloudAuthInfo.tenant.tenantType?: string`，企业用户读到 `enterprise` 后菜单 filter 隐藏 `account-billing`。新注册 personal 用户由 lotus 在 registration tx 内赠送 ¥10，幂等键 `tenants.signup_bonus_granted_at`；余额 < 赠送额度时 UI 不再显示"含 10 元赠送"提示。第二期接支付宝充值订单（暂未实现）。Spec：`~/lotus/docs/superpowers/specs/2026-05-19-personal-billing-and-signup-bonus-design.md`。
