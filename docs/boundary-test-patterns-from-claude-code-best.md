# claude-code-best 边界测试模式总结

> 调研来源：7 个并行 agent（4×Sonnet + 3×Haiku）对 claude-code-best 全量测试文件交叉扫描。
> 总计覆盖 **~215 个边界测试案例**，去重后整理为以下 6 大类。
> 产出目的：为 lotus-app 编写边界测试提供可直接借鉴的模式库。

---

## 一、数值边界（Numeric Boundary）

**核心思路**：测试「恰好等于」、「恰好超过」、「零值」、「最大/最小有意义值」四个点。

### 1.1 恰好等于上限 vs 刚超过上限（最常见模式）

```ts
// 文件：src/utils/__tests__/claudemd.test.ts
test("correctly identifies threshold boundary", () => {
  const atThreshold = "x".repeat(MAX_MEMORY_CHARACTER_COUNT);       // 恰好等于
  const overThreshold = "x".repeat(MAX_MEMORY_CHARACTER_COUNT + 1); // 刚超过

  const result = getLargeMemoryFiles([
    mockMemoryFile({ content: atThreshold }),   // 不应触发
    mockMemoryFile({ content: overThreshold }), // 应触发
  ]);
  expect(result).toHaveLength(1); // 只有超过的那个
});
```

**lotus-app 对标**：`max_agent_turns` 的边界——恰好 1000 次不截断，1001 次触发截断。

### 1.2 极大值被截断到上限

```ts
// 文件：src/utils/shell/__tests__/outputLimits.test.ts
test("caps at upper limit", () => {
  process.env.BASH_MAX_OUTPUT_LENGTH = "999999"; // 极大值
  expect(getMaxOutputLength()).toBe(150_000);     // 必须截断到上限
});
```

### 1.3 负数被视为无效，回退默认值

```ts
// 文件：src/utils/shell/__tests__/outputLimits.test.ts
test("returns default for negative value", () => {
  process.env.BASH_MAX_OUTPUT_LENGTH = "-1"; // 无效输入
  expect(getMaxOutputLength()).toBe(30_000);  // 回退默认
});
```

### 1.4 零值边界

```ts
// 文件：src/utils/__tests__/tokens.test.ts
test("handles zero values", () => {
  const usage = { input_tokens: 0, output_tokens: 0,
    cache_creation_input_tokens: 0, cache_read_input_tokens: 0 };
  expect(getTokenCountFromUsage(usage)).toBe(0); // 零值求和不出错
});
```

### 1.5 进程退出码的语义边界（最小有意义值 vs 刚超过）

```ts
// 文件：src/tools/PowerShellTool/__tests__/commandSemantics.test.ts
test("robocopy exit 7 (success with mismatches) is not error", () => {
  // 7 = 最大成功边界值
  expect(interpretCommandResult("robocopy src dest", 7, "", "").isError).toBe(false);
});
test("robocopy exit 8 (copy errors) is error", () => {
  // 8 = 刚超过成功范围
  expect(interpretCommandResult("robocopy src dest", 8, "", "error").isError).toBe(true);
});
```

---

## 二、字符串/长度边界（String Boundary）

**核心思路**：空字符串、宽度=0、宽度=1、恰好等于限制、多字节字符（CJK）边界。

### 2.1 宽度为 0 和 1 的极端截断

```ts
// 文件：src/utils/__tests__/truncate.test.ts
test("returns ellipsis for maxWidth 1", () => {
  expect(truncateToWidth("hello", 1)).toBe("…"); // 只有省略号
});
test("returns empty for maxWidth 0", () => {
  expect(truncateToWidthNoEllipsis("hello", 0)).toBe(""); // 零宽返回空
});
```

### 2.2 CJK 多字节字符的宽度边界

```ts
// 文件：src/utils/__tests__/truncate.test.ts
test("truncates CJK string at width boundary (2 per char)", () => {
  // CJK 每字符占 2 宽，"你好世界"=8宽，maxWidth=4 只能放1个汉字+省略号
  expect(truncateToWidth("你好世界", 4)).toBe("你…");
});
test("passes through mixed ASCII + CJK at exact limit", () => {
  // "hello你好" = 5 + 2×2 = 9 宽，恰好等于 maxWidth=9，不截断
  expect(truncateToWidth("hello你好", 9)).toBe("hello你好");
});
```

### 2.3 超出容量后停止增长（accumulator 模式）

```ts
// 文件：src/utils/__tests__/stringUtils.test.ts
test("stops accepting data once truncated and full", () => {
  const acc = new EndTruncatingAccumulator(5);
  acc.append("12345"); // 填满
  acc.append("67890"); // 继续追加
  expect(acc.length).toBe(5); // 长度不再增长
  acc.append("more");
  expect(acc.length).toBe(5);
});
```

### 2.4 危险 Unicode 字符（全部为不可见字符）

```ts
// 文件：src/utils/__tests__/sanitization.test.ts
test("handles string with only dangerous characters", () => {
  // 零宽字符 + BOM，清洗后应几乎为空
  const result = partiallySanitizeUnicode("\u200B\u200C\u200D\uFEFF");
  expect(result.length).toBeLessThanOrEqual(1);
});
```

### 2.5 URL 长度上限

```ts
// 文件：src/tools/WebFetchTool/__tests__/urlValidation.test.ts
test("rejects very long URL", () => {
  const longUrl = "https://example.com/" + "a".repeat(MAX_URL_LENGTH); // 超过 2000
  expect(validateURL(longUrl)).toBe(false);
});
```

---

## 三、空值/缺失边界（Null / Empty Boundary）

**核心思路**：每个接收外部输入的函数都测 `null`、`undefined`、`""`、`[]`、`{}`。

### 3.1 null/undefined 输入安全返回（不抛异常）

```ts
// 文件：src/utils/__tests__/json.test.ts
test("returns null for null input", () => {
  expect(safeParseJSON(null as any)).toBeNull();
});
test("returns null for undefined input", () => {
  expect(safeParseJSON(undefined as any)).toBeNull();
});

// 文件：src/utils/__tests__/uuid.test.ts
test("returns null for non-string", () => {
  expect(validateUuid(null)).toBeNull();
  expect(validateUuid(undefined)).toBeNull();
});
```

### 3.2 空字符串边界

```ts
// 文件：src/tools/WebFetchTool/__tests__/urlValidation.test.ts
test("rejects empty string", () => {
  expect(validateURL("")).toBe(false);
});

// 文件：src/tools/FileEditTool/__tests__/utils.test.ts
test("handles empty string", () => {
  expect(normalizeQuotes("")).toBe("");
  expect(stripTrailingWhitespace("")).toBe("");
});

// 文件：src/utils/permissions/__tests__/permissionRuleParser.test.ts
test("handles empty string", () => {
  expect(escapeRuleContent("")).toBe("");
});
```

### 3.3 空数组边界

```ts
// 文件：src/utils/__tests__/tokens.test.ts
test("returns 0 for empty messages", () => {
  expect(tokenCountFromLastAPIResponse([])).toBe(0);
});
test("returns null for empty messages", () => {
  expect(getCurrentUsage([])).toBeNull();
});

// 文件：src/tools/AgentTool/__tests__/agentToolUtils.test.ts
test("returns 0 for empty array", () => {
  expect(countToolUses([])).toBe(0);
});
```

### 3.4 空对象/空缓冲区边界

```ts
// 文件：src/utils/__tests__/CircularBuffer.test.ts
test("getRecent on empty buffer returns empty array", () => {
  const buf = new CircularBuffer<number>(5);
  expect(buf.getRecent(3)).toEqual([]); // 从未 add 过
});

// 文件：tests/integration/context-build.test.ts
test("getLargeMemoryFiles returns empty for empty input", () => {
  expect(getLargeMemoryFiles([])).toEqual([]);
});
```

### 3.5 空输入时不触发操作

```ts
// 文件：src/utils/__tests__/bufferedWriter.test.ts
test("flush with empty buffer does not call writeFn", () => {
  const written: string[] = [];
  const writer = createBufferedWriter({ writeFn: (c) => written.push(c) });
  writer.flush();
  expect(written).toEqual([]); // writeFn 从未被调用
});
```

---

## 四、格式错误边界（Malformed Input Boundary）

**核心思路**：非法 JSON、非法 UUID、错误格式、缺少必填字段、非法字符集。

### 4.1 非法 JSON 格式

```ts
// 文件：src/utils/__tests__/json.test.ts
test("returns null for invalid JSON", () => {
  expect(safeParseJSON("{bad}")).toBeNull(); // 不抛，安全返回 null
});
test("returns empty array for empty string", () => {
  expect(parseJSONL("")).toEqual([]); // JSONL 空行 → 空数组
});
```

### 4.2 UUID 格式错误的多个维度

```ts
// 文件：src/utils/__tests__/uuid.test.ts
test("returns null for invalid UUID format", () => {
  expect(validateUuid("not-a-uuid")).toBeNull();          // 完全不像
  expect(validateUuid("550e8400-e29b-41d4-a716")).toBeNull(); // 段数不足
  expect(validateUuid("550e8400e29b41d4a716446655440000")).toBeNull(); // 无连字符
});
test("returns null for UUID with invalid chars", () => {
  expect(validateUuid("550e8400-e29b-41d4-a716-44665544000g")).toBeNull(); // 'g' 非十六进制
});
test("returns null for UUID with leading/trailing whitespace", () => {
  expect(validateUuid(" 550e8400-e29b-41d4-a716-446655440000")).toBeNull();
  expect(validateUuid("550e8400-e29b-41d4-a716-446655440000 ")).toBeNull();
});

// 文件：src/utils/__tests__/taggedId.test.ts
test("throws on invalid UUID (too short)", () => {
  expect(() => toTaggedId("user", "abcdef")).toThrow("Invalid UUID hex length");
});
test("throws on invalid UUID (too long)", () => {
  expect(() => toTaggedId("user", "550e8400e29b41d4a716446655440000ff"))
    .toThrow("Invalid UUID hex length");
});
```

### 4.3 NaN 和非数字字符串

```ts
// 文件：src/utils/__tests__/envValidation.test.ts
test("treats NaN-producing strings as invalid", () => {
  const result = validateBoundedIntEnvVar("TEST_VAR", "NaN", 100, 1000);
  expect(result.effective).toBe(100);   // 回退默认值
  expect(result.status).toBe("invalid");
});
test("returns default for zero", () => {
  const result = validateBoundedIntEnvVar("TEST_VAR", "0", 100, 1000);
  expect(result.status).toBe("invalid"); // 0 不是正整数
});
```

### 4.4 缺少必填字段 / 缺少 frontmatter

```ts
// 文件：src/utils/__tests__/frontmatterParser.test.ts
test("returns empty frontmatter when none exists", () => {
  const result = parseFrontmatter("Just content, no frontmatter");
  expect(result.frontmatter).toEqual({}); // 优雅降级，不崩溃
});
test("returns undefined for zero", () => {
  expect(parsePositiveIntFromFrontmatter(0)).toBeUndefined(); // 0 不是正整数
});
test("returns undefined for negative number", () => {
  expect(parsePositiveIntFromFrontmatter(-1)).toBeUndefined();
});
```

### 4.5 安全相关的格式边界

```ts
// 文件：src/tools/WebFetchTool/__tests__/urlValidation.test.ts
test("rejects URL with username", () => {
  expect(validateURL("https://user@example.com/path")).toBe(false); // 含凭据
});
test("protocol change is not permitted", () => {
  // https → http 降级重定向
  expect(isPermittedRedirect("https://example.com/path", "http://example.com/path")).toBe(false);
});
test("invalid URL returns false", () => {
  expect(isPermittedRedirect("not-a-url", "also-not-a-url")).toBe(false);
});
```

---

## 五、权限边界（Permission Boundary）

**核心思路**：未知值 → 安全默认、细粒度规则不能越权匹配全局规则、路径白名单严格匹配。

### 5.1 未知权限模式回退安全默认值

```ts
// 文件：src/utils/permissions/__tests__/PermissionMode.test.ts
test("returns 'default' for unknown string", () => {
  expect(permissionModeFromString("unknown")).toBe("default");
  expect(permissionModeFromString("")).toBe("default");      // 空字符串也回退
  expect(permissionModeFromString("PLAN")).toBe("default");  // 大小写敏感
});
test("returns true for undefined", () => {
  expect(isDefaultMode(undefined)).toBe(true); // 未配置视为默认
});
```

### 5.2 细粒度规则不匹配整工具 deny

```ts
// 文件：src/utils/permissions/__tests__/permissions.test.ts
test("rule with content does not match whole-tool deny", () => {
  // "Bash(rm -rf)" 只针对 rm -rf 命令，不阻断整个 Bash 工具
  const ctx = makeContext({ denyRules: ["Bash(rm -rf)"] });
  const result = getDenyRuleForTool(ctx, makeTool("Bash"));
  expect(result).toBeNull(); // 不匹配
});
```

### 5.3 全部被 deny 时返回空数组（不是 null）

```ts
// 文件：src/utils/permissions/__tests__/permissions.test.ts
test("returns empty array when all agents denied", () => {
  const ctx = makeContext({ denyRules: ["Agent(Explore)", "Agent(Research)"] });
  expect(filterDeniedAgents(agents, ctx, "Agent")).toEqual([]); // [] 不是 null
});
```

### 5.4 路径白名单严格匹配（子域名不自动继承）

```ts
// 文件：src/tools/WebFetchTool/__tests__/preapproved.test.ts
test("subdomain of preapproved host does not match", () => {
  // docs.python.org 被预审批，但 sub.docs.python.org 不继承
  expect(isPreapprovedHost("sub.docs.python.org", "/3/")).toBe(false);
});
test("path-scoped entry does not match other paths", () => {
  // github.com/anthropics 被审批，github.com/torvalds 不被审批
  expect(isPreapprovedHost("github.com", "/torvalds/linux")).toBe(false);
});
test("empty hostname returns false", () => {
  expect(isPreapprovedHost("", "/")).toBe(false);
});
```

### 5.5 转义字符作为字面量匹配

```ts
// 文件：src/utils/permissions/__tests__/shellRuleMatching.test.ts
test("handles escaped asterisk as literal", () => {
  // "echo \*" 中的 \* 是字面量星号，不是通配符
  expect(matchWildcardPattern("echo \\*", "echo *")).toBe(true);
  expect(matchWildcardPattern("echo \\*", "echo hello")).toBe(false);
});
```

---

## 六、状态边界（State Boundary）

**核心思路**：初始状态、终态隔离、重复操作防护、恰好不触发 vs 刚好触发。

### 6.1 状态未改变时不触发通知（Object.is 等值边界）

```ts
// 文件：src/state/__tests__/store.test.ts
test("setState does not notify when state unchanged (Object.is)", () => {
  store.setState(prev => prev); // 返回同一引用
  expect(notified).toBe(false); // 不通知
});
test("onChange is not called when state unchanged", () => {
  store.setState(prev => prev);
  expect(called).toBe(false);
});
```

### 6.2 终态隔离（dispatched 状态的 work item 不再可取）

```ts
// 文件：packages/remote-control-server/src/__tests__/store.test.ts
test("skips non-pending items", () => {
  const item = storeCreateWorkItem({ environmentId: "env1", ... });
  storeUpdateWorkItem(item.id, { state: "dispatched" }); // 状态转移
  expect(storeGetPendingWorkItem("env1")).toBeUndefined(); // 不再返回
});
```

### 6.3 重复操作防护（已消费的请求第二次 resolve 返回 false）

```ts
// 文件：src/services/mcp/__tests__/channelPermissions.test.ts
test("duplicate resolve returns false (already consumed)", () => {
  const cb = createChannelPermissionCallbacks();
  cb.onResponse("test-id", () => {});
  expect(cb.resolve("test-id", "allow", "server")).toBe(true);  // 第一次成功
  expect(cb.resolve("test-id", "allow", "server")).toBe(false); // 第二次失败
});
```

### 6.4 过期令牌返回 null（终态边界）

```ts
// 文件：packages/remote-control-server/src/__tests__/auth.test.ts
test("returns null for expired token", () => {
  const token = generateWorkerJwt("ses_old", -10); // TTL=-10，已过期
  expect(verifyWorkerJwt(token)).toBeNull();
});
```

### 6.5 超时阈值的「刚超过」测试

```ts
// 文件：packages/remote-control-server/src/__tests__/disconnect-monitor.test.ts
test("environment times out when lastPollAt is too old", () => {
  // 刚好超过超时时间 1 分钟，应触发断连
  const oldDate = new Date(Date.now() - timeoutMs - 60000);
  storeUpdateEnvironment(env.id, { lastPollAt: oldDate });
  expect(updated?.status).toBe("disconnected");
});
```

### 6.6 循环缓冲区 overflow（capacity 溢出边界）

```ts
// 文件：src/utils/__tests__/CircularBuffer.test.ts
test("capacity=1 keeps only the most recent item", () => {
  const buf = new CircularBuffer<number>(1);
  buf.add(10); buf.add(20); buf.add(30);
  expect(buf.toArray()).toEqual([30]); // 只保留最新
});
test("addAll with overflow", () => {
  const buf = new CircularBuffer<number>(3);
  buf.addAll([1, 2, 3, 4, 5]); // 一次性溢出
  expect(buf.toArray()).toEqual([3, 4, 5]); // 只保留最后 N 个
});
```

---

## 七、lotus-app 可直接套用的边界测试清单

基于以上模式，lotus-app 当前最缺少的边界测试：

### 7.1 `masking_level` 边界

| 边界场景 | 测试要点 |
|---|---|
| `from_str_or_strict("")` | 空字符串回退 `Strict` |
| `from_str_or_strict("STRICT")` | 大小写不敏感 → `Strict` |
| `from_str_or_strict("unknown_value")` | 未知值回退 `Strict` |
| Relaxed 级别时 `mask_text` 输出等于输入 | 零 masking 验证 |
| Strict 级别时 6 类 PII 同时存在 | 全类型 masking 验证 |
| mask → unmask roundtrip | 编解码一致性 |
| 同一 PII 出现两次复用同一 placeholder | 去重验证 |

### 7.2 `max_agent_turns` 边界

| 边界场景 | 测试要点 |
|---|---|
| `AppSettings` 默认值恰好为 1000 | 字段存在且值正确 |
| `from_string_map` 传入 `"0"` | 零值被拒，回退默认 |
| `from_string_map` 传入 `"999999"` | 极大值不截断（1000 < 999999，以设置值为准） |
| `from_string_map` 传入 `"-1"` | 负值被拒，回退默认 |
| `TurnConfig.max_iterations` 等于 `settings.max_agent_turns` | 传递链路正确 |
| 当 iterations=max_iterations-3 时 safeguard 注入提示 | safeguard 触发边界 |
| 当 iterations=max_iterations 时 turn 终止 | 截断边界 |

### 7.3 框架 E2E（MockLlmExecutor 模式）

| 边界场景 | 测试要点 |
|---|---|
| Mock 返回 1 次 ToolCalls + 1 次 ContentComplete | 标准工具调用链路 |
| Mock 无限返回 ToolCalls，max_iterations=3 | 确认第 3 轮截断，事件序列正确 |
| Mock 返回 Cancelled | 事件序列含 cancelled |
| masking_level=Strict 时 executor 收到的 input 已脱敏 | 脱敏传递验证 |

---

## 八、边界测试的编写模板（Rust 版）

```rust
// 数值边界模板
#[test]
fn test_max_turns_at_limit_does_not_truncate() {
    let settings = AppSettings { max_agent_turns: 1000, ..Default::default() };
    assert_eq!(settings.max_agent_turns, 1000);
    // 恰好等于上限不应触发 safeguard
    let action = check_iteration(999, 1000, "some content");
    assert!(matches!(action, SafeguardAction::Continue));
}

#[test]
fn test_max_turns_over_limit_triggers_safeguard() {
    // 刚超过上限
    let action = check_iteration(1000, 1000, "some content");
    assert!(matches!(action, SafeguardAction::InjectPromptAndContinue(_)));
}

// 空值边界模板
#[test]
fn test_masking_level_from_empty_string_falls_back_to_strict() {
    assert_eq!(MaskingLevel::from_str_or_strict(""), MaskingLevel::Strict);
}

// 格式错误边界模板
#[test]
fn test_masking_level_from_unknown_value_falls_back_to_strict() {
    assert_eq!(MaskingLevel::from_str_or_strict("UNKNOWN_LEVEL"), MaskingLevel::Strict);
    assert_eq!(MaskingLevel::from_str_or_strict("  "), MaskingLevel::Strict); // 纯空白
}
```
