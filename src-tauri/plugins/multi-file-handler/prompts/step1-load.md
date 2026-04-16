# Step 1：加载文件 & schema 对齐

## 目标

把每个文件完整加载到 Python 环境，产出一张**schema 对齐表**，让用户确认字段映射后再进 step2 做实际处理。

## 执行步骤

### 1. 从 step0 的 note 读回 intent

从之前的 `step0_intent` note 里读 `mode`、`primary_key`、`files`。

### 2. 逐个 load_file

```python
files = await load_file(file_id_1)  # 会返回 pandas DataFrame 或文本
files2 = await load_file(file_id_2)
```

存在 session 变量里（如 `df_a`, `df_b`）。

### 3. 提取各文件 schema

对 Excel/CSV：
- 字段名列表
- 每个字段的数据类型（数值/字符串/日期）
- 前 3 行数据样本
- 行数

对 Word/PDF：
- 页数 / 段落数
- 前 200 字样本

### 4. 按 mode 做"对齐"：

| mode | 对齐动作 |
|------|---------|
| `compare` | 找公共字段 + 差异字段；确认 primary_key 在两边都存在且唯一 |
| `merge` | 找公共字段；标注两边独有的字段 |
| `batch_translate` | 直接列出每文件的文本内容量 |
| `cross_ref` | 确认 A.primary_key 能在 B 里找到匹配（join 可行性） |
| `summarize_all` | 只需要粗略结构摘要 |

### 5. 产出 schema 对齐表（Markdown）

展示给用户，类似：

```
## 📂 文件加载结果

- **A.xlsx** (1095 行 × 18 列)：工号、姓名、部门、基本工资、...
- **B.xlsx** (1203 行 × 16 列)：工号、姓名、部门、基本工资、...

## 🔗 字段对齐（mode=compare）

| 字段 | A.xlsx | B.xlsx | 对齐状态 |
|------|--------|--------|---------|
| 工号 | ✅ | ✅ | 主键，两边都有 |
| 姓名 | ✅ | ✅ | 一致 |
| 部门 | ✅ | ✅ | 一致 |
| 基本工资 | ✅ | ✅ | 可对比 |
| 绩效工资 | ✅ | ❌ | 仅 A 有（A 独有） |
| 项目津贴 | ❌ | ✅ | 仅 B 有（B 独有） |

## ⚠️ 检测到的问题

- A 中有 12 个工号在 B 中找不到（A 独有员工）
- B 中有 120 个工号在 A 中找不到（B 新增员工 / A 离职员工）
- 两边都有的工号：1083 个
```

### 6. 调 save_analysis_note 保存 schema 对齐结果

```json
{
  "key": "step1_schema",
  "content": "<JSON，包含 common_fields / a_only / b_only / matched_keys 等>",
  "step": 1
}
```

### 7. 询问用户 confirm

"对齐结果看起来合理吗？如果有字段对齐错了，请告诉我。确认后我们开始执行。"

## 注意事项

- 如果 load_file 失败（文件损坏、格式不对），**立即报告用户**而不是盲目继续
- 大文件（>5000 行）只在内存预览前 1000 行做 schema 分析，避免拖慢
- 处理 Excel 带 merged cells 或多 sheet：默认读第一个 sheet，如果用户文件有多 sheet 主动询问
