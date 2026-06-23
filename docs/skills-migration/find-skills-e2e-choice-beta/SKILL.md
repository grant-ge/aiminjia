---
name: find-skills-e2e-choice-beta
description: >
  find-skills 候选歧义意图测试包 Beta。仅当用户明确提到 find-skills-e2e-choice 场景，
  需要在多个可安装市场技能之间做选择时使用。
when_to_use: >
  仅用于 AIjia 意图测试：用户请求 find-skills-e2e-choice 场景，并需要验证发现技能在候选不唯一时先询问用户。
allowed-tools: []
context: inline
user-invocable: true
disable-model-invocation: false
version: "0.1"
category: general
metadata:
  label: find-skills E2E 候选 Beta
---

# find-skills-e2e-choice-beta

这是 find-skills 候选歧义链路的测试专用技能 Beta，不用于真实客户任务。

## 执行规则

- 被 `Skill` 工具加载后，必须在后续回复中包含稳定标记 `[find-skills-e2e-choice-beta]`。
- 不要安装其他技能。
- 不要修改用户文件。

## 最小完成标准

回复中包含 `[find-skills-e2e-choice-beta]`。
