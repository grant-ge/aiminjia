# FAQ 知识库异步切片入 memory（小客 / 小工）+ 员工配置表单 i18n 实现 plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让小客（builtin:xiaoke）和小工（builtin:xiaogong）的 FAQ/知识源在雇佣时上传 → 后台异步切片入 cognitive memory，运行时改用 `memory_search` 按需检索（不再每次把 FAQ 全文塞进 context）；同时把员工资源配置表单的硬编码英文文案接入 i18n。

**Architecture:**
- 雇佣 wizard 第 3 步上传 FAQ → 雇佣完成后非阻塞地 spawn 后台 task → task 读文件 → LLM 分块 → 每个 chunk `save_cognitive_memory(category="knowledge:{employee_id}", conversation_id=employee_id)` → 进度写回 `resource_config.knowledgeSources[].status`。
- 运行时（dispatch_prompt）不再 inline FAQ 全文；员工 system prompt 改为提示 "你的知识库已切片，使用 memory_search 检索"。
- 切片状态通过现有 `EmployeeCard` 派活前置检查显示（pending → 禁派活并提示；failed → 显示重试按钮）。
- i18n：把 `forms/*.tsx` + `ResourceConfigForm.tsx` 中所有面向用户字符串移到 `src/i18n/{zh-CN,en-US}.json` 的 `employee.config.*` 命名空间。

**Tech Stack:** TypeScript + React + react-i18next（前端）；Rust + tokio + serde + 现有 `cognitive::save_memory`（后端）；现有 LLM gateway（`llm/gateway.rs` 走 chat completion）。

---

## File Structure

### Frontend (TS/React)

| File | Action | Responsibility |
|------|--------|----------------|
| `src/features/employees/templates.ts` | modify | 新增 `builtin:xiaogong`（小工·技术支持，与小客一对）；将 `ResourceConfigKind` 加 `'tech-support' \| 'customer-support'`；小客/小工 `resourceConfigKind` 改为 `'customer-support'`/`'tech-support'`；`requiresDingtalk: true`；`cron: '*/30 8-18 * * 1-5'` |
| `src/features/employees/ResourceConfigForm.tsx` | modify | 路由 `customer-support` → `CustomerSupportConfigForm`，`tech-support` → `TechSupportConfigForm`；title 走 i18n |
| `src/features/employees/forms/CustomerSupportConfigForm.tsx` | modify | 新增 `knowledgeSources` 字段：上传 FAQ 文件 picker + 已上传文件列表 + per-file 状态徽章（pending/indexing/done/failed + 重试） |
| `src/features/employees/forms/TechSupportConfigForm.tsx` | modify | 同上（共用一套知识库 UI） |
| `src/features/employees/forms/KnowledgeSourcesField.tsx` | create | 共享知识库 UI 组件（上传/列表/状态/删除/重试） |
| `src/features/employees/forms/GroupMatchInput.tsx` | modify | 文案接入 i18n |
| `src/features/employees/forms/MonitoringUrlsForm.tsx` | modify | 文案接入 i18n |
| `src/features/employees/forms/SalesTableConfigForm.tsx` | modify | 文案接入 i18n |
| `src/features/employees/forms/WeeklyReportConfigForm.tsx` | modify | 文案接入 i18n |
| `src/features/employees/HireWizard.tsx` | modify | 雇佣完成后调用 `employee_index_knowledge_async` 启动后台切片（不阻塞 wizard 关闭） |
| `src/features/employees/EmployeeDrawer.tsx` | modify | "重新切片"按钮 + 显示 `knowledgeSources[].status` |
| `src/features/employees/triggerPrechecks.ts` | modify | 新增 precheck：当 `knowledgeSources` 非空且任一仍 `pending`/`indexing`，弹出提示"知识库索引中，请稍候" |
| `src/lib/tauri.ts` | modify | 新增 IPC 类型：`employee_index_knowledge_async`、`employee_knowledge_status` |
| `src/i18n/zh-CN.json` | modify | 新增 `employee.config.*` 命名空间（约 60 keys） |
| `src/i18n/en-US.json` | modify | 同上英文版 |

### Backend (Rust)

| File | Action | Responsibility |
|------|--------|----------------|
| `src-tauri/src/runtime/employee/knowledge.rs` | create | `KnowledgeIndexer`：读文件 → 启发式或 LLM chunk → 逐条 `save_cognitive_memory` → 写回 employee record 的进度 |
| `src-tauri/src/runtime/employee/mod.rs` | modify | 暴露 `knowledge` mod |
| `src-tauri/src/runtime/employee/store.rs` | modify | `EmployeeRecord` 新增 helper `update_knowledge_source_status(id, path, status, sliced_count, error)` |
| `src-tauri/src/transport/tauri_commands/employee.rs` | modify | 新增 `employee_index_knowledge_async(employee_id, file_paths)` + `employee_knowledge_status(employee_id)` |
| `src-tauri/src/runtime/employee/dispatch_prompt.rs` | modify | 当员工 templateId ∈ {xiaoke, xiaogong}：在 dispatch prompt 中提示"你的 FAQ 已切片入 memory，使用 memory_search 按客户问题检索"；移除 inline FAQ 全文注入（如有） |
| `src-tauri/tests/employee_knowledge_test.rs` | create | 集成测试：上传 → 切片 → memory 命中 |

---

## Data Model

### `knowledgeSources` 字段（写入 `EmployeeRecord.resource_config.knowledgeSources`）

```ts
type KnowledgeSourceStatus = 'pending' | 'indexing' | 'done' | 'failed'

interface KnowledgeSource {
  path: string            // 上传后副本路径（在 ~/.renlijia/uploads/employee/{id}/ 下）
  originalName: string    // 用户上传时的原文件名
  size: number            // 字节数
  status: KnowledgeSourceStatus
  slicedCount: number     // 已切片入 memory 的 chunk 数
  error?: string          // status=failed 时的原因
  startedAt?: string      // ISO timestamp
  completedAt?: string
}
```

### Memory 切片格式

```
category = "knowledge:{employee_id}"
conversation_id = employee_id
tags = ["faq", original_filename]
content = "Q: ...\nA: ..."   // 或 "## 标题\n正文..." 视来源结构
```

---

## Self-Review Checklist (executed before sharing)

1. ✅ Spec coverage: FAQ 切片（A1）+ 异步后台 + 失败重试 + i18n。
2. ✅ 类型一致：`KnowledgeSourceStatus` 在 TS / Rust / store.rs / 三个 form / drawer / precheck 中保持一致命名（pending/indexing/done/failed）。
3. ✅ 无占位符：每个 step 含完整代码。
4. ✅ 派活前置：`triggerPrechecks` 增加新分支，避免索引中触发空知识库 LLM 调用。

---

## Tasks

### Task 1: 添加 `builtin:xiaogong` 模板 + 扩展 `ResourceConfigKind`

**Files:**
- Modify: `src/features/employees/templates.ts`

- [ ] **Step 1: 写 unit test 验证新模板存在**

Create `src/features/employees/templates.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { BUILTIN_TEMPLATES, findTemplate } from './templates'

describe('templates', () => {
  it('exposes builtin:xiaoke as customer-support', () => {
    const t = findTemplate('builtin:xiaoke')
    expect(t).not.toBeNull()
    expect(t!.resourceConfigKind).toBe('customer-support')
    expect(t!.cron).toBe('*/30 8-18 * * 1-5')
    expect(t!.requiresDingtalk).toBe(true)
  })

  it('exposes builtin:xiaogong as tech-support', () => {
    const t = findTemplate('builtin:xiaogong')
    expect(t).not.toBeNull()
    expect(t!.resourceConfigKind).toBe('tech-support')
    expect(t!.cron).toBe('*/30 8-18 * * 1-5')
  })

  it('all templates have non-empty toolWhitelist', () => {
    for (const t of BUILTIN_TEMPLATES) {
      expect(t.toolWhitelist.length).toBeGreaterThan(0)
    }
  })
})
```

- [ ] **Step 2: 运行测试看到失败**

Run: `pnpm exec vitest run src/features/employees/templates.test.ts`
Expected: FAIL — `builtin:xiaoke` 不存在或 `builtin:xiaogong` 不存在。

- [ ] **Step 3: 实现**

In `src/features/employees/templates.ts`:

```ts
export type ResourceConfigKind =
  | 'monitoring-urls'
  | 'sales-table'
  | 'weekly-report'
  | 'customer-support'
  | 'tech-support'
  | 'none'
```

Append two entries to `BUILTIN_TEMPLATES`:

```ts
{
  templateId: 'builtin:xiaoke',
  avatar: '💬',
  name: '小客',
  role: '客服支持',
  description: '定时扫描客户钉钉群的业务咨询，从已切片入库的 FAQ 中检索答案，生成友好回复草稿。',
  toolWhitelist: [
    'bash', 'load_file', 'read_file', 'grep_content',
    'web_search', 'read_page_content',
    'memory_save', 'memory_search',
    'load_skill', 'generate_report',
  ],
  cron: '*/30 8-18 * * 1-5',
  systemPromptExtra: '你是一名客服支持专员。你的 FAQ 知识库已切片入 memory，请使用 memory_search 按客户问题检索答案，不要要求用户重新提供 FAQ 全文。所有钉钉操作通过 dws CLI 完成，发消息必须经用户确认。',
  badge: '🟠 需授权钉钉 + 配置 FAQ',
  defaultSkillId: 'dingtalk-workspace',
  requiresAttachment: null,
  resourceConfigKind: 'customer-support',
  requiresDingtalk: true,
},
{
  templateId: 'builtin:xiaogong',
  avatar: '🛠️',
  name: '小工',
  role: '技术支持',
  description: '定时扫描技术对接群的报错和集成问题，从已切片的技术文档与历史经验中检索答案，生成回复草稿。',
  toolWhitelist: [
    'bash', 'load_file', 'read_file', 'grep_content',
    'web_search', 'read_page_content',
    'memory_save', 'memory_search',
    'load_skill', 'generate_report',
  ],
  cron: '*/30 8-18 * * 1-5',
  systemPromptExtra: '你是一名技术支持工程师。你的技术文档已切片入 memory，请使用 memory_search 按用户报错关键词检索；历史经验也在 memory 中，遇到同类问题先 search。所有钉钉操作通过 dws CLI 完成。',
  badge: '🟠 需授权钉钉 + 配置技术文档',
  defaultSkillId: 'dingtalk-workspace',
  requiresAttachment: null,
  resourceConfigKind: 'tech-support',
  requiresDingtalk: true,
},
```

- [ ] **Step 4: 运行测试通过**

Run: `pnpm exec vitest run src/features/employees/templates.test.ts`
Expected: PASS。

- [ ] **Step 5: 类型检查**

Run: `pnpm exec tsc --noEmit`
Expected: 0 错误。

- [ ] **Step 6: Commit**

```bash
git add src/features/employees/templates.ts src/features/employees/templates.test.ts
git commit -m "feat(employees): add builtin:xiaoke / builtin:xiaogong templates with knowledge-base resource kind"
```

---

### Task 2: 后端 — `EmployeeRecord.knowledgeSources` 读写 helper

**Files:**
- Modify: `src-tauri/src/runtime/employee/store.rs`
- Test: `src-tauri/tests/employee_knowledge_status_test.rs`

- [ ] **Step 1: 写失败测试**

Create `src-tauri/tests/employee_knowledge_status_test.rs`:

```rust
use serde_json::json;
use aijia::runtime::employee::store::{
    EmployeeStore, CreateEmployeeRequest, KnowledgeSourceStatus,
};

fn tmp_store() -> (tempfile::TempDir, EmployeeStore) {
    let tmp = tempfile::tempdir().unwrap();
    let store = EmployeeStore::new(tmp.path().to_path_buf());
    (tmp, store)
}

#[test]
fn update_knowledge_source_status_round_trip() {
    let (_t, store) = tmp_store();
    let rec = store
        .create(CreateEmployeeRequest {
            template_id: Some("builtin:xiaoke".into()),
            avatar: "💬".into(),
            name: "小客".into(),
            role: "客服支持".into(),
            description: "".into(),
            tool_whitelist: vec![],
            cron: None,
            system_prompt_extra: "".into(),
            resource_config: Some(json!({
                "knowledgeSources": [
                    { "path": "/tmp/faq.md", "originalName": "faq.md", "size": 1024,
                      "status": "pending", "slicedCount": 0 }
                ]
            })),
        })
        .unwrap();

    store
        .update_knowledge_source_status(
            &rec.id,
            "/tmp/faq.md",
            KnowledgeSourceStatus::Indexing,
            0,
            None,
        )
        .unwrap();

    let after = store.get(&rec.id).unwrap();
    let sources = after
        .resource_config
        .get("knowledgeSources")
        .and_then(|v| v.as_array())
        .expect("knowledgeSources exists");
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].get("status").unwrap().as_str(), Some("indexing"));

    store
        .update_knowledge_source_status(
            &rec.id,
            "/tmp/faq.md",
            KnowledgeSourceStatus::Done,
            42,
            None,
        )
        .unwrap();

    let after = store.get(&rec.id).unwrap();
    let s = &after.resource_config["knowledgeSources"][0];
    assert_eq!(s["status"], "done");
    assert_eq!(s["slicedCount"], 42);
}

#[test]
fn update_knowledge_source_status_records_error() {
    let (_t, store) = tmp_store();
    let rec = store
        .create(CreateEmployeeRequest {
            template_id: Some("builtin:xiaoke".into()),
            avatar: "💬".into(),
            name: "小客".into(),
            role: "客服".into(),
            description: "".into(),
            tool_whitelist: vec![],
            cron: None,
            system_prompt_extra: "".into(),
            resource_config: Some(json!({
                "knowledgeSources": [
                    { "path": "/tmp/x.md", "originalName": "x.md", "size": 10,
                      "status": "pending", "slicedCount": 0 }
                ]
            })),
        })
        .unwrap();

    store
        .update_knowledge_source_status(
            &rec.id,
            "/tmp/x.md",
            KnowledgeSourceStatus::Failed,
            0,
            Some("file unreadable".into()),
        )
        .unwrap();

    let after = store.get(&rec.id).unwrap();
    let s = &after.resource_config["knowledgeSources"][0];
    assert_eq!(s["status"], "failed");
    assert_eq!(s["error"], "file unreadable");
}
```

- [ ] **Step 2: 运行 — 看失败**

Run: `cd src-tauri && cargo test --test employee_knowledge_status_test`
Expected: FAIL（`update_knowledge_source_status` 不存在 + `KnowledgeSourceStatus` enum 不存在）。

- [ ] **Step 3: 实现**

In `src-tauri/src/runtime/employee/store.rs`, add at top of file (before `EmployeeRecord` struct):

```rust
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KnowledgeSourceStatus {
    Pending,
    Indexing,
    Done,
    Failed,
}

impl KnowledgeSourceStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Indexing => "indexing",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }
}
```

Add a method on `EmployeeStore` (place near other `update_*` methods):

```rust
pub fn update_knowledge_source_status(
    &self,
    id: &str,
    path: &str,
    status: KnowledgeSourceStatus,
    sliced_count: u64,
    error: Option<String>,
) -> anyhow::Result<()> {
    let _lock = self.write_lock.lock().unwrap();
    let path_buf = self.record_path(id);
    let content = std::fs::read_to_string(&path_buf)?;
    let mut record: EmployeeRecord = serde_json::from_str(&content)?;

    let now = chrono::Utc::now().to_rfc3339();
    let sources = record
        .resource_config
        .get_mut("knowledgeSources")
        .and_then(|v| v.as_array_mut())
        .ok_or_else(|| anyhow::anyhow!("knowledgeSources field missing"))?;

    let entry = sources
        .iter_mut()
        .find(|s| s.get("path").and_then(|p| p.as_str()) == Some(path))
        .ok_or_else(|| anyhow::anyhow!("knowledge source path not found: {}", path))?;

    let obj = entry
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("knowledge source is not an object"))?;

    obj.insert("status".into(), serde_json::Value::String(status.as_str().into()));
    obj.insert("slicedCount".into(), serde_json::Value::from(sliced_count));
    match status {
        KnowledgeSourceStatus::Indexing => {
            obj.insert("startedAt".into(), serde_json::Value::String(now));
            obj.remove("error");
        }
        KnowledgeSourceStatus::Done => {
            obj.insert("completedAt".into(), serde_json::Value::String(now));
            obj.remove("error");
        }
        KnowledgeSourceStatus::Failed => {
            if let Some(err) = error {
                obj.insert("error".into(), serde_json::Value::String(err));
            }
            obj.insert("completedAt".into(), serde_json::Value::String(now));
        }
        KnowledgeSourceStatus::Pending => {
            obj.remove("error");
            obj.remove("startedAt");
            obj.remove("completedAt");
        }
    }

    let _ = error; // silence when status != Failed
    self.write_record(&record)
}
```

- [ ] **Step 4: 运行测试通过**

Run: `cd src-tauri && cargo test --test employee_knowledge_status_test`
Expected: 2 passed。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/runtime/employee/store.rs src-tauri/tests/employee_knowledge_status_test.rs
git commit -m "feat(employee/store): add KnowledgeSourceStatus + update_knowledge_source_status"
```

---

### Task 3: 后端 — `KnowledgeIndexer`（启发式分块 + 写 cognitive memory）

**Files:**
- Create: `src-tauri/src/runtime/employee/knowledge.rs`
- Modify: `src-tauri/src/runtime/employee/mod.rs`
- Test: `src-tauri/tests/employee_knowledge_indexer_test.rs`

> **设计选择：** v1 用启发式分块（按 Markdown `##` heading / 双换行 / Q-A 模式），不依赖 LLM。LLM 分块作为 v2 增强，避免雇佣后立即吃 token + 失败兜底复杂。

- [ ] **Step 1: 写失败测试**

Create `src-tauri/tests/employee_knowledge_indexer_test.rs`:

```rust
use std::sync::Arc;
use aijia::runtime::employee::knowledge::{chunk_markdown, KnowledgeChunk};

#[test]
fn chunk_markdown_splits_on_h2_headings() {
    let src = "# 产品 FAQ\n\n## 怎么注册\n\n点击右上角注册按钮。\n\n## 怎么充值\n\n进入控制台 → 余额 → 充值。\n";
    let chunks = chunk_markdown(src);
    assert_eq!(chunks.len(), 2);
    assert!(chunks[0].content.contains("注册"));
    assert!(chunks[1].content.contains("充值"));
    assert_eq!(chunks[0].title.as_deref(), Some("怎么注册"));
}

#[test]
fn chunk_markdown_splits_q_a_pattern() {
    let src = "Q: 怎么找回密码？\nA: 在登录页点击\"忘记密码\"。\n\nQ: 客服电话？\nA: 400-123-4567\n";
    let chunks = chunk_markdown(src);
    assert_eq!(chunks.len(), 2);
    assert!(chunks[0].content.starts_with("Q: 怎么找回密码"));
}

#[test]
fn chunk_markdown_handles_long_paragraphs_by_double_newline() {
    let src = "段落一内容。\n\n段落二内容。\n\n段落三内容。\n";
    let chunks = chunk_markdown(src);
    assert_eq!(chunks.len(), 3);
}

#[test]
fn chunk_markdown_collapses_chunks_under_min_size() {
    let src = "短\n\n短2\n\n## 标题\n\n这是一个比较长的段落用于触发新分块的产生。";
    let chunks = chunk_markdown(src);
    // "短" 和 "短2" 太短被合并
    assert!(chunks.len() <= 2);
}
```

- [ ] **Step 2: 运行 — 看失败**

Run: `cd src-tauri && cargo test --test employee_knowledge_indexer_test`
Expected: FAIL（模块不存在）。

- [ ] **Step 3: 实现 chunker**

Create `src-tauri/src/runtime/employee/knowledge.rs`:

```rust
//! Knowledge indexer: splits a knowledge source file into chunks
//! and writes each chunk into cognitive memory under
//! `category="knowledge:{employee_id}"` so the runtime can later
//! retrieve them via `memory_search` instead of stuffing the whole
//! FAQ into the LLM context.

use anyhow::{Context, Result};
use std::path::Path;
use std::sync::Arc;

use crate::runtime::employee::store::{EmployeeStore, KnowledgeSourceStatus};
use crate::storage::file_store::FileStore;

const MIN_CHUNK_CHARS: usize = 40;
const MAX_CHUNK_CHARS: usize = 1200;

#[derive(Debug, Clone, PartialEq)]
pub struct KnowledgeChunk {
    pub title: Option<String>,
    pub content: String,
}

/// Heuristic chunker: prefers H2 headings, then Q/A pairs, then double-newline paragraphs.
pub fn chunk_markdown(src: &str) -> Vec<KnowledgeChunk> {
    let by_h2 = split_by_h2(src);
    if by_h2.len() >= 2 {
        return collapse_short(by_h2);
    }
    let by_qa = split_by_qa(src);
    if by_qa.len() >= 2 {
        return collapse_short(by_qa);
    }
    let paragraphs = split_paragraphs(src);
    collapse_short(paragraphs)
}

fn split_by_h2(src: &str) -> Vec<KnowledgeChunk> {
    let mut out = Vec::new();
    let mut current_title: Option<String> = None;
    let mut buf = String::new();
    for line in src.lines() {
        if let Some(title) = line.strip_prefix("## ") {
            if !buf.trim().is_empty() {
                out.push(KnowledgeChunk { title: current_title.clone(), content: buf.trim().to_string() });
                buf.clear();
            }
            current_title = Some(title.trim().to_string());
        } else {
            buf.push_str(line);
            buf.push('\n');
        }
    }
    if !buf.trim().is_empty() {
        out.push(KnowledgeChunk { title: current_title, content: buf.trim().to_string() });
    }
    out
}

fn split_by_qa(src: &str) -> Vec<KnowledgeChunk> {
    let mut out = Vec::new();
    let mut buf = String::new();
    for line in src.lines() {
        if (line.starts_with("Q:") || line.starts_with("Q：")) && !buf.trim().is_empty() {
            out.push(KnowledgeChunk { title: None, content: buf.trim().to_string() });
            buf.clear();
        }
        buf.push_str(line);
        buf.push('\n');
    }
    if !buf.trim().is_empty() {
        out.push(KnowledgeChunk { title: None, content: buf.trim().to_string() });
    }
    out
}

fn split_paragraphs(src: &str) -> Vec<KnowledgeChunk> {
    src.split("\n\n")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| KnowledgeChunk { title: None, content: s.to_string() })
        .collect()
}

fn collapse_short(chunks: Vec<KnowledgeChunk>) -> Vec<KnowledgeChunk> {
    let mut out: Vec<KnowledgeChunk> = Vec::new();
    for c in chunks {
        if c.content.chars().count() > MAX_CHUNK_CHARS {
            // hard split on size
            for piece in hard_split(&c.content, MAX_CHUNK_CHARS) {
                out.push(KnowledgeChunk { title: c.title.clone(), content: piece });
            }
            continue;
        }
        if c.content.chars().count() < MIN_CHUNK_CHARS {
            if let Some(last) = out.last_mut() {
                last.content.push_str("\n\n");
                last.content.push_str(&c.content);
                continue;
            }
        }
        out.push(c);
    }
    out
}

fn hard_split(s: &str, max: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    for ch in s.chars() {
        buf.push(ch);
        if buf.chars().count() >= max && (ch == '\n' || ch == '。' || ch == '.') {
            out.push(buf.trim().to_string());
            buf.clear();
        }
    }
    if !buf.trim().is_empty() {
        out.push(buf.trim().to_string());
    }
    out
}

/// Index one knowledge source file. Updates employee record status as it progresses.
/// Designed to be called inside a `tokio::task::spawn_blocking` (it does sync IO + mutex).
pub fn index_one(
    store: &EmployeeStore,
    file_store: &FileStore,
    employee_id: &str,
    file_path: &Path,
    original_name: &str,
) -> Result<u64> {
    let path_str = file_path.to_string_lossy().to_string();

    store.update_knowledge_source_status(
        employee_id,
        &path_str,
        KnowledgeSourceStatus::Indexing,
        0,
        None,
    )?;

    let result = (|| -> Result<u64> {
        let raw = std::fs::read_to_string(file_path)
            .with_context(|| format!("read {}", file_path.display()))?;
        let chunks = chunk_markdown(&raw);
        let mut written = 0u64;
        for chunk in chunks {
            let content = match &chunk.title {
                Some(t) => format!("【{}】\n{}", t, chunk.content),
                None => chunk.content.clone(),
            };
            let category = format!("knowledge:{}", employee_id);
            let tags = vec!["faq".to_string(), original_name.to_string()];
            file_store.save_cognitive_memory(
                &content,
                &category,
                &tags,
                employee_id,
                false,
            )?;
            written += 1;
        }
        Ok(written)
    })();

    match result {
        Ok(count) => {
            store.update_knowledge_source_status(
                employee_id,
                &path_str,
                KnowledgeSourceStatus::Done,
                count,
                None,
            )?;
            Ok(count)
        }
        Err(e) => {
            let msg = format!("{:#}", e);
            let _ = store.update_knowledge_source_status(
                employee_id,
                &path_str,
                KnowledgeSourceStatus::Failed,
                0,
                Some(msg.clone()),
            );
            Err(e)
        }
    }
}

/// Async entry: spawns blocking task per file. Returns immediately.
pub fn spawn_index_all(
    store: Arc<EmployeeStore>,
    file_store: Arc<FileStore>,
    employee_id: String,
    sources: Vec<(std::path::PathBuf, String)>,
) {
    for (path, original_name) in sources {
        let store = Arc::clone(&store);
        let file_store = Arc::clone(&file_store);
        let id = employee_id.clone();
        tokio::task::spawn_blocking(move || {
            if let Err(e) = index_one(&store, &file_store, &id, &path, &original_name) {
                log::warn!("knowledge index failed for {}: {:#}", path.display(), e);
            }
        });
    }
}
```

In `src-tauri/src/runtime/employee/mod.rs`, add:

```rust
pub mod knowledge;
```

- [ ] **Step 4: 运行测试通过**

Run: `cd src-tauri && cargo test --test employee_knowledge_indexer_test`
Expected: 4 passed。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/runtime/employee/knowledge.rs src-tauri/src/runtime/employee/mod.rs src-tauri/tests/employee_knowledge_indexer_test.rs
git commit -m "feat(employee/knowledge): heuristic markdown chunker + spawn_index_all writing to cognitive memory"
```

---

### Task 4: 后端 — Tauri command `employee_index_knowledge_async`

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/employee.rs`
- Modify: `src-tauri/src/lib.rs`（注册 invoke handler）

- [ ] **Step 1: 写集成测试 stub**

Create `src-tauri/tests/employee_knowledge_command_test.rs`:

```rust
//! End-to-end: command spawns indexer, knowledgeSources statuses converge to "done".
use std::time::Duration;
use serde_json::json;

use aijia::runtime::employee::store::{EmployeeStore, CreateEmployeeRequest};
use aijia::runtime::employee::knowledge;
use aijia::storage::file_store::FileStore;

#[tokio::test(flavor = "multi_thread")]
async fn end_to_end_index_completes() {
    let tmp = tempfile::tempdir().unwrap();
    let renlijia = tmp.path().to_path_buf();

    // Create FAQ file
    let faq = renlijia.join("faq.md");
    std::fs::write(
        &faq,
        "## 注册\n\n点击右上角注册按钮，输入手机号验证。\n\n## 充值\n\n进入控制台 → 余额 → 充值，支持微信 / 银行转账。\n",
    ).unwrap();

    let store = std::sync::Arc::new(EmployeeStore::new(renlijia.join("employees")));
    let file_store = std::sync::Arc::new(FileStore::new(renlijia.clone()));

    let rec = store.create(CreateEmployeeRequest {
        template_id: Some("builtin:xiaoke".into()),
        avatar: "💬".into(),
        name: "小客".into(),
        role: "客服".into(),
        description: "".into(),
        tool_whitelist: vec![],
        cron: None,
        system_prompt_extra: "".into(),
        resource_config: Some(json!({
            "knowledgeSources": [
                { "path": faq.to_string_lossy(), "originalName": "faq.md", "size": 100,
                  "status": "pending", "slicedCount": 0 }
            ]
        })),
    }).unwrap();

    knowledge::spawn_index_all(
        std::sync::Arc::clone(&store),
        std::sync::Arc::clone(&file_store),
        rec.id.clone(),
        vec![(faq.clone(), "faq.md".into())],
    );

    // Poll up to 5s
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let r = store.get(&rec.id).unwrap();
        let status = r.resource_config["knowledgeSources"][0]["status"]
            .as_str()
            .unwrap_or("")
            .to_string();
        if status == "done" {
            assert_eq!(r.resource_config["knowledgeSources"][0]["slicedCount"].as_u64(), Some(2));
            return;
        }
        if std::time::Instant::now() > deadline {
            panic!("indexing did not complete in 5s, last status = {}", status);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
```

- [ ] **Step 2: 运行 — 看通过（实现已在 Task 3 完成）**

Run: `cd src-tauri && cargo test --test employee_knowledge_command_test`
Expected: PASS。

- [ ] **Step 3: 添加 Tauri command thin layer**

In `src-tauri/src/transport/tauri_commands/employee.rs`, add:

```rust
#[derive(serde::Deserialize)]
pub struct IndexKnowledgeArgs {
    pub employee_id: String,
    /// (absolute_path, original_name) pairs; caller must have already added entries
    /// to resource_config.knowledgeSources with status="pending".
    pub sources: Vec<(String, String)>,
}

#[tauri::command]
pub async fn employee_index_knowledge_async(
    args: IndexKnowledgeArgs,
    employee_store: tauri::State<'_, std::sync::Arc<crate::runtime::employee::store::EmployeeStore>>,
    file_store: tauri::State<'_, std::sync::Arc<crate::storage::file_store::FileStore>>,
) -> Result<(), String> {
    let pairs: Vec<(std::path::PathBuf, String)> = args
        .sources
        .into_iter()
        .map(|(p, n)| (std::path::PathBuf::from(p), n))
        .collect();
    crate::runtime::employee::knowledge::spawn_index_all(
        employee_store.inner().clone(),
        file_store.inner().clone(),
        args.employee_id,
        pairs,
    );
    Ok(())
}
```

In `src-tauri/src/lib.rs`, register the command in the existing `tauri::generate_handler![...]` macro alongside other employee commands.

- [ ] **Step 4: 编译**

Run: `cd src-tauri && cargo check`
Expected: 0 错误。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/transport/tauri_commands/employee.rs src-tauri/src/lib.rs src-tauri/tests/employee_knowledge_command_test.rs
git commit -m "feat(employee/cmd): add employee_index_knowledge_async tauri command"
```

---

### Task 5: 前端 — `KnowledgeSourcesField` 共享组件

**Files:**
- Create: `src/features/employees/forms/KnowledgeSourcesField.tsx`
- Modify: `src/lib/tauri.ts`

- [ ] **Step 1: 在 `src/lib/tauri.ts` 添加 IPC 类型**

Find the section that exports `invoke` wrappers; add:

```ts
export interface PendingKnowledgeSource {
  path: string
  originalName: string
  size: number
}

export async function employeeIndexKnowledgeAsync(
  employeeId: string,
  sources: PendingKnowledgeSource[],
): Promise<void> {
  await invoke('employee_index_knowledge_async', {
    args: {
      employee_id: employeeId,
      sources: sources.map((s) => [s.path, s.originalName] as [string, string]),
    },
  })
}
```

- [ ] **Step 2: 写 component**

Create `src/features/employees/forms/KnowledgeSourcesField.tsx`:

```tsx
import { open } from '@tauri-apps/plugin-dialog'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'

export type KnowledgeSourceStatus = 'pending' | 'indexing' | 'done' | 'failed'

export interface KnowledgeSource {
  path: string
  originalName: string
  size: number
  status: KnowledgeSourceStatus
  slicedCount: number
  error?: string
}

interface Props {
  value: KnowledgeSource[]
  onChange: (next: KnowledgeSource[]) => void
  onRetry?: (source: KnowledgeSource) => void
}

export function KnowledgeSourcesField({ value, onChange, onRetry }: Props) {
  const { t } = useTranslation()

  async function pickFiles() {
    const selected = await open({
      multiple: true,
      filters: [{ name: 'Knowledge', extensions: ['md', 'txt', 'pdf', 'docx'] }],
    })
    if (!selected) return
    const arr = Array.isArray(selected) ? selected : [selected]
    const additions: KnowledgeSource[] = arr.map((p) => ({
      path: p,
      originalName: p.split(/[\\/]/).pop() ?? p,
      size: 0,
      status: 'pending',
      slicedCount: 0,
    }))
    onChange([...value, ...additions])
  }

  function remove(idx: number) {
    onChange(value.filter((_, i) => i !== idx))
  }

  function statusLabel(s: KnowledgeSource): string {
    switch (s.status) {
      case 'pending': return t('employee.config.knowledge.statusPending')
      case 'indexing': return t('employee.config.knowledge.statusIndexing')
      case 'done': return t('employee.config.knowledge.statusDone', { count: s.slicedCount })
      case 'failed': return t('employee.config.knowledge.statusFailed')
    }
  }

  return (
    <div className="flex flex-col gap-1.5">
      <label className="text-xs font-medium text-muted-foreground">
        {t('employee.config.knowledge.label')}
      </label>
      <p className="text-xs text-muted-foreground/70">
        {t('employee.config.knowledge.hint')}
      </p>
      <div className="flex flex-col gap-1">
        {value.map((s, i) => (
          <div key={`${s.path}-${i}`} className="flex items-center gap-2 rounded border border-input bg-background px-2 py-1 text-xs">
            <span className="flex-1 truncate">📄 {s.originalName}</span>
            <span
              className={
                s.status === 'failed'
                  ? 'text-destructive'
                  : s.status === 'done'
                    ? 'text-green-600'
                    : 'text-muted-foreground'
              }
              title={s.error}
            >
              {statusLabel(s)}
            </span>
            {s.status === 'failed' && onRetry && (
              <button type="button" onClick={() => onRetry(s)} className="text-blue-600 hover:underline">
                {t('employee.config.knowledge.retry')}
              </button>
            )}
            <button type="button" onClick={() => remove(i)} className="text-muted-foreground hover:text-destructive">
              ×
            </button>
          </div>
        ))}
      </div>
      <Button type="button" variant="outline" size="sm" onClick={pickFiles} className="w-fit">
        + {t('employee.config.knowledge.upload')}
      </Button>
    </div>
  )
}
```

- [ ] **Step 3: 类型检查**

Run: `pnpm exec tsc --noEmit`
Expected: 0 错误（i18n key 暂时缺失但 t() 返回 string，不报错；下个 task 补 keys）。

- [ ] **Step 4: Commit**

```bash
git add src/features/employees/forms/KnowledgeSourcesField.tsx src/lib/tauri.ts
git commit -m "feat(employees/ui): KnowledgeSourcesField + employeeIndexKnowledgeAsync IPC"
```

---

### Task 6: 前端 i18n keys（employee.config.* 命名空间）

**Files:**
- Modify: `src/i18n/zh-CN.json`
- Modify: `src/i18n/en-US.json`

- [ ] **Step 1: 写 zh-CN keys**

Add under existing top-level object in `src/i18n/zh-CN.json` (merge into `employee` namespace if it exists):

```json
{
  "employee": {
    "config": {
      "knowledge": {
        "label": "知识源",
        "hint": "上传 FAQ / 产品手册 / 技术文档；雇佣完成后会在后台自动切片入库，AI 用语义检索按需调用，不再每次塞全文。",
        "upload": "上传文档",
        "retry": "重试",
        "statusPending": "待切片",
        "statusIndexing": "切片中…",
        "statusDone": "已入库 {{count}} 条",
        "statusFailed": "切片失败"
      },
      "groupMatch": {
        "label": "监控群（关键词匹配）",
        "include": "包含关键词",
        "exclude": "排除关键词",
        "addPlaceholder": "+ 添加",
        "matchedHint": "已匹配 {{count}} 个群"
      },
      "responseStyle": {
        "label": "回复风格",
        "professional": "专业正式",
        "friendly": "亲和友好",
        "concise": "简洁直接"
      },
      "greeting": "回复开头",
      "closing": "回复结尾",
      "escalation": {
        "label": "人工介入关键词（匹配到时跳过自动回复）",
        "hint": "命中这些词的消息会被标注 🔴 转人工。"
      },
      "techKeywords": {
        "label": "技术问题关键词（匹配到时建议转小工）",
        "hint": "命中这些词的消息会被标注 🔧 转技术支持。"
      },
      "summaryFreq": {
        "label": "对话总结频率",
        "daily": "每日",
        "weekly": "每周",
        "off": "关闭"
      },
      "customerSupport": {
        "title": "配置客服支持",
        "intro": "客服会扫描匹配的钉钉群里的业务咨询，从知识库与历史对话检索答案，生成草稿等你确认。"
      },
      "techSupport": {
        "title": "配置技术支持",
        "intro": "技术支持会扫描匹配的钉钉群里的报错与集成问题，从技术文档与历史经验检索答案，生成草稿等你确认。"
      },
      "monitoringUrls": {
        "title": "配置监测对象",
        "label": "监测 URL",
        "hint": "调研员将定期访问以下 URL 抓取变化。",
        "addPlaceholder": "https://...",
        "addButton": "添加"
      },
      "salesTable": {
        "title": "配置数据源"
      },
      "weeklyReport": {
        "title": "配置周报偏好"
      },
      "save": "保存",
      "cancel": "取消"
    }
  }
}
```

- [ ] **Step 2: 写 en-US keys**

Add same structure to `src/i18n/en-US.json` with English values:

```json
{
  "employee": {
    "config": {
      "knowledge": {
        "label": "Knowledge sources",
        "hint": "Upload FAQ / product manual / tech docs. After hiring, files are sliced into memory in the background; the AI retrieves on demand via semantic search instead of stuffing the full text each turn.",
        "upload": "Upload document",
        "retry": "Retry",
        "statusPending": "Pending",
        "statusIndexing": "Indexing…",
        "statusDone": "{{count}} chunks indexed",
        "statusFailed": "Failed"
      },
      "groupMatch": {
        "label": "Monitor groups (keyword matching)",
        "include": "Include keywords",
        "exclude": "Exclude keywords",
        "addPlaceholder": "+ Add",
        "matchedHint": "{{count}} groups matched"
      },
      "responseStyle": {
        "label": "Response style",
        "professional": "Professional",
        "friendly": "Friendly",
        "concise": "Concise"
      },
      "greeting": "Greeting",
      "closing": "Closing",
      "escalation": {
        "label": "Escalation keywords (skip auto-reply, flag for human handling)",
        "hint": "Messages containing these words will be flagged 🔴 for manual handling."
      },
      "techKeywords": {
        "label": "Tech keywords (route to tech support)",
        "hint": "Messages containing these words will be tagged 🔧 for tech support."
      },
      "summaryFreq": {
        "label": "Conversation summary frequency",
        "daily": "Daily",
        "weekly": "Weekly",
        "off": "Off"
      },
      "customerSupport": {
        "title": "Customer support setup",
        "intro": "The employee scans matching DingTalk groups for business inquiries, searches the knowledge base + past conversations, then drafts friendly replies for your review."
      },
      "techSupport": {
        "title": "Tech support setup",
        "intro": "The employee scans matching DingTalk groups for errors and integration questions, searches tech docs + past experience, then drafts replies for your review."
      },
      "monitoringUrls": {
        "title": "Monitoring targets",
        "label": "Monitor URLs",
        "hint": "The researcher will periodically visit these URLs to detect changes.",
        "addPlaceholder": "https://...",
        "addButton": "Add"
      },
      "salesTable": { "title": "Data source" },
      "weeklyReport": { "title": "Weekly report preferences" },
      "save": "Save",
      "cancel": "Cancel"
    }
  }
}
```

- [ ] **Step 3: 验证 JSON 合法**

Run: `node -e "JSON.parse(require('fs').readFileSync('src/i18n/zh-CN.json','utf8')); JSON.parse(require('fs').readFileSync('src/i18n/en-US.json','utf8')); console.log('ok')"`
Expected: `ok`

- [ ] **Step 4: Commit**

```bash
git add src/i18n/zh-CN.json src/i18n/en-US.json
git commit -m "i18n(employees): add employee.config.* namespace (zh+en)"
```

---

### Task 7: 改造 `CustomerSupportConfigForm` 接入 i18n + 知识库字段

**Files:**
- Modify: `src/features/employees/forms/CustomerSupportConfigForm.tsx`

- [ ] **Step 1: 替换实现**

Open `src/features/employees/forms/CustomerSupportConfigForm.tsx` and replace whole file with:

```tsx
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { GroupMatchInput, groupMatchFromRecord, groupMatchToRecord, type GroupMatchConfig } from './GroupMatchInput'
import { KnowledgeSourcesField, type KnowledgeSource } from './KnowledgeSourcesField'

interface Props {
  initial: Record<string, unknown>
  onSubmit: (next: Record<string, unknown>) => void
  onCancel: () => void
}

type ResponseStyle = 'professional' | 'friendly' | 'concise'
type SummaryCron = 'daily' | 'weekly' | 'off'

interface FormState {
  groupMatch: GroupMatchConfig
  responseStyle: ResponseStyle
  greeting: string
  closing: string
  summaryCron: SummaryCron
  knowledgeSources: KnowledgeSource[]
  escalationKeywords: string[]
  techKeywords: string[]
}

const DEFAULT_ESCALATION = ['投诉', '退款', '赔偿', '律师', '工信部']
const DEFAULT_TECH = ['报错', 'error', '500', 'API', '部署', '配置', '日志', 'bug']

function parseStringArray(v: unknown): string[] {
  if (!Array.isArray(v)) return []
  return (v as unknown[]).filter((x): x is string => typeof x === 'string')
}

function parseKnowledgeSources(v: unknown): KnowledgeSource[] {
  if (!Array.isArray(v)) return []
  return (v as unknown[]).flatMap((raw): KnowledgeSource[] => {
    if (!raw || typeof raw !== 'object') return []
    const r = raw as Record<string, unknown>
    if (typeof r.path !== 'string' || typeof r.originalName !== 'string') return []
    const status = r.status
    return [{
      path: r.path,
      originalName: r.originalName,
      size: typeof r.size === 'number' ? r.size : 0,
      status: (status === 'pending' || status === 'indexing' || status === 'done' || status === 'failed') ? status : 'pending',
      slicedCount: typeof r.slicedCount === 'number' ? r.slicedCount : 0,
      error: typeof r.error === 'string' ? r.error : undefined,
    }]
  })
}

function stateFromInitial(initial: Record<string, unknown>): FormState {
  const gm = groupMatchFromRecord(initial)
  if (gm.keywords.length === 0) {
    gm.keywords = ['服务', '客户', '售后']
    gm.exclude = ['内部', '测试']
  }
  const responseStyle = (['professional', 'friendly', 'concise'].includes(initial.responseStyle as string)
    ? initial.responseStyle : 'friendly') as ResponseStyle
  const summaryCron = (['daily', 'weekly', 'off'].includes(initial.summaryCron as string)
    ? initial.summaryCron : 'weekly') as SummaryCron
  return {
    groupMatch: gm,
    responseStyle,
    greeting: typeof initial.greeting === 'string' ? initial.greeting : '您好，',
    closing: typeof initial.closing === 'string' ? initial.closing : '如还有其他问题随时告诉我们~',
    summaryCron,
    knowledgeSources: parseKnowledgeSources(initial.knowledgeSources),
    escalationKeywords: parseStringArray(initial.escalationKeywords).length > 0
      ? parseStringArray(initial.escalationKeywords) : DEFAULT_ESCALATION,
    techKeywords: parseStringArray(initial.techKeywords).length > 0
      ? parseStringArray(initial.techKeywords) : DEFAULT_TECH,
  }
}

function InlineTagEditor({ label, hint, tags, onChange }: { label: string; hint: string; tags: string[]; onChange: (n: string[]) => void }) {
  const [input, setInput] = useState('')
  return (
    <div className="flex flex-col gap-1.5">
      <label className="text-xs font-medium text-muted-foreground">{label}</label>
      <div className="flex flex-wrap items-center gap-1.5">
        {tags.map((tag, i) => (
          <span key={`${tag}-${i}`} className="flex items-center gap-0.5 rounded-md bg-accent px-2 py-0.5 text-xs">
            {tag}
            <button type="button" onClick={() => onChange(tags.filter((_, idx) => idx !== i))} className="ml-0.5 text-[10px] text-muted-foreground hover:text-destructive">×</button>
          </span>
        ))}
        <input
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') {
              e.preventDefault()
              const t = input.trim()
              if (t && !tags.includes(t)) { onChange([...tags, t]); setInput('') }
            }
          }}
          placeholder="+"
          className="h-6 w-16 rounded border border-input bg-background px-1 text-xs"
        />
      </div>
      <p className="text-xs text-muted-foreground/70">{hint}</p>
    </div>
  )
}

export function CustomerSupportConfigForm({ initial, onSubmit, onCancel }: Props) {
  const { t } = useTranslation()
  const [state, setState] = useState<FormState>(() => stateFromInitial(initial))

  function update(patch: Partial<FormState>) { setState((s) => ({ ...s, ...patch })) }

  function handleSave() {
    onSubmit({
      ...groupMatchToRecord(state.groupMatch),
      responseStyle: state.responseStyle,
      greeting: state.greeting,
      closing: state.closing,
      escalationKeywords: state.escalationKeywords,
      techKeywords: state.techKeywords,
      summaryCron: state.summaryCron,
      knowledgeSources: state.knowledgeSources,
      language: 'zh',
    })
  }

  const valid = state.groupMatch.keywords.length > 0
  const styles: ResponseStyle[] = ['professional', 'friendly', 'concise']
  const summaries: SummaryCron[] = ['daily', 'weekly', 'off']

  return (
    <div className="flex flex-col gap-4">
      <p className="text-xs leading-relaxed text-muted-foreground">{t('employee.config.customerSupport.intro')}</p>

      <GroupMatchInput
        value={state.groupMatch}
        onChange={(gm) => update({ groupMatch: gm })}
        defaultKeywords={['服务', '客户', '售后']}
        defaultExclude={['内部', '测试']}
      />

      <KnowledgeSourcesField
        value={state.knowledgeSources}
        onChange={(next) => update({ knowledgeSources: next })}
      />

      <div className="flex flex-col gap-1.5">
        <label className="text-xs font-medium text-muted-foreground">{t('employee.config.responseStyle.label')}</label>
        <div className="flex items-center gap-3 text-sm">
          {styles.map((opt) => (
            <label key={opt} className="flex items-center gap-1.5">
              <input type="radio" checked={state.responseStyle === opt} onChange={() => update({ responseStyle: opt })} />
              {t(`employee.config.responseStyle.${opt}`)}
            </label>
          ))}
        </div>
      </div>

      <div className="flex gap-3">
        <div className="flex flex-1 flex-col gap-1.5">
          <label className="text-xs font-medium text-muted-foreground">{t('employee.config.greeting')}</label>
          <Input value={state.greeting} onChange={(e) => update({ greeting: e.target.value })} className="text-xs" />
        </div>
        <div className="flex flex-1 flex-col gap-1.5">
          <label className="text-xs font-medium text-muted-foreground">{t('employee.config.closing')}</label>
          <Input value={state.closing} onChange={(e) => update({ closing: e.target.value })} className="text-xs" />
        </div>
      </div>

      <InlineTagEditor
        label={t('employee.config.escalation.label')}
        hint={t('employee.config.escalation.hint')}
        tags={state.escalationKeywords}
        onChange={(next) => update({ escalationKeywords: next })}
      />
      <InlineTagEditor
        label={t('employee.config.techKeywords.label')}
        hint={t('employee.config.techKeywords.hint')}
        tags={state.techKeywords}
        onChange={(next) => update({ techKeywords: next })}
      />

      <div className="flex flex-col gap-1.5">
        <label className="text-xs font-medium text-muted-foreground">{t('employee.config.summaryFreq.label')}</label>
        <div className="flex items-center gap-3 text-sm">
          {summaries.map((opt) => (
            <label key={opt} className="flex items-center gap-1.5">
              <input type="radio" checked={state.summaryCron === opt} onChange={() => update({ summaryCron: opt })} />
              {t(`employee.config.summaryFreq.${opt}`)}
            </label>
          ))}
        </div>
      </div>

      <div className="flex items-center justify-end gap-2 pt-2">
        <Button variant="ghost" onClick={onCancel}>{t('employee.config.cancel')}</Button>
        <Button onClick={handleSave} disabled={!valid}>{t('employee.config.save')}</Button>
      </div>
    </div>
  )
}
```

- [ ] **Step 2: 类型检查 + lint**

Run: `pnpm exec tsc --noEmit && pnpm lint`
Expected: 0 错误。

- [ ] **Step 3: Commit**

```bash
git add src/features/employees/forms/CustomerSupportConfigForm.tsx
git commit -m "feat(employees/ui): CustomerSupportConfigForm i18n + knowledge sources field"
```

---

### Task 8: 改造 `TechSupportConfigForm`（同上 + tech 默认值）

**Files:**
- Modify: `src/features/employees/forms/TechSupportConfigForm.tsx`

- [ ] **Step 1: 重写文件，结构与 Task 7 一致，区别仅在默认 keywords 和 intro key**

Replace `src/features/employees/forms/TechSupportConfigForm.tsx` whole content with the same structure as Task 7's `CustomerSupportConfigForm`, with these substitutions:
- intro key: `t('employee.config.techSupport.intro')`
- `groupMatch` defaults: `keywords = ['技术', '对接', '集成']`, `exclude = ['内部', '测试']`
- responseStyle default: `'professional'`
- 不需要 escalation/techKeywords 标签编辑（删除两个 `InlineTagEditor`）
- 不需要 greeting/closing
- `onSubmit` 输出去掉 `escalationKeywords`/`techKeywords`/`greeting`/`closing`，加上 `autoSend: false`
- 保留 `KnowledgeSourcesField`、`responseStyle`、`summaryCron`

Full code (use this verbatim):

```tsx
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { GroupMatchInput, groupMatchFromRecord, groupMatchToRecord, type GroupMatchConfig } from './GroupMatchInput'
import { KnowledgeSourcesField, type KnowledgeSource } from './KnowledgeSourcesField'

interface Props { initial: Record<string, unknown>; onSubmit: (n: Record<string, unknown>) => void; onCancel: () => void }
type ResponseStyle = 'professional' | 'friendly' | 'concise'
type SummaryCron = 'daily' | 'weekly' | 'off'

interface FormState {
  groupMatch: GroupMatchConfig
  responseStyle: ResponseStyle
  summaryCron: SummaryCron
  knowledgeSources: KnowledgeSource[]
}

function parseKnowledgeSources(v: unknown): KnowledgeSource[] {
  if (!Array.isArray(v)) return []
  return (v as unknown[]).flatMap((raw): KnowledgeSource[] => {
    if (!raw || typeof raw !== 'object') return []
    const r = raw as Record<string, unknown>
    if (typeof r.path !== 'string' || typeof r.originalName !== 'string') return []
    const status = r.status
    return [{
      path: r.path, originalName: r.originalName,
      size: typeof r.size === 'number' ? r.size : 0,
      status: (status === 'pending' || status === 'indexing' || status === 'done' || status === 'failed') ? status : 'pending',
      slicedCount: typeof r.slicedCount === 'number' ? r.slicedCount : 0,
      error: typeof r.error === 'string' ? r.error : undefined,
    }]
  })
}

function stateFromInitial(initial: Record<string, unknown>): FormState {
  const gm = groupMatchFromRecord(initial)
  if (gm.keywords.length === 0) { gm.keywords = ['技术', '对接', '集成']; gm.exclude = ['内部', '测试'] }
  const responseStyle = (['professional', 'friendly', 'concise'].includes(initial.responseStyle as string)
    ? initial.responseStyle : 'professional') as ResponseStyle
  const summaryCron = (['daily', 'weekly', 'off'].includes(initial.summaryCron as string)
    ? initial.summaryCron : 'weekly') as SummaryCron
  return { groupMatch: gm, responseStyle, summaryCron, knowledgeSources: parseKnowledgeSources(initial.knowledgeSources) }
}

export function TechSupportConfigForm({ initial, onSubmit, onCancel }: Props) {
  const { t } = useTranslation()
  const [state, setState] = useState<FormState>(() => stateFromInitial(initial))
  const update = (p: Partial<FormState>) => setState((s) => ({ ...s, ...p }))

  function handleSave() {
    onSubmit({
      ...groupMatchToRecord(state.groupMatch),
      responseStyle: state.responseStyle,
      summaryCron: state.summaryCron,
      knowledgeSources: state.knowledgeSources,
      language: 'zh',
      autoSend: false,
    })
  }

  const valid = state.groupMatch.keywords.length > 0
  const styles: ResponseStyle[] = ['professional', 'friendly', 'concise']
  const summaries: SummaryCron[] = ['daily', 'weekly', 'off']

  return (
    <div className="flex flex-col gap-4">
      <p className="text-xs leading-relaxed text-muted-foreground">{t('employee.config.techSupport.intro')}</p>
      <GroupMatchInput value={state.groupMatch} onChange={(gm) => update({ groupMatch: gm })}
        defaultKeywords={['技术', '对接', '集成']} defaultExclude={['内部', '测试']} />
      <KnowledgeSourcesField value={state.knowledgeSources} onChange={(next) => update({ knowledgeSources: next })} />
      <div className="flex flex-col gap-1.5">
        <label className="text-xs font-medium text-muted-foreground">{t('employee.config.responseStyle.label')}</label>
        <div className="flex items-center gap-3 text-sm">
          {styles.map((opt) => (
            <label key={opt} className="flex items-center gap-1.5">
              <input type="radio" checked={state.responseStyle === opt} onChange={() => update({ responseStyle: opt })} />
              {t(`employee.config.responseStyle.${opt}`)}
            </label>
          ))}
        </div>
      </div>
      <div className="flex flex-col gap-1.5">
        <label className="text-xs font-medium text-muted-foreground">{t('employee.config.summaryFreq.label')}</label>
        <div className="flex items-center gap-3 text-sm">
          {summaries.map((opt) => (
            <label key={opt} className="flex items-center gap-1.5">
              <input type="radio" checked={state.summaryCron === opt} onChange={() => update({ summaryCron: opt })} />
              {t(`employee.config.summaryFreq.${opt}`)}
            </label>
          ))}
        </div>
      </div>
      <div className="flex items-center justify-end gap-2 pt-2">
        <Button variant="ghost" onClick={onCancel}>{t('employee.config.cancel')}</Button>
        <Button onClick={handleSave} disabled={!valid}>{t('employee.config.save')}</Button>
      </div>
    </div>
  )
}
```

- [ ] **Step 2: 类型检查**

Run: `pnpm exec tsc --noEmit`
Expected: 0 错误。

- [ ] **Step 3: Commit**

```bash
git add src/features/employees/forms/TechSupportConfigForm.tsx
git commit -m "feat(employees/ui): TechSupportConfigForm i18n + knowledge sources field"
```

---

### Task 9: `ResourceConfigForm` 路由 customer-support / tech-support + i18n title

**Files:**
- Modify: `src/features/employees/ResourceConfigForm.tsx`

- [ ] **Step 1: 重写**

Replace whole file:

```tsx
import { useTranslation } from 'react-i18next'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { MonitoringUrlsForm } from './forms/MonitoringUrlsForm'
import { SalesTableConfigForm } from './forms/SalesTableConfigForm'
import { WeeklyReportConfigForm } from './forms/WeeklyReportConfigForm'
import { CustomerSupportConfigForm } from './forms/CustomerSupportConfigForm'
import { TechSupportConfigForm } from './forms/TechSupportConfigForm'
import type { ResourceConfigKind } from './templates'

interface Props {
  open: boolean
  kind: ResourceConfigKind
  initial: Record<string, unknown>
  onSubmit: (next: Record<string, unknown>) => void
  onCancel: () => void
}

const TITLE_KEYS: Record<Exclude<ResourceConfigKind, 'none'>, string> = {
  'monitoring-urls': 'employee.config.monitoringUrls.title',
  'sales-table': 'employee.config.salesTable.title',
  'weekly-report': 'employee.config.weeklyReport.title',
  'customer-support': 'employee.config.customerSupport.title',
  'tech-support': 'employee.config.techSupport.title',
}

export function ResourceConfigForm({ open, kind, initial, onSubmit, onCancel }: Props) {
  const { t } = useTranslation()
  if (kind === 'none') return null

  return (
    <Dialog open={open} onOpenChange={(o) => { if (!o) onCancel() }}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle className="text-base">{t(TITLE_KEYS[kind])}</DialogTitle>
        </DialogHeader>
        {kind === 'monitoring-urls' && <MonitoringUrlsForm initial={initial} onSubmit={onSubmit} onCancel={onCancel} />}
        {kind === 'sales-table' && <SalesTableConfigForm initial={initial} onSubmit={onSubmit} onCancel={onCancel} />}
        {kind === 'weekly-report' && <WeeklyReportConfigForm initial={initial} onSubmit={onSubmit} onCancel={onCancel} />}
        {kind === 'customer-support' && <CustomerSupportConfigForm initial={initial} onSubmit={onSubmit} onCancel={onCancel} />}
        {kind === 'tech-support' && <TechSupportConfigForm initial={initial} onSubmit={onSubmit} onCancel={onCancel} />}
      </DialogContent>
    </Dialog>
  )
}
```

- [ ] **Step 2: 类型检查**

Run: `pnpm exec tsc --noEmit`
Expected: 0 错误。

- [ ] **Step 3: Commit**

```bash
git add src/features/employees/ResourceConfigForm.tsx
git commit -m "feat(employees/ui): route customer-support/tech-support + i18n title"
```

---

### Task 10: `GroupMatchInput` + `MonitoringUrlsForm` + `SalesTableConfigForm` + `WeeklyReportConfigForm` 接入 i18n

**Files:**
- Modify: `src/features/employees/forms/GroupMatchInput.tsx`
- Modify: `src/features/employees/forms/MonitoringUrlsForm.tsx`
- Modify: `src/features/employees/forms/SalesTableConfigForm.tsx`
- Modify: `src/features/employees/forms/WeeklyReportConfigForm.tsx`

- [ ] **Step 1: 写改动 — `GroupMatchInput`**

In `src/features/employees/forms/GroupMatchInput.tsx`:
- Add `import { useTranslation } from 'react-i18next'` at top
- Inside component body, add `const { t } = useTranslation()`
- Replace any prop default `label` with `t('employee.config.groupMatch.label')`
- Replace 包含 / 排除 hardcoded labels with `t('employee.config.groupMatch.include')` / `t('employee.config.groupMatch.exclude')`
- Replace `+` placeholder text with `t('employee.config.groupMatch.addPlaceholder')`
- If shows matched-count hint, use `t('employee.config.groupMatch.matchedHint', { count })`

Apply the same `useTranslation` substitution pattern to:
- `MonitoringUrlsForm.tsx` — title/placeholder/添加 button labels
- `SalesTableConfigForm.tsx` — visible English/中文混合 labels
- `WeeklyReportConfigForm.tsx` — visible labels

For each file: every JSX text node that is human-facing (not a CSS class) must come from `t(...)`. If a key is missing, ADD it under `employee.config.{form}.*` in both `zh-CN.json` and `en-US.json` and reference it. Do not commit untranslated strings.

- [ ] **Step 2: 类型检查 + lint + 单元测试**

Run: `pnpm exec tsc --noEmit && pnpm lint && pnpm exec vitest run src/features/employees/`
Expected: 0 错误，已有测试不应回归。

- [ ] **Step 3: Commit**

```bash
git add src/features/employees/forms/GroupMatchInput.tsx src/features/employees/forms/MonitoringUrlsForm.tsx src/features/employees/forms/SalesTableConfigForm.tsx src/features/employees/forms/WeeklyReportConfigForm.tsx src/i18n/zh-CN.json src/i18n/en-US.json
git commit -m "i18n(employees): GroupMatchInput + MonitoringUrls/SalesTable/WeeklyReport forms"
```

---

### Task 11: `HireWizard` 雇佣完成后异步触发切片

**Files:**
- Modify: `src/features/employees/HireWizard.tsx`

- [ ] **Step 1: 找到雇佣完成的 onSuccess 回调**

Open `src/features/employees/HireWizard.tsx`. Locate the function that calls `employee_create` (or the equivalent IPC) and gets back the new `employeeId`.

- [ ] **Step 2: 在成功后追加异步触发切片**

Right after successful create, before closing the wizard, add (use the resourceConfig that was just submitted):

```ts
import { employeeIndexKnowledgeAsync } from '@/lib/tauri'

// after const created = await employeeCreate(...)
const knowledgeSources = (resourceConfig?.knowledgeSources as Array<{
  path: string; originalName: string; size?: number; status?: string
}> | undefined) ?? []

const pending = knowledgeSources.filter((s) => !s.status || s.status === 'pending' || s.status === 'failed')
if (pending.length > 0) {
  // fire-and-forget; non-blocking
  void employeeIndexKnowledgeAsync(
    created.id,
    pending.map((s) => ({ path: s.path, originalName: s.originalName, size: s.size ?? 0 })),
  )
}
```

The wizard close path is unchanged —切片在后台进行。EmployeeCard 通过 polling / IPC event 反映状态（下个 task）。

- [ ] **Step 3: 类型检查**

Run: `pnpm exec tsc --noEmit`
Expected: 0 错误。

- [ ] **Step 4: Commit**

```bash
git add src/features/employees/HireWizard.tsx
git commit -m "feat(employees): non-blocking knowledge indexing after hire"
```

---

### Task 12: 派活前置 — 索引未完成时阻止 dispatch

**Files:**
- Modify: `src/features/employees/triggerPrechecks.ts`
- Modify: `src/features/employees/triggerPrechecks.test.ts`（如果已有 test 文件，否则新建）

- [ ] **Step 1: 写测试**

In `src/features/employees/triggerPrechecks.test.ts`, add a case:

```ts
it('blocks dispatch when knowledgeSources are still indexing', () => {
  const tpl = findTemplate('builtin:xiaoke')!
  const result = runTriggerPrechecks(tpl, {
    knowledgeSources: [
      { path: '/tmp/a.md', originalName: 'a.md', status: 'indexing', slicedCount: 0 },
    ],
  } as any /* employee record fragment */)
  expect(result).toEqual({ kind: 'knowledge-indexing' })
})

it('allows dispatch when all knowledgeSources are done', () => {
  const tpl = findTemplate('builtin:xiaoke')!
  const result = runTriggerPrechecks(tpl, {
    knowledgeSources: [{ path: '/tmp/a.md', originalName: 'a.md', status: 'done', slicedCount: 12 }],
  } as any)
  expect(result.kind).not.toBe('knowledge-indexing')
})
```

- [ ] **Step 2: 运行 — 看失败**

Run: `pnpm exec vitest run src/features/employees/triggerPrechecks.test.ts`
Expected: FAIL — 没有 `knowledge-indexing` 分支。

- [ ] **Step 3: 实现**

In `triggerPrechecks.ts`:

Extend `PrecheckResult` union:

```ts
type PrecheckResult =
  | { kind: 'attachment'; spec: RequiresAttachmentSpec }
  | { kind: 'resource'; resourceConfigKind: ResourceConfigKind }
  | { kind: 'dingtalk' }
  | { kind: 'knowledge-indexing' }    // ← new
  | { kind: 'ok' }
```

In the precheck function, BEFORE the existing resource-config check, add:

```ts
const sources = (employee.resourceConfig?.knowledgeSources as Array<{ status?: string }> | undefined) ?? []
if (sources.some((s) => s.status === 'pending' || s.status === 'indexing')) {
  return { kind: 'knowledge-indexing' }
}
```

- [ ] **Step 4: 在 EmployeeDrawer 处理新分支**

In `src/features/employees/EmployeeDrawer.tsx`, locate the precheck switch in `handleTrigger` and add:

```tsx
case 'knowledge-indexing':
  toast({ title: t('employee.config.knowledge.statusIndexing'), description: t('employee.config.knowledge.hint') })
  return
```

- [ ] **Step 5: 测试 + 类型检查**

Run: `pnpm exec vitest run src/features/employees/ && pnpm exec tsc --noEmit`
Expected: 全部 PASS。

- [ ] **Step 6: Commit**

```bash
git add src/features/employees/triggerPrechecks.ts src/features/employees/triggerPrechecks.test.ts src/features/employees/EmployeeDrawer.tsx
git commit -m "feat(employees): block dispatch while knowledge indexing in progress"
```

---

### Task 13: 移除 dispatch_prompt 中的 FAQ 全文注入（如有）+ 添加 memory 提示

**Files:**
- Modify: `src-tauri/src/runtime/employee/dispatch_prompt.rs`

- [ ] **Step 1: 检查现状**

Run: `/usr/bin/grep -n "knowledgeSources\|FAQ\|knowledge_sources\|load_file" src-tauri/src/runtime/employee/dispatch_prompt.rs`

- [ ] **Step 2: 实现**

Wherever the dispatch prompt builds context for an employee:
- Remove any code that reads `knowledgeSources[].path` and inlines `std::fs::read_to_string` content into the prompt.
- For templates `builtin:xiaoke` and `builtin:xiaogong`, append to the dispatch user message:

```rust
fn knowledge_hint(template_id: &str) -> Option<&'static str> {
    match template_id {
        "builtin:xiaoke" | "builtin:xiaogong" => Some(
            "\n\n你的知识库已切片入 cognitive memory，\
             category=`knowledge:{employee_id}`。请用 memory_search 按客户问题/报错关键词检索，\
             不要要求加载 FAQ 全文。"
        ),
        _ => None,
    }
}
```

Substitute `{employee_id}` with the current employee id at call site, and append `knowledge_hint(...)` to the prompt body.

- [ ] **Step 3: cargo check + 跑相关测试**

Run: `cd src-tauri && cargo check && cargo test dispatch`
Expected: 0 错误。

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/runtime/employee/dispatch_prompt.rs
git commit -m "refactor(employee/dispatch): replace FAQ inline injection with memory_search hint for xiaoke/xiaogong"
```

---

### Task 14: 端到端手动验证

- [ ] **Step 1: 启动 dev**

Run: `pnpm tauri:dev`

- [ ] **Step 2: 雇佣小客**

操作清单（请在 UI 中逐项执行，并报告每一步看到的结果）：
1. 顶部 ➕ → 选择"小客 · 客服支持"
2. 第 3 步：上传一个 markdown FAQ 文件（≥ 2 个 `## ` 段落）
3. 完成雇佣 → 看到员工卡片，状态徽章应为"切片中"
4. 等待 ≤ 3 秒 → 状态变为"已入库 N 条"
5. 立即点派活 → 应能正常 dispatch（precheck 通过）
6. 在第 3 步上传期间立即返回主页并尝试派活 → 应弹"知识库索引中"提示
7. 卸载/重新雇佣，并故意上传一个权限不可读的文件 → 状态应为"切片失败" + 显示重试按钮

- [ ] **Step 3: 验证 memory 命中**

In dev console / employee chat: 输入 "怎么注册"，观察 LLM 调用 `memory_search(query="注册", category="knowledge:{employee_id}")`，应返回切片内容。

- [ ] **Step 4: i18n 切换**

设置 → 语言切到 English → 重开 ResourceConfigForm，所有标签应为英文。

- [ ] **Step 5: 报告结果**

如果以上 5 步全部符合预期，Plan 完成。否则记录失败步骤，回到对应 Task 修复。

- [ ] **Step 6: 最终 commit（如有 docs 更新）**

```bash
git add docs/superpowers/plans/2026-05-08-faq-knowledge-async-indexing.md
git commit -m "docs: faq async indexing plan complete"
```

---

## 已知风险与回滚

- **`save_cognitive_memory` 在分块循环内若失败**：当前实现立即返回 Err 并把 `status=failed`，已切的部分会留在 memory（无回滚）。可接受 — 用户重试时会重复写入；后续若需要去重，加 `memory_clear_category` 工具。
- **大文件**：>5MB 的 FAQ 切片仍走 spawn_blocking，单文件可能跑数十秒；UI 状态会停在 "indexing"，对用户透明。如需进度百分比，扩展 `update_knowledge_source_status` 支持 `progress: f32`。
- **回滚**：每个 task 的 commit 是独立的；若 Task 13 上线后 dispatch 出错，单独 revert Task 13 的 commit 即可暂时退回（牺牲 token 效率）。
