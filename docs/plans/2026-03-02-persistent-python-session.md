# 方案设计：持久化 Python 会话（会话级 REPL）

> 状态：已确认，待实施 | 优先级：P1 | 创建：2026-03-02 | Review：2026-03-02

## 背景

当前每次 `execute_python` 调用启动新进程，固定开销 3-8s：
- 进程启动 ~100ms
- Python 初始化 ~200ms
- import pandas/numpy/scipy ~2-3s
- sandbox preamble ~100ms
- exec(_analysis_utils.py) ~100-500ms
- pkl 反序列化恢复 _df + _user_vars ~0.5-5s（与数据量正相关）

一次完整薪酬分析（Step 0-5）约 10-30 次 execute_python 调用，累计 30-240s 浪费在重复开销上。

长会话中存在多文件、多次清洗、中间变量依赖等场景，一次性进程模型导致：
1. 变量丢失 → LLM 被迫重新加载/重新计算
2. 每次调用 ~50KB ANALYSIS_UTILS 重新编译
3. 大 DataFrame pkl 反序列化消耗大量时间
4. LLM 直接写的文件（不通过 `_export_detail`）不被跟踪，成为孤儿文件

## 当前方案（方案 B：文件即状态）

已实现，通过 pkl 快照 + epilogue/preamble 模拟变量持久化：

```
analysis/{conv_id}/
├── _original.pkl           # 原始数据（不可变）
├── _step_df.pkl            # 工作 DataFrame
├── _step{N}_df.pkl         # 各步骤快照（可回滚）
├── _step_dfs.pkl           # 多 DataFrame 字典
├── _user_vars.pkl          # 用户创建的所有变量（col_map, results 等）
├── step1_result.json       # 步骤结果缓存
└── ...
```

**优点**：架构不变、崩溃安全、已经在用
**缺点**：每次调用 3-8s 固定开销（数据大时更长）；变量持久化依赖 pkl 序列化/反序列化

## 方案 A：持久化 Python 进程

### 核心思路

每个分析会话维护一个常驻 Python 进程，变量天然保留在内存中。

**适用范围**：仅 `handle_execute_python` 在分析模式下使用持久会话。以下调用方保持一次性进程不变：
- `parser.rs`（文件上传解析）— 一次性任务，无状态需求
- `python_bridge.rs`（Python 插件）— 无状态，每次独立运行

```
Rust (per conversation)              Python (long-running process)
┌──────────────────┐                 ┌──────────────────────────┐
│ execute_python   │  stdin (code)   │ while True:              │
│   call #1        │ ──────────────► │   code = read_block()    │
│                  │ ◄────────────── │   result = exec(code)    │
│                  │  stdout (result)│   write_result(result)   │
│                  │                 │                          │
│ execute_python   │  stdin (code)   │   # globals 天然保留     │
│   call #2        │ ──────────────► │   # _df, col_map 都在   │
│                  │ ◄────────────── │   # 无需 pkl 反序列化   │
│                  │  stdout (result)│                          │
└──────────────────┘                 └──────────────────────────┘
                                     空闲 15 分钟后自动退出
```

### 架构设计

#### 1. 新增模块：`python/session.rs`

```rust
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, Notify};
use std::sync::atomic::AtomicU64;

/// 会话级 Python 进程管理器（Tauri managed state）
pub struct PythonSessionManager {
    sessions: DashMap<String, Arc<PythonSession>>,
    workspace_path: PathBuf,
    python_binary: PathBuf,
    python_home: Option<PathBuf>,
    max_sessions: usize,           // 最大同时存活进程数，默认 3
}

struct PythonSession {
    child: Mutex<Child>,
    stdin: Mutex<ChildStdin>,
    stdout_reader: Mutex<BufReader<ChildStdout>>,
    stderr_reader: Mutex<BufReader<ChildStderr>>,
    execution_lock: Mutex<()>,     // 同一会话串行执行
    cancel_notify: Notify,         // 中断信号
    created_at: Instant,
    last_used: AtomicU64,
    conversation_id: String,
    initialized: AtomicBool,       // sandbox + imports 是否已注入
}
```

#### 2. IPC 协议（双通道分离）

**原方案缺陷**：stdout 同时承载用户 `print()` 输出、IPC 控制标记（`__RESULT_BEGIN__`）和文件注册标记（`__GENERATED_FILE__`）。如果用户代码 print 了含标记字符串的内容，协议崩溃。

**修正**：分离为 stdout（用户输出）+ 元数据文件（系统控制）。

```
通道分配：
  stdin   → Rust 发送代码块
  stdout  → Python 用户 print() 输出（纯净，不含系统标记）
  stderr  → Python 异常/警告
  元数据  → 临时 JSON 文件 {workspace}/temp/_meta_{uuid}.json
```

IPC 时序：
```
Rust → Python (stdin):
  __EXEC__ {uuid} {timeout_seconds}\n
  {python code}
  __END__\n

Python 执行完毕后:
  1. 将 {exit_code, execution_time_ms, generated_files, written_paths}
     写入 {workspace}/temp/_meta_{uuid}.json
  2. stdout 输出 __DONE__ {uuid}\n 作为完成信号

Rust 端：
  1. 逐行读 stdout，收集用户输出
  2. 遇到 __DONE__ {uuid} → 停止读取
  3. 读取 _meta_{uuid}.json 获取结构化元数据
  4. 读取 stderr 获取错误输出
  5. 删除 _meta_{uuid}.json
```

**为什么不用 fd 3 / Unix socket**：
- fd 3 在 Windows 上不可用（Tauri 需跨平台）
- Unix socket 需要额外的 tokio 异步连接管理
- 临时 JSON 文件最简单，且利用已有的 `_safe_open` 写权限体系

Python 端 REPL loop（内嵌为 Rust 字符串常量 `REPL_SCRIPT`，运行时写入 temp 目录）：

```python
import sys, os, json, time, traceback, io

# 元数据收集（每次 exec 后写入 JSON 文件）
_meta = {}
_written_files = []  # _safe_open 写拦截记录

def _repl_loop():
    while True:
        line = sys.stdin.readline()
        if not line:
            break  # stdin closed → Rust 端已退出
        line = line.strip()
        if not line.startswith('__EXEC__'):
            continue

        parts = line.split(' ', 2)
        uuid = parts[1] if len(parts) > 1 else 'unknown'
        timeout = int(parts[2]) if len(parts) > 2 else 120

        # 读取代码块
        code_lines = []
        for code_line in sys.stdin:
            if code_line.strip() == '__END__':
                break
            code_lines.append(code_line)
        code = ''.join(code_lines)

        # 重置本次执行的追踪状态
        _written_files.clear()
        _meta.clear()
        start = time.time()
        exit_code = 0

        # 捕获 stdout（用户 print）到 StringIO
        _capture = io.StringIO()
        _old_stdout = sys.stdout
        sys.stdout = _capture

        try:
            exec(code, globals())
        except KeyboardInterrupt:
            exit_code = 130  # SIGINT
        except Exception:
            sys.stdout = _old_stdout
            traceback.print_exc()  # → stderr
            exit_code = 1
        finally:
            sys.stdout = _old_stdout

        elapsed = int((time.time() - start) * 1000)
        user_stdout = _capture.getvalue()

        # 写元数据到临时 JSON 文件
        meta_path = os.path.join(os.getcwd(), 'temp', f'_meta_{uuid}.json')
        try:
            with open(meta_path, 'w', encoding='utf-8') as f:
                json.dump({
                    'exit_code': exit_code,
                    'execution_time_ms': elapsed,
                    'generated_files': _meta.get('generated_files', []),
                    'written_paths': list(_written_files),
                }, f, ensure_ascii=False)
        except Exception as e:
            print(f'[WARN] Failed to write meta: {e}', file=sys.stderr)

        # 输出用户 stdout + 完成信号
        if user_stdout:
            _old_stdout.write(user_stdout)
            if not user_stdout.endswith('\n'):
                _old_stdout.write('\n')
        _old_stdout.write(f'__DONE__ {uuid}\n')
        _old_stdout.flush()

if __name__ == '__main__':
    _repl_loop()
```

#### 3. 会话初始化（首次调用时）

首次 `execute_python` 进入持久会话时，按顺序发送：

```
阶段 1：sandbox preamble（与现有 sandbox.rs preamble() 一致）
  → sys/os/builtins 设置
  → _ALLOWED_PATHS
  → resource limit
  → os.chdir(workspace)

阶段 2：trusted imports
  → import pandas, numpy, scipy, openpyxl, json, glob

阶段 3：_safe_open 注入（增强版，带写操作追踪）
  → builtins.open = _safe_open  (带 _written_files 记录)

阶段 4：ANALYSIS_UTILS 加载
  → exec(open('temp/_analysis_utils.py').read())

阶段 5：已有快照恢复（如果是崩溃恢复或空闲回收后重启）
  → 从 pkl 文件恢复 _df, _dfs, _user_vars
```

**后续调用：直接发送用户代码 + loaded_files_preamble（如有新文件），跳过阶段 1-4。**

#### 4. 生命周期管理

| 事件 | 行为 |
|------|------|
| 分析模式首次 execute_python | 懒启动 Python 进程，执行阶段 1-5 初始化 |
| 后续 execute_python | 复用已有进程，仅发送 loaded_preamble（如有新文件） + 用户代码 |
| 新文件上传（会话中途） | `handle_load_file` 用一次性 PythonRunner 解析 → 生成 `_df = _smart_read_data(path)` → 发送到持久会话执行 |
| 用户按停止（stop_streaming） | Rust 发送 SIGINT → Python 抛出 KeyboardInterrupt → REPL loop 捕获并恢复（见 §6） |
| 空闲 15 分钟 | 写 checkpoint → kill 进程 → 释放内存。下次调用自动重启 + 从 checkpoint 恢复 |
| 会话删除 | kill 对应进程 → 清理绑定该 conversation 的所有文件（见 §8） |
| Python 崩溃 (OOM/segfault) | 检测到进程退出 → 自动重启 → 从最近 checkpoint 恢复（降级到方案 B 的恢复路径） |
| 应用退出 | 遍历所有活跃会话：写 checkpoint → kill 进程。`kill_on_drop(true)` 兜底 |
| 单次执行超时 | kill 进程 → 自动重启 → 从上次 checkpoint 恢复 |
| 达到 max_sessions 上限 | LRU 淘汰最久未使用的会话（写 checkpoint → kill） |

#### 5. Checkpoint 策略

持久进程的优势是不需要每次调用都做 pkl 序列化。Checkpoint 仅在关键时刻写入：

| 时机 | 写入内容 | 触发方式 |
|------|----------|----------|
| 步骤切换（advance_step） | `_step{N}_df.pkl` + `_user_vars.pkl` + `step{N}_result.json` | orchestrator 调用 |
| 空闲回收前 | `_step_df.pkl` + `_user_vars.pkl`（完整快照） | idle timer 触发 |
| 应用退出前 | 同上 | shutdown hook |
| 进程崩溃后重启 | 无（读取已有 checkpoint 恢复） | crash recovery |
| 用户主动保存 | `_export_detail` 等工具生成文件 | 工具调用 |

**不再写 checkpoint 的场景**：
- 每次 execute_python 调用后（当前方案 B 的 epilogue）→ **取消**，这是最大的性能收益来源

Checkpoint 通过向持久会话发送系统代码实现：
```python
# checkpoint 代码（Rust 端在步骤切换时发送）
import pickle as _pkl
_snap_dir = _ANALYSIS_DIR
_pkl.dump(_df, open(os.path.join(_snap_dir, '_step_df.pkl.tmp'), 'wb'))
os.replace(os.path.join(_snap_dir, '_step_df.pkl.tmp'),
           os.path.join(_snap_dir, '_step_df.pkl'))
# ... 类似保存 _user_vars 等
```

#### 6. 执行中断机制

**原方案缺陷**：`stop_streaming` 只取消 LLM 流，不中断 Python。持久进程中，一个耗时 60s 的函数会阻塞整个会话。

**修正方案**：

```
用户按停止
    → 前端 IPC: stop_streaming(conversation_id)
    → Rust: gateway.cancel_conversation()  [已有，取消 LLM 流]
    → Rust: session_manager.interrupt(conversation_id)  [新增]
        → 向 Python 进程发送 SIGINT (Unix) / CtrlBreak (Windows)
        → Python KeyboardInterrupt → REPL loop except 捕获
        → exit_code = 130，写元数据，输出 __DONE__
        → REPL loop 继续等待下一个 __EXEC__（进程不死）

超时保护（fallback）：
    → SIGINT 后 5 秒内无 __DONE__ 响应（Unix）
    → 或直接 kill + restart（Windows）
    → 下次调用时自动重启 + checkpoint 恢复
```

Rust 端实现：
```rust
impl PythonSession {
    /// Interrupt current execution. Unix: SIGINT. Windows: kill + restart.
    async fn interrupt(&self) -> Result<()> {
        let child = self.child.lock().await;
        if let Some(pid) = child.id() {
            #[cfg(unix)]
            {
                unsafe { libc::kill(pid as i32, libc::SIGINT); }
            }
            #[cfg(windows)]
            {
                // Windows: no reliable SIGINT for non-console processes.
                // Kill the process; it will be restarted with checkpoint recovery.
                let _ = child.kill();
            }
        }
        Ok(())
    }
}
```

#### 7. 安全模型

**设计原则**：`validate_code()` 是第一道也是主要防线（静态分析，在代码进入进程前拦截）。持久进程的安全假设是：**通过 validate_code 的代码是可信的**。事后污染检测作为 defense-in-depth，不作为主要安全层。

```
┌──────────────────────────────────────────────────────────────┐
│ 安全层（按执行顺序）                                          │
├──────────────────────────────────────────────────────────────┤
│ L1: validate_code() — 静态分析（进入进程前）                   │
│     → 拦截 exec/eval/compile/os.system/subprocess 等         │
│     → 拦截 ctypes/importlib/__import__ 等                    │
│     → 这是主要防线，与现有逻辑完全一致                         │
│                                                              │
│ L2: sandbox preamble — _safe_open 文件写限制（首次注入）       │
│     → 所有 open() 写操作限制在 _ALLOWED_PATHS 内             │
│     → 增强：记录所有写路径到 _written_files                   │
│                                                              │
│ L3: 受限 exec namespace（每次执行）                            │
│     → exec(code, _exec_globals) 而非 exec(code, globals())   │
│     → _exec_globals 包含 _df, col_map 等用户变量              │
│     → 不包含 _safe_open._orig, _original_open 等内部引用      │
│                                                              │
│ L4: 事后完整性检查（每次 exec 后，defense-in-depth）           │
│     → 检查 builtins.open, builtins.__import__ 的 id 是否变化  │
│     → 检查 sys.modules 中是否出现 forbidden modules            │
│     → 发现篡改 → kill 进程，下次调用重启                      │
│                                                              │
│ L5: 进程级资源限制                                            │
│     → memory: resource.setrlimit (512MB)                     │
│     → 单次执行 timeout (120s)                                 │
│     → 进程级 wall-clock timeout (15min = agent 超时)          │
└──────────────────────────────────────────────────────────────┘
```

#### 8. 文件生命周期管理

**原方案缺陷**：LLM 直接调用 `df.to_excel()` / `plt.savefig()` 写文件时，不经过 `_export_detail`，文件不被注册、不被跟踪、不被清理。

**修正**：增强 `_safe_open`，自动跟踪所有写操作。

```python
# 增强版 _safe_open（在 sandbox preamble 中注入）
_written_files = []  # 全局追踪列表

_original_open = builtins.open
def _safe_open(file, mode='r', *args, **kwargs):
    if isinstance(file, (str, bytes)):
        file_str = file if isinstance(file, str) else file.decode('utf-8', errors='replace')
        if any(m in str(mode) for m in ('w', 'a', 'x')):
            abs_path = os.path.realpath(os.path.abspath(file_str))
            # 路径限制检查（已有逻辑）
            allowed = any(
                abs_path.startswith(os.path.realpath(p))
                for p in _ALLOWED_PATHS
            ) if _ALLOWED_PATHS else False
            if not allowed:
                raise PermissionError(f"Writing to '{file_str}' blocked")
            # 新增：记录写路径
            _written_files.append(abs_path)
    return _original_open(file, mode, *args, **kwargs)
builtins.open = _safe_open
```

Rust 端在每次 exec 结束后，从 `_meta_{uuid}.json` 的 `written_paths` 字段获取本次写入的所有文件路径，与已注册文件对比：

```rust
// 在 handle_execute_python 处理 meta 时
for path in &meta.written_paths {
    let rel_path = path.strip_prefix(&ctx.workspace_path).unwrap_or(path);
    // 跳过临时文件和 pkl 快照
    if is_temp_or_snapshot(rel_path) {
        continue;
    }
    // 检查是否已通过 __GENERATED_FILE__ 注册
    if !already_registered(rel_path) {
        // 自动注册为该会话的文件
        auto_register_file(ctx, rel_path, &ctx.conversation_id)?;
    }
}
```

**文件分类规则**：

| 路径模式 | 类型 | 注册 | 清理时机 |
|----------|------|------|----------|
| `temp/code_*.py` | 临时脚本 | 不注册 | 执行后立即删除（一次性模式）/ 不产生（持久模式） |
| `temp/_analysis_utils.py` | 系统模块 | 不注册 | 应用启动时 cleanup_temp_dir |
| `temp/_meta_*.json` | IPC 元数据 | 不注册 | 每次 exec 读取后立即删除 |
| `temp/_repl.py` | REPL 脚本 | 不注册 | 进程结束后删除 |
| `analysis/{conv_id}/*.pkl` | 数据快照 | 不注册 | 会话删除时随目录清理 |
| `analysis/{conv_id}/*.json` | 步骤缓存 | 不注册 | 同上 |
| `uploads/*` | 用户上传 | file_index.json | 会话删除时 |
| `exports/*.xlsx` | 通过 `_export_detail` 生成 | `__GENERATED_FILE__` → storage | 会话删除时 |
| `charts/*.png` | 通过 `generate_chart` 工具 | storage | 会话删除时 |
| `reports/*.html` | 通过 `generate_report` 工具 | storage | 会话删除时 |
| **`exports/*.csv` 等（LLM 直接写）** | **`_written_paths` 自动发现** | **自动注册 → storage** | **会话删除时** |

**会话删除时的清理流程**（已有 + 增强）：
```
delete_conversation(conv_id)
  → kill Python 持久进程（如果存在）
  → storage.delete_conversation(conv_id)         [已有：删 conv.json, messages, file_index]
  → 删除 analysis/{conv_id}/ 目录                [已有]
  → 遍历 storage 中 conv_id 绑定的所有文件记录：
      对每个 registered file → 删除物理文件       [已有]
  → 新增：遍历 _written_paths 自动注册的文件 → 删除
```

#### 9. 并发控制

```rust
impl PythonSessionManager {
    /// 获取或创建会话。如果达到 max_sessions，LRU 淘汰最旧的。
    async fn get_or_create(&self, conversation_id: &str) -> Result<Arc<PythonSession>> {
        if let Some(session) = self.sessions.get(conversation_id) {
            // 检查进程是否存活
            if session.is_alive().await {
                session.touch(); // 更新 last_used
                return Ok(session.clone());
            }
            // 进程已死，移除旧条目
            self.sessions.remove(conversation_id);
        }
        // 淘汰检查
        if self.sessions.len() >= self.max_sessions {
            self.evict_lru().await?; // 写 checkpoint → kill
        }
        // 创建新会话
        let session = PythonSession::spawn(
            conversation_id,
            &self.workspace_path,
            &self.python_binary,
            self.python_home.as_ref(),
        ).await?;
        let session = Arc::new(session);
        self.sessions.insert(conversation_id.to_string(), session.clone());
        Ok(session)
    }

    /// 执行代码。同一会话串行（execution_lock），不同会话并行。
    pub async fn execute(
        &self,
        conversation_id: &str,
        code: &str,
        timeout: Duration,
    ) -> Result<ExecutionResult> {
        let session = self.get_or_create(conversation_id).await?;
        let _lock = session.execution_lock.lock().await;

        // 进程健康检查（Mutex 持有期间）
        if !session.is_alive().await {
            // 进程在等锁期间被 kill（超时/中断），需要重启
            let new_session = self.restart_with_recovery(conversation_id).await?;
            return new_session.send_and_receive(code, timeout).await;
        }

        session.send_and_receive(code, timeout).await
    }

    /// 中断指定会话的当前执行。
    pub async fn interrupt(&self, conversation_id: &str) -> Result<()> {
        if let Some(session) = self.sessions.get(conversation_id) {
            session.interrupt().await?;
        }
        Ok(())
    }

    /// 关闭指定会话（会话删除时调用）。
    pub async fn destroy(&self, conversation_id: &str) {
        if let Some((_, session)) = self.sessions.remove(conversation_id) {
            session.write_checkpoint().await.ok();
            session.kill().await;
        }
    }

    /// 关闭所有会话（应用退出时调用）。
    pub async fn shutdown_all(&self) {
        for entry in self.sessions.iter() {
            entry.value().write_checkpoint().await.ok();
            entry.value().kill().await;
        }
        self.sessions.clear();
    }
}
```

**超时/kill 后的 Mutex 处理**：
- `send_and_receive` 内部持有 `execution_lock`
- 超时 → 在 `send_and_receive` 内 kill 进程 → 方法返回 Err
- 下一个调用 acquire lock 成功后，`is_alive()` 检测到进程已死 → 触发重启恢复

#### 10. python.rs 集成变更

```rust
// handle_execute_python 的变化：

pub(crate) async fn handle_execute_python(ctx: &PluginContext, args: &Value) -> Result<String> {
    let code = require_str(args, "code")?;
    let purpose = optional_str(args, "purpose").unwrap_or("code execution");

    // 1. validate_code（不变）
    let sandbox = SandboxConfig::for_workspace(&ctx.workspace_path);
    sandbox.validate_code(code).map_err(|e| anyhow!("Sandbox violation: {}", e))?;

    // 2. auto-load（不变）
    // ...

    // 3. 构建 loaded_files_preamble（不变）
    let loaded_preamble = build_loaded_files_preamble(...);

    let is_analysis = orchestrator::get_step_state(...).is_some();

    if is_analysis {
        // ─── 持久会话路径 ───
        let session_mgr: &PythonSessionManager = ctx.session_manager;

        // 组装代码：loaded_preamble + user_code（不含 analysis_preamble 和 epilogue！）
        let session_code = if loaded_preamble.is_empty() {
            code.to_string()
        } else {
            format!("{}\n{}", loaded_preamble, code)
        };

        let timeout = Duration::from_secs(120);
        let result = session_mgr.execute(
            &ctx.conversation_id,
            &session_code,
            timeout,
        ).await?;

        // 从 _meta_{uuid}.json 读取元数据
        // 处理 generated_files + written_paths
        // ...

    } else {
        // ─── 一次性进程路径（日常聊天，不变）───
        let runner = PythonRunner::new(ctx.workspace_path.clone(), ctx.app_handle.as_ref());
        let result = runner.execute_raw(&final_code).await?;
        // ...
    }
}
```

**关键简化**：持久会话中不再需要 analysis_preamble 和 epilogue。
- `_df`、`col_map`、`results` 等变量天然保留在进程内存
- snapshot 相关逻辑移到 checkpoint 策略（§5）
- `_analysis_utils.py` 在会话初始化时加载一次

### 改动范围

| 文件 | 改动 | 说明 |
|------|------|------|
| `python/session.rs` | **新增** | 会话级进程管理器 + IPC + 生命周期 |
| `python/repl.py` | **不再需要** | 内嵌为 Rust 字符串常量 `REPL_SCRIPT` |
| `python/runner.rs` | 不变 | 保留：日常聊天 + parser + python_bridge + 崩溃恢复 |
| `python/sandbox.rs` | 小改 | `_safe_open` 增加 `_written_files` 追踪 |
| `python/mod.rs` | 小改 | 导出 session 模块 |
| `llm/tool_executor/python.rs` | 重构 | 分析模式走 session，日常走 runner |
| `lib.rs` | 小改 | 注册 PythonSessionManager 为 managed state |
| `commands/chat.rs` | 小改 | stop_streaming 增加 session.interrupt()；delete_conversation 增加 session.destroy() |
| `tauri.conf.json` | 不变 | repl.py 内嵌为字符串常量，无需 bundle |

### 性能预估

| 指标 | 方案 B (当前) | 方案 A (持久进程) |
|------|-------------|-----------------|
| 首次调用 | ~5s | ~5s（冷启动一样） |
| 后续调用 | 3-8s | **<100ms**（代码直接 exec） |
| 10 次调用累计开销 | 30-80s | ~5s（仅首次） |
| 30 次调用累计开销 | 90-240s | ~5s |
| 内存占用 | 每次回收 | 常驻 ~100-300MB/进程 |
| 步骤切换 checkpoint | N/A | ~0.5-3s（仅在切步骤时） |

### 风险评估

| 风险 | 概率 | 影响 | 缓解 |
|------|------|------|------|
| LLM 代码污染全局状态 | 低 | 中 | L1 validate_code 拦截 + L3 受限 namespace + L4 事后检测 |
| Python OOM 崩溃 | 低 | 中 | 自动重启 + checkpoint 恢复（方案 B 兜底） |
| 进程泄漏 | 低 | 中 | max_sessions 上限 + LRU 淘汰 + 空闲回收 + kill_on_drop |
| stdout 用户输出含 `__DONE__` 字符串 | 极低 | 中 | UUID 后缀使碰撞概率极低；可加 HMAC 校验 |
| 并发 Mutex 竞争 + 进程重启 | 低 | 中 | lock 后 is_alive 检查 + 自动重启 |
| 中断后状态不一致 | 中 | 低 | KeyboardInterrupt 在 Python 中可被 try/finally 安全处理 |
| LLM 直接写文件不被跟踪 | 中→低 | 中 | `_written_files` 自动跟踪 + 自动注册 |

### 工期估算（修正后）

| 阶段 | 内容 | 预估 |
|------|------|------|
| 1 | `session.rs` 核心（spawn/IPC/send_and_receive/timeout） | 1.5 天 |
| 2 | `repl.py` REPL loop + stdout 捕获 + 元数据文件 | 0.5 天 |
| 3 | 生命周期管理（idle/evict/destroy/shutdown_all/restart） | 1 天 |
| 4 | Checkpoint 策略（advance_step 触发/idle 前写/恢复逻辑） | 0.5 天 |
| 5 | 中断机制（SIGINT + stop_streaming 集成） | 0.5 天 |
| 6 | 文件追踪（_written_files + 自动注册 + 会话清理） | 0.5 天 |
| 7 | `python.rs` 重构（分析模式 session / 日常 runner 双路径） | 0.5 天 |
| 8 | 集成（lib.rs 注册 / chat.rs 钩子 / sandbox.rs 增强） | 0.5 天 |
| 9 | 测试 + 端到端调试 | 1.5 天 |
| **合计** | | **7 天** |

### 与方案 B 的关系

**不互斥，方案 B 作为降级路径保留**：

```
                    ┌─────────────────────┐
                    │  execute_python 调用  │
                    └──────────┬──────────┘
                               │
                    ┌──────────▼──────────┐
                    │    is_analysis?      │
                    └──────────┬──────────┘
                         │           │
                       yes          no
                         │           │
              ┌──────────▼────┐  ┌──▼──────────────┐
              │ 持久会话 (A)   │  │ 一次性进程 (B)   │
              └──────────┬────┘  └─────────────────┘
                         │
                    ┌────▼─────┐
                    │ 进程存活?  │
                    └────┬─────┘
                    │         │
                  yes        no
                    │         │
             ┌──────▼──┐  ┌──▼──────────────────┐
             │ 直接 exec │  │ 重启 + checkpoint   │
             │ (<100ms)  │  │ 恢复 (方案 B 兜底)  │
             └──────────┘  └─────────────────────┘
```

### 附录：已确认的设计决策

1. **max_sessions 默认值**：3（够用）
2. **空闲超时**：15 分钟
3. **Windows 中断**：直接 kill + restart（不依赖 `GenerateConsoleCtrlEvent`）
4. **repl.py 打包方式**：内嵌为 Rust 字符串常量（`const REPL_SCRIPT: &str = r#"..."#`），不作为独立 resource 文件
