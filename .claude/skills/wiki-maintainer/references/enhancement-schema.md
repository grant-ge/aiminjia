# Enhancement Schema

enhancement 文件放在：

```text
.understand-anything/enhancements/<module>.json
```

每个模块一个 JSON 对象，路径使用 repo-relative path。

## 必填结构

```json
{
  "module": "runtime-permission-chain",
  "source_basis": "code-tests-only",
  "key_nodes": [
    {
      "filePath": "src-tauri/src/runtime/query_engine.rs",
      "summary": "中文摘要，说明该文件在链路中的职责。",
      "tags": ["runtime", "tools", "permission"],
      "complexity": "complex"
    }
  ],
  "semantic_edges": [
    {
      "sourceFilePath": "src-tauri/src/runtime/query_engine.rs",
      "targetFilePath": "src-tauri/src/runtime/tools/dispatcher.rs",
      "type": "calls",
      "reason": "QueryEngine 把工具调用交给 dispatcher 执行并进入权限流水线。",
      "weight": 0.9
    }
  ],
  "architecture_findings": [
    {
      "title": "权限决策集中在 dispatcher/permission 层",
      "description": "中文说明，必须基于当前代码或测试。",
      "evidence": [
        "src-tauri/src/runtime/tools/dispatcher.rs",
        "src-tauri/src/runtime/tools/permission.rs"
      ]
    }
  ],
  "tour_steps": [
    {
      "title": "从 QueryEngine 进入工具权限流水线",
      "description": "中文导览说明。",
      "filePaths": [
        "src-tauri/src/runtime/query_engine.rs",
        "src-tauri/src/runtime/tools/dispatcher.rs",
        "src-tauri/src/runtime/tools/permission.rs"
      ]
    }
  ]
}
```

## 必填字段

| 字段 | 规则 |
|---|---|
| `module` | 稳定 kebab-case 模块 id。 |
| `key_nodes` | 非空。每项必须有存在的 `filePath`、中文 `summary`、`tags[]`、`complexity`。 |
| `semantic_edges` | 非空。每项必须有存在的 source/target path、edge `type`、中文 `reason`。 |
| `architecture_findings` | 非空。每项必须有当前来源 `evidence`；产品架构声明必须有代码/测试证据。 |
| `tour_steps` | 非空。每项必须指向存在的 `filePaths`。 |

## 推荐边类型

优先使用 Understand-Anything core edge types：

- `imports`、`exports`、`contains`、`calls`、`depends_on`、`configures`
- `reads_from`、`writes_to`、`transforms`、`validates`
- `tested_by`、`publishes`、`subscribes`、`routes`
- `documents`、`related`

`scripts/apply-understand-enhancements.mjs` 会规范化部分 alias，但 enhancement 文件最好直接写 canonical type。

## 可选未来字段

需要更严格审计或函数 trace 时，可以扩展这些字段：

```json
{
  "risks_or_gaps": [
    {
      "title": "风险标题",
      "description": "风险说明",
      "evidence": ["src-tauri/src/runtime/tools/dispatcher.rs"]
    }
  ],
  "test_coverage_edges": [
    {
      "testFilePath": "src-tauri/tests/tool_permission_pipeline_test.rs",
      "targetFilePath": "src-tauri/src/runtime/tools/permission.rs",
      "type": "validates",
      "reason": "该测试验证权限流水线的 ask/allow/deny 分支。"
    }
  ],
  "function_traces": [
    {
      "name": "Runtime tool permission ask flow",
      "steps": [
        {
          "filePath": "src-tauri/src/runtime/query_engine.rs",
          "symbol": "run_tool"
        }
      ],
      "evidence": ["src-tauri/tests/tool_permission_pipeline_test.rs"]
    }
  ]
}
```

这些字段只有在 `scripts/apply-understand-enhancements.mjs` 明确支持后，才算正式 schema。
