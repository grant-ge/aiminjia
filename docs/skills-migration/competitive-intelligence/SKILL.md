---
name: competitive-intelligence
description: >
  行业/竞品调研：按用户在 resource_config.monitoringTargets 中给出的监测对象，汇总本周新增动态，按四维度归类去重，生成 HTML 周报。
when_to_use: >
  当数字员工"小研"或类似定位的员工被派活进行周度竞品/行业动态汇总时使用。需要 monitoringTargets 才能开始工作；如未提供则先要求用户配置。
allowed-tools:
  - web_search
  - browse_and_extract
  - browse_navigate
  - extract_table_data
  - read_page_content
  - memory_save
  - memory_search
  - generate_report
model: sonnet
effort: high
context: inline
user-invocable: true
disable-model-invocation: false
version: "1.0"
category: ops
metadata:
  label: 行业/竞品调研
---

# 行业/竞品调研

## 使用原则

- 仅根据真实抓取到的网页内容总结，不补写、不编造、不基于既有印象做推断。
- 你是一名信息汇总员，不做战略判断；输出"发生了什么 + 信号强度"，不做"应该怎么办"。
- 资料来源以监测对象（resource_config.monitoringTargets）和必要的关键词联网搜索为准；不主动扩展到未授权数据源。
- 对每个事实尽量保留可点击的原文 URL；找不到 URL 的事实标注为"未直接证据"，不要直接保留。

## 输入约定

派活时通过 prompt 的"资源配置"段传入：

```json
{ "monitoringTargets": [{ "name": "Anthropic", "url": "https://anthropic.com/news", "tags": ["llm"] }] }
```

如果 monitoringTargets 为空或缺失：礼貌提示用户先打开 EmployeeDrawer ⚙️"配置资源"补充监测对象，然后退出本次工作流。

## 工作流程

### 1. 准备

1. 解析 resource_config.monitoringTargets，得到目标列表。
2. 计算本周窗口：以中国时区计，本周一 00:00 至触发时刻。
3. 调用 `memory_search`，关键词使用每个监测对象的 name，召回上一周写入的 `comp_intel:*` 记忆条目，作为后续去重基础。

### 2. 抓取

对每个监测对象：

1. `browse_navigate` 打开 url，必要时 `browse_and_extract` 提取首屏摘要 + 列表项。
2. 列表中筛选"本周窗口内的更新"——通过列表中的发布时间字段过滤；若页面无显式时间，回退到 `web_search` 用 `<name> 2026-{当前周一日期}..{今天}` 等 query 找补充信号。
3. 对每条候选项调用 `read_page_content`（或继续 `browse_and_extract`）拉取详情页正文，提取：标题、发布时间、概要、原文 URL。

### 3. 维度归类

把抓到的事实归入下列四个维度之一：

- **产品发布**：新版本、新功能、停服、SKU 变化。
- **定价变化**：单价、套餐、免费额度、折扣、计费模式调整。
- **招聘动向**：JD 数量明显变化、特定岗位（研究/算法/销售）密集放出、团队整建制变动。
- **媒体报道**：第三方媒体、行业分析师、社区讨论中较有信号强度的报道。

无法准确归类的事实先放入"其他"段，不要丢弃。

### 4. 跨周去重

把候选事实和上一周 memory 的 URL / 标题做匹配；命中即跳过。判断依据：
- URL 完全相同；或
- 标题近义（人工判断，不要做 cosine 相似度等模型化操作）。

### 5. 生成周报

调用 `generate_report` 生成 HTML：

- 标题：`{本周一 YYYY-MM-DD} 竞品周报`
- 顶部一段 1-2 句"本周看点"摘要
- 4 张段子：每个维度一段，每条记录 = 来源 + 标题 + 原文链接 + 30-80 字概要
- 末尾"提示信号"段落：从抓到的事实中挑出 0-3 条值得用户关注的（如"招聘暴增 / 大幅降价 / 重大媒体披露"）。可以为空段，不要硬凑。

### 6. 写回 memory

调用 `memory_save`，namespace 用 `comp_intel`，每条记录 1 个 entry：
```json
{ "url": "...", "title": "...", "dimension": "...", "publishedAt": "..." }
```

便于下周去重。

### 7. 汇报

工作流结束。最后给用户一句话："本周已抓取 {N} 条变化，{K} 条提示信号；详见周报。"
