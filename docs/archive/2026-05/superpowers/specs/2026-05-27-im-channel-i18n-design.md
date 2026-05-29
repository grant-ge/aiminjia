# IM 频道页面多语言（i18n）

- 状态：Draft
- 日期：2026-05-27
- 范围：`src/features/channel/` 全量接入 i18n + 补 en-US 翻译

## 1. 背景

`src/features/channel/` 下 9 个 tsx 文件（不含 `.test.tsx`）目前都是硬编码中文。
`i18n/zh-CN.json` 的 `channel.*` 子树只覆盖 ChannelPage 顶部 hero、状态徽章、
`actions.*`、钉钉 remove dialog 一小块（共 38 个 key），其它 platform 配置弹窗、
ChannelConfigDetails、WhatsappRiskBanner、TelegramChannelConfig 等都没接 i18n。

其他 feature（home、inbox、employees、chat 等）已经全量双语，channel 是仓库
里仍在裸中文的最大未完成区域之一。

## 2. 目标 / 非目标

### 目标

1. `src/features/channel/` 下 9 个非测试文件全部使用 `useTranslation()`，移除
   所有硬编码中文。
2. `i18n/zh-CN.json` 的 `channel.*` 子树重排：按 `common / platforms / <platform>
   / details / status / actions / remove / dialog / errors` 分组。
3. `i18n/en-US.json` 镜像同 shape，提供完整英文文案。
4. 平台显示名（钉钉 / 飞书 / 企业微信 / 个人微信 / Telegram / WhatsApp）跟随
   语言切换：中文环境 "钉钉"，英文环境 "DingTalk"。
5. 4 个 `.test.tsx` 跟随更新断言；vitest 默认 `lng=zh-CN`，中文断言保留。

### 非目标

- 不引入 ICU MessageFormat / pluralrules（项目目前未使用）
- 不做日期 / 数字本地化（已有 zh-CN locale 即可）
- 不改后端任何字符串（连接错误等仍是后端 raw message，前端只在用户取消
  / 网络异常等明确分支落 i18n 错误文案；后端原始 error 字符串不翻译）
- 不在这次同时给其它 feature 补 i18n
- 不引入第三方翻译服务流程

## 3. 设计

### 3.1 i18n key 结构

```
channel.title                                      "IM 频道" / "IM Channels"
channel.heroTitle / heroDesc / heroPrivacy
channel.status.{unconfigured,configuredOffline,connected,connecting,reconnecting,configError,disabled,offline}
channel.actions.{configure,remove,moreDingtalkConfig,configureWith,enabledAria,disabledAria,enable,disable,viewDetails,retry,done,cancel,close,confirm,refresh,copy,copied}
channel.platforms.dingtalk.{name,brand,description,topbarTitle,inactive,icon}
channel.platforms.feishu.{name,brand,description,topbarTitle,comingSoon}
channel.platforms.wecom.{name,brand,description,topbarTitle}
channel.platforms.wechat.{name,brand,description,topbarTitle}
channel.platforms.telegram.{name,brand,description,topbarTitle}
channel.platforms.whatsapp.{name,brand,description,topbarTitle}
channel.dingtalk.config.{title,preparingQr,scanHint,retryQr,success,successHint,switchConfig,doneButton,errorBeginFailed,errorNoConfig,errorPollFailed}
channel.feishu.config.{title,...}
channel.wecom.config.{title,fields.*,steps.*,validation.*,errors.*}
channel.wechat.config.{...}
channel.telegram.config.{title,fields.*,steps.*,validation.*,errors.*}
channel.whatsapp.config.{title,steps.*,errors.*,states.*}
channel.whatsapp.risk.{dialogTitle,intro,risksHeading,risks.*,suggestionsHeading,suggestions.*,acknowledge,continue,cancel}
channel.details.{title,readonly,fields.{appKey,appSecret,robotCode,source,createdAt,updatedAt,iLinkBotId},secretConfirm.{title,description,confirm,cancel,reveal,reading,revealed},titleByPlatform.{dingtalk,feishu,wecom,wechat}}
channel.remove.{title,description,confirm,cancel,titleByPlatform.{dingtalk,feishu,wecom,wechat,telegram,whatsapp}}
channel.dialog.{title,description}
channel.errors.{openFileTitle,openFileMessage,...}
```

最终 key 数量预计 +200~300（zh-CN 与 en-US 同步）。

### 3.2 平台显示名取数路径

旧：

```ts
const PLATFORM_DISPLAY_NAME: Record<PlatformKey, string> = {
  dingtalk: '钉钉', feishu: '飞书', ...
}
```

新：

```ts
function getPlatformName(t: TFunction, p: PlatformKey) {
  return t(`channel.platforms.${p}.name`)
}
```

`PLATFORM_LOGO_SRC` 是路径常量，保留；`PLATFORM_DISPLAY_NAME` 删除。

### 3.3 富文本片段（WhatsappRiskBanner）

`WhatsappRiskBanner.tsx` 含 `<b>` 和 `<a>` 内嵌的中文段落。**不引入 `<Trans>`**。
做法：
- 把整段长文本拆成多个 i18n key（`risk.intro1`, `risk.intro2`, `risk.intro3`），
  `<b>`/`<a>` 留在 JSX，文本碎片走 `t()`。
- 列表项每条一个 key（`risk.risks.unauthorized`, `risk.risks.banRisk`,
  `risk.risks.virtualNumber`, `risk.risks.broadcast`）。
- 强调标签的位置在中英文中可能错位——可接受，因为这只是把短语加粗，错位不
  会改变语义。

### 3.4 测试策略

- vitest setup（`src/test/setup.ts` 或现有 i18n 初始化处）默认 `lng=zh-CN`。
  现有中文断言（如 `expect(...).toHaveTextContent('未配置')`）保持，因为文案
  没变。
- 改的是 i18n key 路径而非文案：旧文案如果在新结构里位置变了但字面量相同，
  测试不动。
- 个别地方文案微调（例如把"已显示" 拆成跟 `revealed` 状态匹配的新文案）需要
  同步改测试断言。
- 不新增 en-US smoke 测试（YAGNI；CI 时间 / 维护成本 > 价值）。

### 3.5 文件改动清单

| 文件 | 动作 |
|---|---|
| `src/i18n/zh-CN.json` | 重排 `channel.*`，补全约 +250 key |
| `src/i18n/en-US.json` | 镜像 shape，写英文 |
| `src/features/channel/ChannelPage.tsx` | 接 `useTranslation`；删 `PLATFORM_DISPLAY_NAME`；约 70 处替换 |
| `src/features/channel/ChannelConfig.tsx` | 接 i18n；约 12 处 |
| `src/features/channel/ChannelConfigDetails.tsx` | `copyForPlatform()` 改成读 i18n；约 27 处 |
| `src/features/channel/FeishuChannelConfig.tsx` | 约 22 处 |
| `src/features/channel/WecomChannelConfig.tsx` | 约 70 处 |
| `src/features/channel/WechatChannelConfig.tsx` | 约 20 处 |
| `src/features/channel/TelegramChannelConfig.tsx` | 约 38 处 |
| `src/features/channel/WhatsappChannelConfig.tsx` | 约 80 处 |
| `src/features/channel/WhatsappRiskBanner.tsx` | 约 25 处 |
| `src/features/channel/*.test.tsx` × 4 | 仅当文案改动了才更新断言 |

### 3.6 实施顺序

1. 重写 `zh-CN.json` 的 `channel.*` 子树（保留现有 38 个 key 的内容、按新结构归位 + 新增 key）
2. 镜像 `en-US.json` 同结构，写完整英文
3. 替换：`ChannelPage` → `ChannelConfig` → `ChannelConfigDetails` → 各 platform config →`WhatsappRiskBanner`
4. 跑 `pnpm test`，必要时同步改测试断言
5. `pnpm exec tsc --noEmit` + `pnpm lint`
6. dev server 里手动切 zh-CN / en-US 各看一遍 ChannelPage 与每个平台配置弹窗

## 4. 验收

- `grep -nE '[一-龥]' src/features/channel/*.tsx`（排除测试文件和注释）应当
  仅余少量注释或 logo unicode 字符；JSX/字符串字面量里的中文清零
- `pnpm test` 全绿
- `pnpm exec tsc --noEmit` 无错
- 切到 en-US 后 ChannelPage、6 个平台配置弹窗、ConfigDetails、Remove 确认、
  WhatsApp Risk Banner 全部渲染英文，无 missing key 警告

## 5. 风险与缓解

| 风险 | 缓解 |
|---|---|
| WhatsApp / Wecom 文案密集，存在漏译 | 完成后跑一次 `pnpm exec tsc --noEmit && pnpm lint`，再手动 dev server 切英文逐弹窗肉眼过 |
| 平台名跟语言切后，topbar / Dialog title 拼接出现奇怪空格 | 用 `t('channel.actions.configureWith', {name: ...})` 这类 placeholder 形式，避免 `${name}配置` 这种紧贴拼接 |
| 测试断言锁死了原中文，重排后断言失败 | 文案不变只动 key 时测试不会失败；若 PlatformCopy 重构改了字面量，按红 / 修测试 / 再绿的顺序处理 |
| en-US 文案专业度问题 | 参考 OpenClaw / Telegram / WhatsApp 官方 UI 措辞，必要时请求 review |

## 6. 未做 / 后续

- 其它 feature（如还有 hardcoding 的）不在本次范围
- 不本地化日期 / 数字（暂仍以 ISO 字符串展示）
- 不接 ICU plural / gender forms
