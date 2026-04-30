# rules.md — masking_level 传递链路测试意图

## 意图 1：relaxed 设置下 PII 不被脱敏

**前提**
- executor 返回 `masking_level = "relaxed"`

**操作**
- 发一条包含身份证号（如 110108199001011234）的消息，触发一次对话

**断言**
- LLM 收到的 `LlmStepInput.masking_level` 是 `"relaxed"`
- 发给 LLM 的内容里身份证号没有被替换成占位符

---

## 意图 2：strict 设置下 PII 全部被脱敏

**前提**
- executor 返回 `masking_level = "strict"`

**操作**
- 发一条包含身份证号、手机号、邮箱的消息

**断言**
- LLM 收到的内容里身份证号被替换为 `[ID_CARD_1]`
- 手机号被替换为 `[PHONE_1]`
- 邮箱被替换为 `[EMAIL_1]`

---

## 意图 3：空值或未知值回退 strict

**前提**
- DB 里 `data_masking_level` 为空字符串或未知值（如 `"off"`）

**操作**
- 触发一次对话

**断言**
- 实际生效的 masking_level 是 `"strict"`
- PII 被正常脱敏，不因空值而跳过脱敏

---

## 意图 4：masking_level 在多轮工具调用中保持一致

**前提**
- executor 返回 `masking_level = "relaxed"`

**操作**
- MockExecutor 预设 3 次 ToolCalls + 1 次 ContentComplete（多轮循环）

**断言**
- 每一次 `run_llm_step` 收到的 `input.masking_level` 都是 `"relaxed"`
- settings 只从 executor 读取一次，后续每轮复用同一份快照
