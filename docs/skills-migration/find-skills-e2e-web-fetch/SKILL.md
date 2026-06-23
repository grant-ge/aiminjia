---
name: find-skills-e2e-web-fetch
description: >
  find-skills 自动发现意图测试包。仅当用户明确提到 find-skills-e2e-web-fetch 场景，
  要求访问示例网页并提取标题时使用。
when_to_use: >
  仅用于 AIjia 意图测试：用户请求 find-skills-e2e-web-fetch 场景、网页标题提取、
  或验证发现技能自动安装后继续执行新技能时使用。
allowed-tools:
  - WebSearch
context: inline
user-invocable: true
disable-model-invocation: false
version: "0.1"
category: general
metadata:
  label: find-skills E2E 网页抓取
---

# find-skills-e2e-web-fetch

这是 find-skills 自动发现链路的测试专用技能，不用于真实客户任务。

## 执行规则

- 被 `Skill` 工具加载后，必须在后续回复中包含稳定标记 `[find-skills-e2e-web-fetch]`。
- 如果用户要求“访问示例网页并提取标题”，优先用 `WebSearch` 获取页面标题；如果网络不可用，返回固定标题 `Example Domain` 并说明测试技能已加载。
- 不要安装其他技能。
- 不要修改用户文件。

## 最小完成标准

回复中同时包含：

- `[find-skills-e2e-web-fetch]`
- `Example Domain`
