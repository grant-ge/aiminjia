# DingTalk OPEN_CLAW Registration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a one-click DingTalk `OPEN_CLAW`扫码开通入口 that starts a registration session, opens the DingTalk creation page, polls for credentials, and saves them into the existing IM channel config.

**Architecture:** Keep provisioning inside the existing Rust channel subsystem. Add a focused registration client for `/app/registration/init|begin|poll`, expose Tauri commands through `commands::channel`, and extend the existing React `ChannelConfig` with a扫码开通 flow while preserving manual AppKey/AppSecret/RobotCode fallback.

**Tech Stack:** Rust/Tauri commands, reqwest, serde, tokio tests; React 19, Zustand store, Tauri invoke wrappers, Vitest component tests.

---

## File Structure

- Create `src-tauri/src/connector/channel/dingtalk_registration.rs`: pure registration types, URL/source constants, request/response parsing helpers, and async network client.
- Modify `src-tauri/src/connector/channel/mod.rs`: export the new registration module.
- Modify `src-tauri/src/connector/channel/manager.rs`: add `begin_dingtalk_registration`, `poll_dingtalk_registration`, and make `poll_dingtalk_registration` save credentials directly on success.
- Modify `src-tauri/src/connector/channel/types.rs`: add serializable registration begin/poll result types.
- Modify `src-tauri/src/commands/channel.rs`: add Tauri IPC commands for begin and poll.
- Modify `src-tauri/src/lib.rs`: register new Tauri commands.
- Modify `src/lib/tauri.ts`: add frontend invoke wrappers and types.
- Modify `src/features/channel/ChannelConfig.tsx`: add one-click OPEN_CLAW card, polling flow, and keep manual config.
- Add/update tests next to touched files.

## Task 1: Rust Registration Client and Manager Commands

**Files:**
- Create: `src-tauri/src/connector/channel/dingtalk_registration.rs`
- Modify: `src-tauri/src/connector/channel/types.rs`
- Modify: `src-tauri/src/connector/channel/mod.rs`
- Modify: `src-tauri/src/connector/channel/manager.rs`
- Modify: `src-tauri/src/commands/channel.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Write failing unit tests for URL/source and poll normalization**

Add tests in `dingtalk_registration.rs` before implementation:

```rust
#[test]
fn builds_open_claw_verification_url_with_source() {
    let url = build_open_claw_verification_url("ABCD-EFGH-IJKL", "OPEN_CLAW");
    assert_eq!(
        url,
        "https://open-dev.dingtalk.com/openapp/registration/openClaw?user_code=ABCD-EFGH-IJKL&source=OPEN_CLAW"
    );
}

#[test]
fn normalizes_poll_status_values() {
    assert_eq!(normalize_poll_status("SUCCESS"), RegistrationPollState::Success);
    assert_eq!(normalize_poll_status("waiting"), RegistrationPollState::Waiting);
    assert_eq!(normalize_poll_status("EXPIRED"), RegistrationPollState::Expired);
    assert_eq!(normalize_poll_status("whatever"), RegistrationPollState::Unknown);
}
```

- [ ] **Step 2: Run tests and verify RED**

Run: `cd src-tauri && cargo test connector::channel::dingtalk_registration --lib`

Expected: FAIL because `dingtalk_registration` module/functions/types do not exist yet.

- [ ] **Step 3: Implement minimal registration module**

Implement constants `DEFAULT_REGISTRATION_BASE_URL=https://oapi.dingtalk.com`, `OPEN_CLAW_SOURCE=OPEN_CLAW`, serializable raw response types, `RegistrationPollState`, `normalize_poll_status`, `build_open_claw_verification_url`, `begin_registration`, and `poll_registration`.

- [ ] **Step 4: Wire manager and commands**

Add manager methods and Tauri commands:

```rust
channel_begin_dingtalk_registration() -> RegistrationBeginResult
channel_poll_dingtalk_registration(device_code: String) -> RegistrationPollResult
```

`poll_dingtalk_registration` should keep `client_secret` inside Rust, encrypt and save it immediately on success, then connect DingTalk Stream. The frontend must not receive or send AppSecret. `robot_code` defaults to `app_key` when DingTalk omits it.

- [ ] **Step 5: Run Rust verification**

Run: `cd src-tauri && cargo test connector::channel::dingtalk_registration --lib`

Expected: PASS.

Run: `cd src-tauri && cargo check`

Expected: PASS.

## Task 2: Frontend IPC and ChannelConfig UI

**Files:**
- Modify: `src/lib/tauri.ts`
- Modify: `src/features/channel/ChannelConfig.tsx`
- Add/Modify: `src/features/channel/ChannelConfig.test.tsx`

- [ ] **Step 1: Write failing UI tests**

Add tests that mock `@/lib/tauri` and assert:

```tsx
it('starts OPEN_CLAW registration when one-click button is clicked', async () => {
  // render <ChannelConfig />
  // click text /钉钉扫码一键开通/
  // expect channelBeginDingtalkRegistration called once
  // expect window.open called with returned verificationUriComplete
})

it('saves credentials when polling returns success', async () => {
  // mock begin result with deviceCode and URL
  // mock first poll success with clientId/robotCode only
  // click start
  // expect onSaved called after backend has saved credentials
})
```

- [ ] **Step 2: Run tests and verify RED**

Run: `pnpm vitest run src/features/channel/ChannelConfig.test.tsx`

Expected: FAIL because wrappers/UI do not exist.

- [ ] **Step 3: Add Tauri wrappers**

Add TypeScript interfaces and functions:

```ts
export interface ChannelRegistrationBeginResult { deviceCode: string; userCode: string; verificationUriComplete: string; intervalSeconds: number; expiresInSeconds: number; source: string }
export interface ChannelRegistrationPollResult { state: 'waiting' | 'success' | 'fail' | 'expired' | 'unknown'; clientId?: string; robotCode?: string; failReason?: string }
export function channelBeginDingtalkRegistration(): Promise<ChannelRegistrationBeginResult>
export function channelPollDingtalkRegistration(deviceCode: string): Promise<ChannelRegistrationPollResult>
```

- [ ] **Step 4: Implement ChannelConfig one-click UI**

Add a primary card/button above manual fields: “钉钉扫码一键开通”. On click, call begin, open `verificationUriComplete`, then poll until success/fail/expired with returned interval. On success, the backend has already encrypted/saved credentials; call `onSaved` without exposing AppSecret to React.

- [ ] **Step 5: Run frontend verification**

Run: `pnpm vitest run src/features/channel/ChannelConfig.test.tsx`

Expected: PASS.

Run: `pnpm build`

Expected: PASS.

## Task 3: Final Integration Verification

- [ ] **Step 1: Run targeted Rust checks**

Run: `cd src-tauri && cargo test connector::channel::dingtalk_registration --lib`
Expected: PASS.

Run: `cd src-tauri && cargo check`
Expected: PASS.

- [ ] **Step 2: Run targeted frontend checks**

Run: `pnpm vitest run src/features/channel/ChannelConfig.test.tsx`
Expected: PASS.

- [ ] **Step 3: Manual smoke path**

Use dev app UI, click “钉钉扫码一键开通”, confirm browser opens an URL like:

```text
https://open-dev.dingtalk.com/openapp/registration/openClaw?user_code=...&source=OPEN_CLAW
```

Do not claim successful credential provisioning unless poll returns `success` and the backend save/connect path completes.

---

## Self-Review

- Spec coverage: includes OPEN_CLAW source, begin URL, poll, secure config save, manual fallback, tests.
- Placeholder scan: no TBD/TODO placeholders remain; test skeletons describe exact assertions.
- Type consistency: Rust/TS expose only non-sensitive registration state externally; `clientSecret` remains inside Rust and maps to the existing encrypted AppSecret save path.
