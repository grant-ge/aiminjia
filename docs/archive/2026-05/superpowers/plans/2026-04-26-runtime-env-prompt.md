# Runtime 环境提示语义改造 Implementation Plan

**Goal:** 让模型在每轮对话的 `[当前环境]` 中知道 Runtime 的 Python/Node/npm/npx/uv/uvx 绝对路径，并在用户自然说“运行 Python/Node/npm 脚本”时默认使用这些路径，而不是系统 PATH。

**Architecture:** 不改 `bash` 的 PATH，不把系统命令重定向到 Runtime，也不要求用户说“仁励家的 Python”。只在动态环境上下文里注入中性的 `Runtime` 路径与规则，让模型根据提示选择正确的绝对路径。Runtime 路径来自现有 `RuntimeResolver::workspace_dependencies()`，失败时静默跳过 Runtime 段，避免影响主聊天流程。

**Tech Stack:** Rust / Tauri / existing `RuntimeResolver` / existing chat dynamic context builder / Cargo integration tests.

---

## 为什么这样改

真实用户不会说“用仁励家的 Python”。他们会说：

- “帮我跑一下这个 Python 脚本”
- “用 Python 分析这个 Excel”
- “运行 npm build”
- “执行这个 node 脚本”

所以模型需要知道：当用户自然提到 Python、Node、npm、npx、uv 或相关脚本时，默认应该使用应用托管 Runtime 的绝对路径，而不是系统 PATH 里的 `python3`、`node`、`npm`。

这次不做 `bash PATH` 注入，原因是：

1. 透明：模型和用户能看到实际使用的是哪个可执行文件。
2. 安全：不改变 shell 默认解析行为，避免用户以为 `python3` 是系统 Python。
3. 可解释：如果模型用了系统 Python，可以直接从命令里看出来它没有遵守 Runtime 路径规则。
4. 足够：只要把 Runtime 地址注入 `[当前环境]`，模型无论选择 bash、execute_python、MCP 还是其他工具，都能读到同一份环境提示。

## 最终提示语义

`[当前环境]` 中新增 Runtime 段，文案使用中性名称，不使用“仁励家 Runtime”这种品牌化表达：

```text
Runtime: 已安装
Runtime 当前目录: /Users/.../Library/Caches/renlijia-runtimes/renlijia-primary-runtime/versions/2026.04.26-runtime.1
Python: /Users/.../python/bin/python3
Node: /Users/.../node/bin/node
npm: /Users/.../node/bin/npm
npx: /Users/.../node/bin/npx
uv: /Users/.../uv/bin/uv
uvx: /Users/.../uv/bin/uvx
规则: 当用户要求运行 Python、Node、npm、npx、uv 或相关脚本时，默认使用以上 Runtime 绝对路径；只有用户明确要求系统环境时，才使用系统 PATH 中的命令。
```

注意：路径中出现 `renlijia-runtimes` 是文件系统隔离命名，可以保留；提示标签不需要品牌化。


## 与现有 `[当前环境]` 链路的关系

本改造不是新增一条 prompt 注入通道，而是复用现有 `[当前环境]` 链路：

```text
TauriLegacyTurnExecutor::get_env_info()
  -> runtime/chat/context_builder.rs::build_env_info()
  -> chat_turn_driver.rs::build_iteration_context(..., env_info, ...)
  -> [动态上下文]
  -> 模型看到 [当前环境]
```

现有 `[当前环境]` 已经包含：

```text
[当前环境]
工作目录 / 已连接目录
Git 状态
Platform
```

这次只是在同一个 `[当前环境]` 段落里追加 Runtime 路径：

```text
Runtime: 已安装
Runtime 当前目录: ...
Python: ...
Node: ...
npm: ...
npx: ...
uv: ...
uvx: ...
规则: 当用户要求运行 Python、Node、npm、npx、uv 或相关脚本时，默认使用以上 Runtime 绝对路径；只有用户明确要求系统环境时，才使用系统 PATH 中的命令。
```

这样做的原因：

1. 工作目录、Git、Platform、Runtime 都是“当前运行环境”的事实，应该放在一起。
2. 这段 dynamic context 每轮都会注入，适合放可能随 workspace/runtime 状态变化的信息。
3. 不污染静态 system prompt，不破坏 prompt cache 前缀稳定性。
4. 不需要新增工具、不需要 UI 特殊逻辑、不需要用户学习“仁励家 Python”这种说法。

## 改造范围

### 修改文件

- `src-tauri/src/runtime/chat/context_builder.rs`
  - 给 `build_env_info(...)` 增加可选 Runtime 环境信息参数。
  - 新增 `ManagedRuntimeEnvInfo` 结构体，只包含 Runtime 根目录和各工具绝对路径。
  - 新增格式化逻辑，输出中性的 `Runtime: 已安装` 和真实用户语义规则。

- `src-tauri/src/transport/tauri_commands/chat.rs`
  - 在 `TauriLegacyTurnExecutor::get_env_info(...)` 中，从 `self.services.runtime_resolver` 读取 `workspace_dependencies()`。
  - 构造 `ManagedRuntimeEnvInfo` 并传给 `build_env_info(...)`。
  - resolver 缺失或失败时只 `warn`，不阻断聊天。

- `src-tauri/tests/p0_a6_env_info_async_test.rs`
  - 更新 `build_env_info(...)` 调用签名，保持旧契约测试。

### 不修改

- 不修改 `src-tauri/src/runtime/tools/builtin/bash.rs`。
- 不给 bash 注入 managed `PATH`。
- 不修改 `execute_python` 的执行逻辑；它继续使用现有 managed Python 注入链路。
- 不在环境提示里运行 `--version`，只注入地址，避免每轮对话多跑进程。

---

## Task 1: 给环境上下文增加 Runtime 地址模型

**Files:**
- Modify: `src-tauri/src/runtime/chat/context_builder.rs`

- [ ] **Step 1: 写失败测试，覆盖 Runtime 地址和规则文案**

在 `src-tauri/src/runtime/chat/context_builder.rs` 的 `#[cfg(test)] mod tests` 中新增测试：

```rust
#[tokio::test]
async fn test_build_env_info_includes_runtime_paths_and_natural_language_rule() {
    let workspace_path = std::path::PathBuf::from("/tmp/test-workspace");
    let runtime_info = ManagedRuntimeEnvInfo {
        runtime_root: "/cache/renlijia/current".into(),
        python_path: "/cache/renlijia/python/bin/python3".into(),
        node_path: "/cache/renlijia/node/bin/node".into(),
        npm_path: "/cache/renlijia/node/bin/npm".into(),
        npx_path: "/cache/renlijia/node/bin/npx".into(),
        uv_path: "/cache/renlijia/uv/bin/uv".into(),
        uvx_path: "/cache/renlijia/uv/bin/uvx".into(),
    };

    let result = build_env_info(&workspace_path, None, Some(&runtime_info)).await;

    assert!(result.contains("Runtime: 已安装"));
    assert!(result.contains("Runtime 当前目录: /cache/renlijia/current"));
    assert!(result.contains("Python: /cache/renlijia/python/bin/python3"));
    assert!(result.contains("Node: /cache/renlijia/node/bin/node"));
    assert!(result.contains("npm: /cache/renlijia/node/bin/npm"));
    assert!(result.contains("npx: /cache/renlijia/node/bin/npx"));
    assert!(result.contains("uv: /cache/renlijia/uv/bin/uv"));
    assert!(result.contains("uvx: /cache/renlijia/uv/bin/uvx"));
    assert!(result.contains("当用户要求运行 Python、Node、npm、npx、uv 或相关脚本时"));
    assert!(result.contains("默认使用以上 Runtime 绝对路径"));
    assert!(result.contains("只有用户明确要求系统环境时"));
    assert!(!result.contains("仁励家 Runtime"));
}
```

- [ ] **Step 2: 运行测试确认失败**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test p0_a6_env_info_async_test -- --nocapture
```

Expected: 如果只新增在 unit test 中，这个命令不会覆盖该 unit test。应改用更精确但会编译 lib test 的命令：

```bash
cargo test --manifest-path src-tauri/Cargo.toml test_build_env_info_includes_runtime_paths_and_natural_language_rule -- --nocapture
```

Expected: FAIL，原因是 `ManagedRuntimeEnvInfo` 不存在，或 `build_env_info` 还不接受第三个参数。

- [ ] **Step 3: 实现 `ManagedRuntimeEnvInfo` 和格式化逻辑**

在 `build_env_info(...)` 附近新增：

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedRuntimeEnvInfo {
    pub runtime_root: std::path::PathBuf,
    pub python_path: std::path::PathBuf,
    pub node_path: std::path::PathBuf,
    pub npm_path: std::path::PathBuf,
    pub npx_path: std::path::PathBuf,
    pub uv_path: std::path::PathBuf,
    pub uvx_path: std::path::PathBuf,
}

impl ManagedRuntimeEnvInfo {
    pub fn format_for_env_info(&self) -> String {
        [
            "Runtime: 已安装".to_string(),
            format!("Runtime 当前目录: {}", self.runtime_root.display()),
            format!("Python: {}", self.python_path.display()),
            format!("Node: {}", self.node_path.display()),
            format!("npm: {}", self.npm_path.display()),
            format!("npx: {}", self.npx_path.display()),
            format!("uv: {}", self.uv_path.display()),
            format!("uvx: {}", self.uvx_path.display()),
            "规则: 当用户要求运行 Python、Node、npm、npx、uv 或相关脚本时，默认使用以上 Runtime 绝对路径；只有用户明确要求系统环境时，才使用系统 PATH 中的命令。".to_string(),
        ]
        .join("\n")
    }
}
```

修改 `build_env_info(...)` 签名：

```rust
pub async fn build_env_info(
    workspace_path: &std::path::PathBuf,
    authorized: Option<(&str, &str)>,
    runtime_info: Option<&ManagedRuntimeEnvInfo>,
) -> String {
```

在 `Platform` 后追加：

```rust
if let Some(runtime_info) = runtime_info {
    parts.push(runtime_info.format_for_env_info());
}
```

- [ ] **Step 4: 更新现有 `build_env_info(...)` 测试调用**

把 `context_builder.rs` 内现有：

```rust
build_env_info(&workspace_path, None).await
```

改成：

```rust
build_env_info(&workspace_path, None, None).await
```

把 authorized 调用改成：

```rust
build_env_info(&workspace_path, authorized_ref, None).await
```

- [ ] **Step 5: 运行测试确认通过**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml test_build_env_info_includes_runtime_paths_and_natural_language_rule -- --nocapture
```

Expected: PASS。

---

## Task 2: 生产聊天链路注入 Runtime 地址

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`

- [ ] **Step 1: 在 `get_env_info(...)` 中构造 Runtime 地址信息**

在 `TauriLegacyTurnExecutor::get_env_info(...)` 中，把：

```rust
use crate::runtime::chat::context_builder::build_env_info;
```

改成：

```rust
use crate::runtime::chat::context_builder::{build_env_info, ManagedRuntimeEnvInfo};
```

在调用 `build_env_info` 前新增：

```rust
let runtime_info = match self.services.runtime_resolver.as_ref() {
    Some(resolver) => match resolver.workspace_dependencies() {
        Ok(deps) => Some(ManagedRuntimeEnvInfo {
            runtime_root: infer_runtime_root(&deps.python),
            python_path: deps.python.clone(),
            node_path: deps.node.clone(),
            npm_path: deps.npm.clone(),
            npx_path: deps.npx.clone(),
            uv_path: deps.uv.clone(),
            uvx_path: deps.uvx.clone(),
        }),
        Err(error) => {
            log::warn!("[get_env_info] managed runtime unavailable: {}", error);
            None
        }
    },
    None => None,
};
```

然后把：

```rust
let env_info = build_env_info(&workspace_path, authorized_ref).await;
```

改成：

```rust
let env_info = build_env_info(&workspace_path, authorized_ref, runtime_info.as_ref()).await;
```

- [ ] **Step 2: 新增 `infer_runtime_root(...)` helper**

在 `impl TauriChatCommandAdapter` 前新增：

```rust
fn infer_runtime_root(path: &std::path::Path) -> std::path::PathBuf {
    let mut root = std::path::PathBuf::new();
    let mut components = path.components().peekable();
    while let Some(component) = components.next() {
        root.push(component.as_os_str());
        if component.as_os_str() == "versions" {
            if let Some(version) = components.next() {
                root.push(version.as_os_str());
                return root;
            }
        }
    }
    path.parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| path.to_path_buf())
}
```

这个 helper 只从任意工具路径中推断 `versions/<version>` 安装根目录，不读文件、不跑命令。

- [ ] **Step 3: 更新外部测试调用签名**

修改 `src-tauri/tests/p0_a6_env_info_async_test.rs`：

```rust
let result = build_env_info(&workspace_path, None, None).await;
```

- [ ] **Step 4: 编译检查**

Run:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: PASS。允许既有 dead_code warning，不允许新增编译错误。

---

## Task 3: 端到端验证模型可见 Runtime 路径

**Files:**
- No code changes.

- [ ] **Step 1: 启动 dev**

Run:

```bash
pnpm tauri dev
```

Expected: App 启动，后台 runtime ensure 不失败。

- [ ] **Step 2: 用户侧验证提示**

在聊天里输入：

```text
请根据当前环境信息，告诉我 Runtime 的 Python、Node、npm、npx、uv、uvx 绝对路径。不要执行命令，只复述你看到的路径。
```

Expected: 模型输出的路径包含：

```text
/Users/a20250311/Library/Caches/renlijia-runtimes/renlijia-primary-runtime/versions/2026.04.26-runtime.1/python/bin/python3
/Users/a20250311/Library/Caches/renlijia-runtimes/renlijia-primary-runtime/versions/2026.04.26-runtime.1/node/bin/node
/Users/a20250311/Library/Caches/renlijia-runtimes/renlijia-primary-runtime/versions/2026.04.26-runtime.1/node/bin/npm
/Users/a20250311/Library/Caches/renlijia-runtimes/renlijia-primary-runtime/versions/2026.04.26-runtime.1/node/bin/npx
/Users/a20250311/Library/Caches/renlijia-runtimes/renlijia-primary-runtime/versions/2026.04.26-runtime.1/uv/bin/uv
/Users/a20250311/Library/Caches/renlijia-runtimes/renlijia-primary-runtime/versions/2026.04.26-runtime.1/uv/bin/uvx
```

- [ ] **Step 3: 验证自然语言规则**

在聊天里输入：

```text
请用 Python 打印 sys.executable。你可以使用 bash，但必须按当前环境的规则选择 Python。
```

Expected: 模型使用 Runtime Python 绝对路径，而不是裸 `python3`：

```bash
/Users/a20250311/Library/Caches/renlijia-runtimes/renlijia-primary-runtime/versions/2026.04.26-runtime.1/python/bin/python3 -c "import sys; print(sys.executable)"
```

输出应包含 Runtime Python 路径。

- [ ] **Step 4: 检查格式**

Run:

```bash
git diff --check
```

Expected: PASS。

---

## 自检清单

- [ ] Runtime 提示标签使用 `Runtime`，不使用 `仁励家 Runtime`。
- [ ] 规则符合真实用户表达：用户说“运行 Python/Node/npm 脚本”即可默认使用 Runtime。
- [ ] 没有改 `bash PATH`。
- [ ] 没有每轮运行 `--version` 探测。
- [ ] Runtime resolver 失败不阻断聊天。
- [ ] 路径来自 `RuntimeResolver::workspace_dependencies()`，不是硬编码。
- [ ] macOS 路径仍位于用户缓存目录 `~/Library/Caches/renlijia-runtimes`。
