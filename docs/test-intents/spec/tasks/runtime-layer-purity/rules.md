# rules.md — runtime 层纯净性测试意图

`src/runtime/**/*.rs` 这一层不能依赖 Tauri 宿主能力。它只应该处理 runtime 逻辑；如果某个新文件开始直接碰 `tauri::`，架构边界就已经破了。

---

## 意图 1：除 `worker_runtime.rs` 这个历史例外外，runtime 层任何新文件都不能直接出现 `tauri::`

**场景**
Runtime 层必须保持纯净。当前仓库允许 `src/runtime/agent/worker_runtime.rs` 作为遗留例外，但除此之外，`src/runtime/**/*.rs` 中任何直接 `use tauri::`、`tauri::AppHandle`、`tauri::Emitter`、`tauri::Manager` 的出现都应该让测试失败，并且明确指出具体文件。

**前提**
- 扫描范围是 `src/runtime/**/*.rs`
- 允许列表只有 `src/runtime/agent/worker_runtime.rs`
- 检查内容同时包含：
  - `use tauri::`
  - `tauri::`
  - `tauri::Manager`
  - `tauri::Emitter`

**操作**
- 递归扫描整个 runtime 层源码

**验收标准**
- 只有 `src/runtime/agent/worker_runtime.rs` 可以作为明确的遗留例外
- 任何其他 runtime 文件只要出现上述字符串之一，测试就必须失败
- 失败信息必须包含具体文件路径
- 新增 runtime 文件一旦引入 tauri 依赖，测试必须第一时间报出

