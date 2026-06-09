# 后端地址规则 — 统一走 `crate::environment`

所有 lotus 后端服务地址必须通过 `src-tauri/src/environment.rs` 解析，**禁止写死、禁止在各模块散落本地环境变量**。

## 必须

- tenant 网关（auth / billing / LLM ingress / `/v1/*` 桌面 API）→ `crate::environment::tenant_host()`
- ops 门户（公开员工模板目录 `/api/public/employee-templates`）→ `crate::environment::ops_host()`
- 两个函数返回**无尾斜杠**的 origin，直接 `format!("{}/path", host())` 拼接即可

已统一的参考实现：
- `auth/client.rs::base_url()` → `tenant_host()`
- `llm/providers/lotus.rs`、`llm/providers/aijia_gateway_v2.rs` → `tenant_host()`
- `commands/workplace_directory.rs` → `tenant_host()`
- `runtime/employee/template_store.rs`（`fetch_manifest` / `fetch_catalog`）→ `ops_host()`

## 禁止

- ❌ 在请求路径里写死 `https://ai-tenant.renlijia.com` / `https://ai-ops.renlijia.com` / `https://{test,pre}-...renlijia.com` 等字面量
- ❌ 自建 `DEFAULT_*_BASE_URL` 常量当兜底
- ❌ 在某个模块里自己 `std::env::var("LOTUS_TENANT_BASE_URL" / "LOTUS_OPS_BASE_URL" / ...)` 绕过 environment 模块
  - 历史教训：`workplace_directory.rs` 曾写死 `DEFAULT_TENANT_BASE_URL` + 自读 `LOTUS_TENANT_BASE_URL`，`template_store.rs` 曾自读 `LOTUS_OPS_BASE_URL`，都已收口删除

## 环境切换机制（唯一真相源）

`src-tauri/src/environment.rs`：
- **release 构建**：编译期锁死 prod（`#[cfg(debug_assertions)]` 把 dev override 路径整段不编译），shipped binary 不受 `config.json` 篡改影响
- **debug 构建**：跟随 dev 环境切换器，预设 test / pre / prod（`dev::PRESETS`），override 持久化在 `~/.renlijia/global/config.json` 的 `dev_environment` 键
- 新增环境切换需求 → 改 `environment.rs`，不要在调用点加分支

## 第三方地址不在此约束内

钉钉 / 飞书 / 企微 / claude / openai 等第三方服务有各自的常量与环境变量（如 `DINGTALK_REGISTRATION_BASE_URL`），与 lotus environment 模块分离，保持现状即可。
