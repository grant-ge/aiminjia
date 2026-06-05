# LongMemEval 上下文压缩评测

评测 AIjia 自动上下文压缩对「长期记忆」的损伤。基于 LongMemEval（ICLR 2025）。

> 设计文档：`docs/superpowers/specs/2026-06-04-context-compaction-eval-design.md`
> 评测代码：`src-tauri/tests/longmemeval_eval.rs`

## 它测什么

给一段带时间戳的多 session 对话历史，问一个只有读过历史才能答对的问题。对每条样本跑两组：

- **A 组（天花板）**：全量历史直接答题
- **B 组（压缩）**：走真实 `prepare_messages_for_llm` 压缩后再答题

两组正确率之差 = **压缩损伤**。重点看 `knowledge-update`（知识更新）和 `temporal-reasoning`（时序）两类。

判分（裁判）逻辑移植自官方 `evaluate_qa.py`，统一走 lotus 网关，**不需要 Python / OpenAI key**。

## 目录

```
tools/longmemeval/
├── download.ps1     # 下载数据集到 data/（已 gitignore）
├── sample_5.json    # 6 类各 1 条小样本（已提交，供快速冒烟，~178KB）
├── data/            # 完整数据集（gitignore，不进版本控制）
└── README.md
```

## 准备

### 方式一：用提交的小样本（快速冒烟，免下载）

```powershell
$env:LME_DATA = "tools/longmemeval/sample_5.json"
```

> 注意：小样本来自 oracle（每条仅约 3 个 session），强制触发压缩可验证链路，但
> 干扰量小，不代表真实大海捞针场景。

### 方式二：下载完整数据集（真实评测）

```powershell
pwsh tools/longmemeval/download.ps1          # 下 oracle + s(264MB)
pwsh tools/longmemeval/download.ps1 -IncludeM  # 额外下 m(超大)
```

数据集区别：

| 文件 | 每条 session 数 | 用途 |
|---|---|---|
| `longmemeval_oracle.json` | ~3（仅证据） | 冒烟，无压缩意义 |
| `longmemeval_s_cleaned.json` | ~40（含干扰，~115k token） | **A 期主集** |
| `longmemeval_m_cleaned.json` | ~500 | B 期，自然溢出触发 |

## 运行

前置：**先在 AIjia app 内登录一次**，确保 `~/.renlijia` 下 JWT 新鲜（评测真实走 lotus 网关计费）。

```powershell
# 冒烟（前 3 条）
$env:LME_DATA = "tools/longmemeval/data/longmemeval_s_cleaned.json"
$env:LME_LIMIT = "3"
cargo test --test longmemeval_eval -- --ignored --nocapture

# 均衡采样（每类 5 条，共约 30 条）
$env:LME_PER_TYPE = "5"
cargo test --test longmemeval_eval -- --ignored --nocapture
```

> 在 `src-tauri/` 目录下运行（cargo test 的工作目录为该包目录，故 `LME_DATA` 用相对
> 仓库根的路径时记得加 `../`，或直接用绝对路径）。

### 环境变量

| 变量 | 含义 | 默认 |
|---|---|---|
| `LME_DATA` | 数据集 json 路径 | 见 `default_data_path()` |
| `LME_LIMIT` | 取前 N 条 | 3 |
| `LME_PER_TYPE` | 每个 question_type 取 N 条（设置后覆盖 LME_LIMIT） | 无 |
| `LME_OUT` | 逐条明细输出目录 | 数据集同目录 |

## 产出

- 控制台：A / B 两组的总正确率 + 6 类分项 + 平均压缩比
- `lme_eval_details.jsonl`：逐条明细（问题 / 标准答案 / 两组回答 / 判定 / 摘要正文），
  用于人工核验裁判判定是否准确

## 成本提示

每条样本约 3–4 次网关调用（摘要 + A 答 + B 答 + 判分 ×2），且 s 数据每条 ~115k token，
约 100 秒/条。500 条全量约 14 小时 + 相应费用。建议先跑均衡 30 条。
