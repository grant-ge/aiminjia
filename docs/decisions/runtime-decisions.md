# 运行时 — 设计决策归档

> 从 CLAUDE.md 迁出的已稳定设计决策。日常编码参考 CLAUDE.md 中的摘要即可，需要细节时查本文件。

## max_tokens 按模型自动选（v0.5.8）

默认输出预算从写死的 4096/100000 改成 `llm::max_tokens::default_max_tokens_for_model(name)` 启发式查表（DeepSeek V4 = 384k、GPT-5 = 128k、GLM-4.5/4.6 = 96k、Claude Sonnet/Opus + Gemini-2 = 64k、Qwen3 / qwen-max-2025 = 16k、Qwen-max-longcontext = 30k、其他兜底 8192）。新增上限按模型加进表里。从 `chat_turn_driver` 调（`llm_settings.primary_model` 在 scope 内）；想覆盖的调用方传 `Some(value)` 即可，否则传 `None` 让 driver 走 per-model 默认。

> **已被「云端唯一化」取代（2026-05-25）**：本地模型路径下线后，`chat_turn_driver` 固定请求 `1_000_000` 上限，**真实上限由 lotus 网关按 upstream 模型钳制/重写**，不再走本地启发式。`default_max_tokens_for_model` 现仅剩自测引用。

## Shell 工具走 managed runtime PATH + 错误诊断上报（2026-05-21，2026-06-30 更新）

`BashTool`（Unix）和 `PowerShellTool`（Windows）spawn 子进程前必须按用户设置注入 `ManagedRuntimeProcessEnv`，把 AIjia 托管 runtime cache 中的 Node/Python/uv 目录前置到子进程 PATH。**为什么**：npm/npx 是 `#!/usr/bin/env node` 的 shebang 脚本，即便用绝对路径调 `$NODE_DIR/bin/npm`，npm 内部的 postinstall(`sh -c "node install.js"`) 也走 PATH，没有这一步，相关安装或工具命令会在用户机器上因找不到 node 或版本不一致失败。2026-06-30 起安装包不再内置 Node/Python/uv 兜底包；managed runtime 只来自本机 cache 或 OSS manifest 下载。每个命令收尾都走 `emit_shell_failure_diagnostic(&ctx, tool, command, exit_code, output, semantics.is_error)` —— 把 exit_code 127/126 或 npm install 失败分类成 `runtime_install_failure / command_not_found / permission_denied / command_timeout / command_failure`，level=Error 的两类（runtime_install / command_not_found）会被服务端 lotus diagnostics handler 升级为 `client_diagnostic_alert` 推到钉钉群。所有逻辑集中在 `src-tauri/src/runtime/tools/builtin/shell_common.rs`，bash.rs / powershell.rs 各只引入两个函数 + 调用两次；分类器单测钉死行为（含 npm postinstall 模式、纯 exit 127、exit 0 不上报、stderr signature 抽取等）。

## 个人版账户与消耗（2026-05-19）

当 `tenant.type === 'personal'` 时，设置面板侧栏出现"我的账户"（key `account-billing`），展示余额 / 本月消耗 / 本月调用次数 + 消耗记录流水分页。后端走 lotus gateway 新端点 `GET /v1/billing/summary` 和 `GET /v1/billing/usage-records?page=&size=`（personal 租户专属，企业租户 403）。Rust 侧：`AuthManager::get_billing_summary` / `get_billing_usage_records`（吃 session_key），`#[tauri::command] billing_summary` / `billing_usage_records` 暴露给前端；TS 侧：`useBillingStore` zustand（summary + records + pagination + loading/error），打开面板 `useEffect` 触发 `refresh()` 并发拉两个接口。`tenant.type` 由 `/v1/profile` 透传 → `AuthTenantInfo.r#type` → `TenantInfo.tenant_type`（camelCase 序列化为 `tenantType`）→ `CloudAuthInfo.tenant.tenantType?: string`，企业用户读到 `enterprise` 后菜单 filter 隐藏 `account-billing`。新注册 personal 用户由 lotus 在 registration tx 内赠送 ¥10，幂等键 `tenants.signup_bonus_granted_at`；余额 < 赠送额度时 UI 不再显示"含 10 元赠送"提示。第二期接支付宝充值订单（暂未实现）。Spec：`~/lotus/docs/superpowers/specs/2026-05-19-personal-billing-and-signup-bonus-design.md`。

## 云端唯一化：移除 use_cloud / 本地模型 / 本地搜索 key（2026-05-25）

产品不再提供"本地配置大模型"和"本地搜索 API key"入口，彻底删除三类遗留配置——`AppSettings.use_cloud`、`tavily_api_key`、`bocha_api_key`（连同 `ResolvedLlmSettings` / `PluginContext` / `RequestScopedRuntimeDeps` / `SearchDeps` 上的同名字段、TS `Settings` 类型/默认值/store setters、两处 settings 命令的加解密）。

**起因是一个登录 BUG**：老装机磁盘上残留 `useCloud=false`（前端默认 true、登录从不回写、无任何 UI 翻转它），导致已登录用户路由绕过云分支 → 用 `primary_model`（云模型名如 `deepseek-v3`）+ 空 key → 网关回退 lotus 时带空 Authorization → 每次重开报 `401 Missing Authorization header`（退出重登只在内存里恢复，重启复发）。SLS 实锤该 401 主要在 Windows、少量 macOS。

三层修复：
1. `llm/gateway.rs::is_auth_revoked_error` 改为**按 HTTP 401 + 结构化错误码/类型**（`authentication_error` / `auth_error`）判定，不再脆弱地匹配消息子串——原来漏掉 "Session key expired"（"session" 与 "expired" 间隔着 "key"，不含子串 "session expired"）和 "Missing Authorization header"。任何 auth 401 都触发"作废 session_key → 刷新 → 重试一次"自愈。
2. session_key 注入改用 `provider_resolves_to_lotus()`（非 `openai`/`claude`/`custom` 即视为 lotus），覆盖 `dispatch_stream` 里"未知 provider 回退 lotus"路径，杜绝空 key 发到网关。
3. `router::select_route` 删除非云分支，**所有任务恒走 `provider="lotus"`**：Reasoning → `model_type=reasoner`（关 tools），否则 `cloud_model_type`（空则 "chat"）。

附带：
- **token 预算**：`chat_turn_driver` 固定请求 `1_000_000`，真实上限由网关按 upstream 模型钳制/重写（见上「max_tokens」节的取代说明）。
- **视觉**：anthropic 图片支持不再受 use_cloud 门控，恒按 `cloud_model` 能力判断。
- **web 搜索**：`execute_web_search_core` 现为云端优先（登录走 lotus `/v1/search`）+ 无 key 的 Bing 兜底，删除 Bocha/Tavily 分支。工具上下文里那个 `use_cloud`（控制 web 搜索）本就是 no-op（`let _ = use_cloud`），一并删净。
- **兼容**：旧 config.json 里残留的 `useCloud` / `tavilyApiKey` / `bochaApiKey` 键被静默忽略（serde 不报未知字段），无需迁移。

**活跃约束**：判定 auth 类 401 一律按错误码/类型，禁止再用消息文案子串匹配；所有 LLM 路由都经 lotus 网关，不再有本地 provider 分支。
