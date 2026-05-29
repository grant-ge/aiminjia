# Expert Team Visuals And Chat Width Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让专家团入口页、开启 team 后的过程 UI、team 对话抽屉统一使用专家团 logo/专家头像，并让 team 对话区域吃设置中的“全宽/居中”聊天宽度配置，默认全宽。

**Architecture:** 专家团业务定义继续以 `src/features/expert-teams/teams.ts` 为源头，不额外新增一层业务配置；只新增轻量视觉 helper/context，把静态 SVG 头像和 team logo 映射注入到现有 team UI。聊天宽度继续复用 Settings store 的 `chatWidthMode`，在主聊天、专家团欢迎页、输入区和 team 抽屉中用同一套容器规则。

**Tech Stack:** React + TypeScript + Vitest + Tauri Rust settings model + Tailwind utility classes。

---

## File Structure

- Modify: `src/features/expert-teams/teams.ts`
  - 保持专家团/专家定义的唯一业务源头。
  - 给 `ExpertPersona` 增加可选 `agentName?: string`，用于把运行时 agent key（如 `brand-lead`）映射回专家头像，但不改变页面显示名。
- Create: `src/features/expert-teams/teamLogo.tsx`
  - 集中维护 team logo 的 icon/color 映射。
  - 暴露 `getExpertTeamLogo(teamId)` 给入口卡片、欢迎页、banner 共用。
- Modify: `src/features/expert-teams/expertAvatar.ts`
  - 保持静态头像 URL lookup。
  - 新增 `getExpertAvatarUrlForAgent(team, agentName)`，优先按 `agentName` 匹配，再按专家名匹配。
- Create: `src/components/team/TeamVisualContext.tsx`
  - 提供当前 active `ExpertTeam` 给嵌套的 runtime team UI。
  - 避免把 `ExpertTeam` props 层层穿透到所有小组件。
- Modify: `src/components/team/AgentAvatar.tsx`
  - 从 `TeamVisualContext` 获取当前团队。
  - 能命中专家头像时渲染 `<img>`，命不中时保留原 initials fallback。
- Modify: `src/features/expert-teams/ExpertTeamCard.tsx`
  - 改为使用 shared team logo helper。
- Modify: `src/components/chat-scene/ExpertTeamBanner.tsx`
  - 用 shared team logo 和专家头像 stack 替代 emoji/initials。
- Modify: `src/components/chat-scene/ExpertTeamWelcome.tsx`
  - 欢迎页使用 shared team logo。
  - 专家列表使用头像 chip。
  - 容器读取 `chatWidthMode`，支持默认全宽和居中。
- Modify: `src/features/chat/ChatPage.tsx`
  - 把 `expertTeamId` 传给主消息区。
  - 用 `TeamVisualProvider` 包住 team 抽屉，确保抽屉中的 agent 头像也能命中专家 SVG。
- Modify: `src/components/layout/ChatArea.tsx`
  - 接收 `expertTeamId`，传给 `MessageList`。
  - `chatWidthMode` 缺省值改为 `full`。
- Modify: `src/components/chat/MessageList.tsx`
  - 接收 `expertTeamId`。
  - 对 inline `TeamProgressBlock` 包 `TeamVisualProvider`。
- Modify: `src/components/chat-scene/ChatBottomArea.tsx`
  - 输入区宽度 shell 读取 `chatWidthMode`，默认全宽。
- Modify: `src/components/team/TeamChatDrawer.tsx`
  - 抽屉内事件/消息内容宽度读取 `chatWidthMode`。
- Modify: `src/components/team/TeammateDetailPanel.tsx`
  - 详情面板内容宽度读取 `chatWidthMode`。
- Modify: `src/types/settings.ts`
  - `DEFAULT_SETTINGS.chatWidthMode` 改成 `'full'`。
- Modify: `src-tauri/src/models/settings.rs`
  - 后端 settings 默认值改成 `full`，保持前后端一致。
- Test: `src/features/expert-teams/expertVisuals.test.ts`
  - 验证运行态 agent name 能映射到专家 SVG。
- Test: `src/components/chat/MessageList.layout.test.tsx`
  - 验证 inline team progress block 能显示专家 SVG。
- Test: `src/components/chat-scene/ExpertTeamWelcome.test.tsx`
  - 验证欢迎页 logo/头像和宽度模式。
- Test: `src/components/chat-scene/__tests__/ChatBottomArea.width.test.tsx`
  - 验证输入区默认全宽、设置居中后居中。
- Test: `src/stores/settingsStore.test.ts`
  - 更新默认设置预期为 `full`。

## Task 1: Shared Team Logo Helper

**Files:**
- Create: `src/features/expert-teams/teamLogo.tsx`
- Modify: `src/features/expert-teams/ExpertTeamCard.tsx`
- Test: `src/features/expert-teams/__tests__/ExpertTeamCard.test.tsx`

- [ ] **Step 1: Write/adjust test for card logo rendering**

Run existing card tests first so baseline is known:

```bash
pnpm vitest run src/features/expert-teams/__tests__/ExpertTeamCard.test.tsx
```

Expected: existing tests pass before refactor, or fail only where assertions depend on old emoji/logo markup.

- [ ] **Step 2: Extract logo mapping**

Create `src/features/expert-teams/teamLogo.tsx` with a small explicit map keyed by team id, returning icon component and style metadata. Keep this visual-only; do not move team business data out of `teams.ts`.

- [ ] **Step 3: Update card to use helper**

Change `src/features/expert-teams/ExpertTeamCard.tsx` to call `getExpertTeamLogo(team.id)`. Do not rename existing team labels.

- [ ] **Step 4: Verify card tests**

```bash
pnpm vitest run src/features/expert-teams/__tests__/ExpertTeamCard.test.tsx
```

Expected: PASS.

## Task 2: Runtime Agent Name To Expert Avatar Mapping

**Files:**
- Modify: `src/features/expert-teams/teams.ts`
- Modify: `src/features/expert-teams/expertAvatar.ts`
- Test: `src/features/expert-teams/expertVisuals.test.ts`

- [ ] **Step 1: Add failing avatar lookup test**

Create `src/features/expert-teams/expertVisuals.test.ts` with assertions for marketing runtime names:

```ts
import { describe, expect, it } from 'vitest';
import { EXPERT_TEAMS } from './teams';
import { getExpertAvatarUrlForAgent } from './expertAvatar';

describe('expert team visuals', () => {
  it('maps marketing runtime agent names to expert avatars', () => {
    const team = EXPERT_TEAMS.find((item) => item.id === 'marketing');
    expect(team).toBeTruthy();
    expect(getExpertAvatarUrlForAgent(team!, 'brand-lead')).toContain('/expert-avatars/marketing/');
    expect(getExpertAvatarUrlForAgent(team!, 'content-lead')).toContain('/expert-avatars/marketing/');
    expect(getExpertAvatarUrlForAgent(team!, 'growth-hacker')).toContain('/expert-avatars/marketing/');
    expect(getExpertAvatarUrlForAgent(team!, 'channel-manager')).toContain('/expert-avatars/marketing/');
  });
});
```

- [ ] **Step 2: Run failing test**

```bash
pnpm vitest run src/features/expert-teams/expertVisuals.test.ts
```

Expected: FAIL because `agentName` / `getExpertAvatarUrlForAgent` does not exist yet.

- [ ] **Step 3: Add minimal mapping**

In `src/features/expert-teams/teams.ts`, add optional `agentName?: string` to `ExpertPersona` and set only the observed marketing mappings:

```ts
{ name: '品牌负责人', agentName: 'brand-lead', ... }
{ name: '内容主理人', agentName: 'content-lead', ... }
{ name: '增长黑客', agentName: 'growth-hacker', ... }
{ name: '渠道经理', agentName: 'channel-manager', ... }
```

Do not change visible names.

- [ ] **Step 4: Add lookup helper**

In `src/features/expert-teams/expertAvatar.ts`, add `getExpertAvatarUrlForAgent(team, agentName)` that:

1. returns `undefined` if no team or no agent name;
2. finds `team.personas.find((persona) => persona.agentName === agentName || persona.name === agentName)`;
3. returns the existing avatar URL for that persona name;
4. falls back to `undefined`.

- [ ] **Step 5: Verify avatar lookup test**

```bash
pnpm vitest run src/features/expert-teams/expertVisuals.test.ts
```

Expected: PASS.

## Task 3: Visual Context For Team Runtime UI

**Files:**
- Create: `src/components/team/TeamVisualContext.tsx`
- Modify: `src/components/team/AgentAvatar.tsx`
- Modify: `src/components/chat/MessageList.tsx`
- Modify: `src/features/chat/ChatPage.tsx`
- Test: `src/components/chat/MessageList.layout.test.tsx`

- [ ] **Step 1: Add failing MessageList test**

Extend `src/components/chat/MessageList.layout.test.tsx` with a case that renders `MessageList` with `expertTeamId="marketing"` and a team marker/overview containing `brand-lead`, then expects an image with `/expert-avatars/marketing/` in `src`.

- [ ] **Step 2: Run failing test**

```bash
pnpm vitest run src/components/chat/MessageList.layout.test.tsx
```

Expected: FAIL because runtime team visuals are not provided to `AgentAvatar`.

- [ ] **Step 3: Add context**

Create `src/components/team/TeamVisualContext.tsx` exporting:

```ts
export const TeamVisualProvider = ...;
export function useTeamVisualContext() { ... }
```

The value is `ExpertTeam | null`.

- [ ] **Step 4: Update AgentAvatar**

In `src/components/team/AgentAvatar.tsx`, call `useTeamVisualContext()` and `getExpertAvatarUrlForAgent(team, name)`. If URL exists, render an `<img>` inside the same sizing shell. Preserve fallback initials/color logic when no URL exists.

- [ ] **Step 5: Provide context in message and drawer paths**

In `src/components/chat/MessageList.tsx`, accept `expertTeamId?: string`, resolve it from `EXPERT_TEAMS`, and wrap `TeamProgressBlock` with `TeamVisualProvider`.

In `src/features/chat/ChatPage.tsx`, pass `expertTeamId` into `ChatArea` and wrap `TeamChatDrawer` with `TeamVisualProvider value={expertTeam ?? null}`.

- [ ] **Step 6: Verify runtime visual tests**

```bash
pnpm vitest run src/components/chat/MessageList.layout.test.tsx src/features/expert-teams/expertVisuals.test.ts
```

Expected: PASS.

## Task 4: Expert Team Welcome And Banner Visuals

**Files:**
- Modify: `src/components/chat-scene/ExpertTeamBanner.tsx`
- Modify: `src/components/chat-scene/ExpertTeamWelcome.tsx`
- Test: `src/components/chat-scene/ExpertTeamWelcome.test.tsx`

- [ ] **Step 1: Add welcome visual test**

Create/extend `src/components/chat-scene/ExpertTeamWelcome.test.tsx` to render the marketing team welcome page and assert:

```ts
expect(screen.getByText('专家团')).toBeTruthy();
expect(container.querySelector('img[src*="/expert-avatars/marketing/"]')).toBeTruthy();
```

Use the actual accessible text already present in the component instead of inventing new copy.

- [ ] **Step 2: Run failing test**

```bash
pnpm vitest run src/components/chat-scene/ExpertTeamWelcome.test.tsx
```

Expected: FAIL until welcome uses shared avatars/logo.

- [ ] **Step 3: Update welcome UI**

In `src/components/chat-scene/ExpertTeamWelcome.tsx`, replace old emoji/initials visuals with `getExpertTeamLogo(team.id)` and existing `getExpertAvatarUrl(team.id, persona.name)` calls. Keep team/persona names unchanged.

- [ ] **Step 4: Update banner UI**

In `src/components/chat-scene/ExpertTeamBanner.tsx`, use `getExpertTeamLogo(team.id)` for the logo and show a small stack/list of expert avatar images when available; fallback remains initials.

- [ ] **Step 5: Verify welcome/banner tests**

```bash
pnpm vitest run src/components/chat-scene/ExpertTeamWelcome.test.tsx
```

Expected: PASS.

## Task 5: Chat Width Setting Applies To Team UI And Defaults To Full

**Files:**
- Modify: `src/types/settings.ts`
- Modify: `src-tauri/src/models/settings.rs`
- Modify: `src/components/layout/ChatArea.tsx`
- Modify: `src/components/chat-scene/ChatBottomArea.tsx`
- Modify: `src/components/chat-scene/ExpertTeamWelcome.tsx`
- Modify: `src/components/team/TeamChatDrawer.tsx`
- Modify: `src/components/team/TeammateDetailPanel.tsx`
- Test: `src/stores/settingsStore.test.ts`
- Test: `src/components/chat-scene/__tests__/ChatBottomArea.width.test.tsx`
- Test: `src/components/chat-scene/ExpertTeamWelcome.test.tsx`

- [ ] **Step 1: Update frontend default test first**

In `src/stores/settingsStore.test.ts`, change the default expectation:

```ts
expect(result.current.settings.chatWidthMode).toBe('full');
```

- [ ] **Step 2: Run failing settings test**

```bash
pnpm vitest run src/stores/settingsStore.test.ts
```

Expected: FAIL if frontend default is still `centered`.

- [ ] **Step 3: Change frontend default**

In `src/types/settings.ts`, set:

```ts
chatWidthMode: 'full',
```

- [ ] **Step 4: Change backend default**

In `src-tauri/src/models/settings.rs`, set `default_chat_width_mode()` to return `"full"` and update the corresponding Rust unit test name/expectation.

- [ ] **Step 5: Add width shell tests**

Create/extend `src/components/chat-scene/__tests__/ChatBottomArea.width.test.tsx` to assert:

1. composer shell is full-width by default;
2. after settings store is set to `centered`, shell uses centered max-width classes/attributes.

Extend `src/components/chat-scene/ExpertTeamWelcome.test.tsx` similarly for welcome content.

- [ ] **Step 6: Apply width classes consistently**

Use the same mode calculation in all relevant UI:

```ts
const chatWidthMode = settings.chatWidthMode ?? 'full';
const widthClass = chatWidthMode === 'centered' ? 'mx-auto w-full max-w-...' : 'w-full';
```

Apply to:

- `ChatArea` message content shell;
- `ChatBottomArea` composer shell;
- `ExpertTeamWelcome` content shell;
- `TeamChatDrawer` event/message content shell;
- `TeammateDetailPanel` detail shell.

- [ ] **Step 7: Verify width tests**

```bash
pnpm vitest run src/stores/settingsStore.test.ts src/components/chat-scene/__tests__/ChatBottomArea.width.test.tsx src/components/chat-scene/ExpertTeamWelcome.test.tsx
```

Expected: PASS.

## Task 6: Full Verification

**Files:**
- No implementation files; verification only.

- [ ] **Step 1: Typecheck frontend**

```bash
pnpm exec tsc --noEmit
```

Expected: exits 0.

- [ ] **Step 2: Run targeted frontend tests**

```bash
pnpm vitest run src/stores/settingsStore.test.ts src/components/settings/__tests__/GeneralPanel.test.tsx src/components/layout/ChatArea.test.tsx src/components/chat-scene/__tests__/ChatBottomArea.width.test.tsx src/components/chat-scene/ExpertTeamWelcome.test.tsx src/components/chat/MessageList.layout.test.tsx src/features/expert-teams/expertVisuals.test.ts src/features/expert-teams/__tests__/ExpertTeamCard.test.tsx
```

Expected: all listed test files pass. Existing stderr from mocked Tauri `invoke/listen` may appear, but the Vitest summary must be green.

- [ ] **Step 3: Check Rust backend**

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: exits 0. Existing warnings are acceptable if not introduced by this change.

- [ ] **Step 4: Run targeted Rust settings test**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib models::settings
```

Expected: settings tests pass.

- [ ] **Step 5: Manual browser sanity check if dev server is running**

Open the app at the active dev server port, then verify:

1. 专家团入口页显示正式 team logo；
2. 进入 marketing team 后，欢迎页展示专家头像；
3. 开启 team 后，过程 UI 和 drawer 中的 `brand-lead` 等 runtime agent 仍显示原名，但头像变成对应 SVG；
4. 设置里切换“全宽/居中”后，主聊天、team 欢迎页、team drawer、输入区宽度一起变化；
5. 新用户/无字段 settings 默认全宽。

Expected: UI matches the listed behavior.

## Self-Review

- Spec coverage:
  - 专家团 logo：Task 1、Task 4。
  - 专家 icon/avatar：Task 2、Task 3、Task 4。
  - 入口页与开启 team 后都带 logo/icon：Task 3、Task 4。
  - team 对话吃全宽/居中设置：Task 5。
  - 默认全宽：Task 5。
  - 名字保持原状：Task 2、Task 3 明确不改 visible names。
- Placeholder scan: no `TBD` / `TODO` / “similar to previous task” placeholders.
- Type consistency:
  - `agentName?: string` is introduced in `ExpertPersona` before `getExpertAvatarUrlForAgent()` consumes it.
  - `expertTeamId?: string` flows `ChatPage -> ChatArea -> MessageList`.
  - `TeamVisualProvider` value is consistently `ExpertTeam | null`.
