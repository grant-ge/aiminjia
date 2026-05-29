# Data Masking Privacy Toggle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把数据脱敏级别默认值改为 `relaxed`，并在"关于 AI 小家"面板的隐私区域加一个开关，关闭=relaxed，开启=strict。

**Architecture:** 改三处：默认值（前端 `DEFAULT_SETTINGS` + 后端 `AppSettings`）；`settingsStore` 加 `setDataMaskingLevel`；`AboutPanel` 接收并渲染开关，调用 `getSettings / updateSettings` 持久化。`AboutPanel` 本身是纯 props 组件，所以在 `SettingsModal` 注入数据和回调。

**Tech Stack:** React / TypeScript / Zustand / Tauri IPC（`getSettings` / `updateSettings`）/ Rust `models/settings.rs`

---

## Files

| Action | Path | What changes |
|---|---|---|
| Modify | `src/types/settings.ts` | `DEFAULT_SETTINGS.dataMaskingLevel` → `'relaxed'` |
| Modify | `src/stores/settingsStore.ts` | 加 `setDataMaskingLevel` action |
| Modify | `src/stores/settingsStore.test.ts` | 更新默认值断言 |
| Modify | `src/components/settings/panels/AboutPanel.tsx` | 新增隐私开关 section，接收 `dataMaskingLevel` + `onDataMaskingChange` props |
| Modify | `src/components/settings/panels/AboutPanel.test.tsx` | 覆盖新 section 渲染和交互 |
| Modify | `src/components/settings/SettingsModal.tsx` | 向 `AboutPanel` 注入 `dataMaskingLevel` 和回调 |
| Modify | `src-tauri/src/models/settings.rs` | `AppSettings` 默认值 → `"relaxed"` |

---

### Task 1: 更新前端默认值并修测试

**Files:**
- Modify: `src/types/settings.ts:41`
- Modify: `src/stores/settingsStore.test.ts:21`

- [ ] **Step 1: 改 `DEFAULT_SETTINGS.dataMaskingLevel`**

在 `src/types/settings.ts` 第 41 行把：
```ts
  dataMaskingLevel: 'strict',
```
改为：
```ts
  dataMaskingLevel: 'relaxed',
```

- [ ] **Step 2: 更新 store 测试里的默认值断言**

在 `src/stores/settingsStore.test.ts` 第 21 行，把：
```ts
    expect(state.dataMaskingLevel).toBe('strict')
```
改为：
```ts
    expect(state.dataMaskingLevel).toBe('relaxed')
```

- [ ] **Step 3: 运行测试确认通过**

```bash
pnpm exec vitest run src/stores/settingsStore.test.ts
```
Expected: 所有测试 PASS。

- [ ] **Step 4: Commit**

```bash
git add src/types/settings.ts src/stores/settingsStore.test.ts
git commit -m "feat(privacy): default dataMaskingLevel to relaxed"
```

---

### Task 2: 更新后端默认值

**Files:**
- Modify: `src-tauri/src/models/settings.rs:90`

- [ ] **Step 1: 改 Rust 默认值**

在 `src-tauri/src/models/settings.rs` 第 90 行，把：
```rust
            data_masking_level: "strict".to_string(),
```
改为：
```rust
            data_masking_level: "relaxed".to_string(),
```

- [ ] **Step 2: 编译确认**

```bash
cd src-tauri && cargo check --lib
```
Expected: `Finished` 无 error。

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/models/settings.rs
git commit -m "feat(privacy): default data_masking_level to relaxed in AppSettings"
```

---

### Task 3: settingsStore 加 setDataMaskingLevel

**Files:**
- Modify: `src/stores/settingsStore.ts`

- [ ] **Step 1: 给 `SettingsState` interface 加方法声明**

在 `src/stores/settingsStore.ts` 第 29 行（`markLoaded` 之前）插入：
```ts
  setDataMaskingLevel: (level: DataMaskingLevel) => void
```
同时确认 import 里有 `DataMaskingLevel`：
```ts
import type { Settings, LlmProvider, FontScale, DataMaskingLevel } from '@/types/settings'
```

- [ ] **Step 2: 加实现**

在 `src/stores/settingsStore.ts` 的 `create` 回调中，`markLoaded` 之前加：
```ts
  setDataMaskingLevel: (dataMaskingLevel) => set({ dataMaskingLevel }),
```

- [ ] **Step 3: 写失败测试**

在 `src/stores/settingsStore.test.ts` 末尾加：
```ts
it('setDataMaskingLevel updates dataMaskingLevel in store', () => {
  useSettingsStore.getState().setDataMaskingLevel('strict')
  expect(useSettingsStore.getState().dataMaskingLevel).toBe('strict')
  useSettingsStore.getState().setDataMaskingLevel('relaxed')
  expect(useSettingsStore.getState().dataMaskingLevel).toBe('relaxed')
})
```

- [ ] **Step 4: 运行测试确认先失败后通过**

```bash
pnpm exec vitest run src/stores/settingsStore.test.ts
```
加入实现后：Expected: 所有测试 PASS。

- [ ] **Step 5: Commit**

```bash
git add src/stores/settingsStore.ts src/stores/settingsStore.test.ts
git commit -m "feat(privacy): add setDataMaskingLevel to settingsStore"
```

---

### Task 4: AboutPanel 加隐私开关

**Files:**
- Modify: `src/components/settings/panels/AboutPanel.tsx`
- Modify: `src/components/settings/panels/AboutPanel.test.tsx`

- [ ] **Step 1: 写失败测试**

在 `src/components/settings/panels/AboutPanel.test.tsx` 里，已有 `renderAboutPanel` helper（或参照现有 `render` 调用）。新增两个测试：

```tsx
it('renders 隐私保护 section with a toggle', () => {
  render(<AboutPanel {...defaultProps} dataMaskingLevel="relaxed" onDataMaskingChange={() => {}} />)
  expect(screen.getByText('隐私保护增强')).toBeInTheDocument()
  const toggle = screen.getByRole('switch', { name: '隐私保护增强' })
  expect(toggle).not.toBeChecked()
})

it('calls onDataMaskingChange with strict when toggle turned on', async () => {
  const onChange = vi.fn()
  render(<AboutPanel {...defaultProps} dataMaskingLevel="relaxed" onDataMaskingChange={onChange} />)
  const toggle = screen.getByRole('switch', { name: '隐私保护增强' })
  await userEvent.click(toggle)
  expect(onChange).toHaveBeenCalledWith('strict')
})
```

> 注意：`defaultProps` 指已有测试里组装的 props 对象（`appName`, `version`, `copyright`, `logoUrl`, `onCheckUpdate`, `onUploadLogs`, `onResetData`, `links`）。新增的 `dataMaskingLevel` 和 `onDataMaskingChange` 需要加进去。

- [ ] **Step 2: 运行测试确认失败**

```bash
pnpm exec vitest run src/components/settings/panels/AboutPanel.test.tsx
```
Expected: 新增两个测试 FAIL（props 不存在）。

- [ ] **Step 3: 更新 AboutPanel props 类型**

在 `src/components/settings/panels/AboutPanel.tsx` 的 `AboutPanelProps` interface 加两个字段：
```ts
import type { DataMaskingLevel } from '@/types/settings'

interface AboutPanelProps {
  // ...原有字段...
  dataMaskingLevel: DataMaskingLevel
  onDataMaskingChange: (level: DataMaskingLevel) => void
}
```

- [ ] **Step 4: 函数签名加参数**

```tsx
export function AboutPanel({
  appName,
  version,
  copyright,
  logoUrl,
  onCheckUpdate,
  onUploadLogs,
  onResetData,
  links,
  dataMaskingLevel,
  onDataMaskingChange,
}: AboutPanelProps) {
```

- [ ] **Step 5: 在"帮助与反馈"section 前插入隐私 section**

在 `<div className="h-px bg-border mb-2" />` 之后、`<section className="flex flex-col gap-3">` 帮助反馈 section 之前，插入：

```tsx
      <section className="flex flex-col gap-4">
        <div className="text-xl font-bold tracking-tight text-foreground">隐私</div>

        <div className="flex items-center justify-between gap-8">
          <div className="flex min-w-0 flex-col gap-1">
            <div className="text-base font-semibold text-foreground">隐私保护增强</div>
            <div className="text-sm text-muted-foreground">
              开启后，发送给模型前会自动隐藏部分敏感信息。关闭后可获得更完整的上下文体验。
            </div>
          </div>
          <Switch
            aria-label="隐私保护增强"
            checked={dataMaskingLevel !== 'relaxed'}
            onCheckedChange={(checked) => onDataMaskingChange(checked ? 'strict' : 'relaxed')}
          />
        </div>
      </section>

      <div className="h-px bg-border mb-2" />
```

- [ ] **Step 6: 运行测试确认通过**

```bash
pnpm exec vitest run src/components/settings/panels/AboutPanel.test.tsx
```
Expected: 所有测试 PASS，包括新增两个。

- [ ] **Step 7: Commit**

```bash
git add src/components/settings/panels/AboutPanel.tsx src/components/settings/panels/AboutPanel.test.tsx
git commit -m "feat(privacy): add data masking toggle to AboutPanel"
```

---

### Task 5: SettingsModal 注入数据和回调

**Files:**
- Modify: `src/components/settings/SettingsModal.tsx`

- [ ] **Step 1: 读现有 about 渲染代码**

打开 `src/components/settings/SettingsModal.tsx`，找到 `{settingsModal === 'about' ? (` 块（约 152 行）。

- [ ] **Step 2: 注入 props**

找到 `<AboutPanel` 的渲染，加上两个新 props：
```tsx
<AboutPanel
  // ...原有 props 不变...
  dataMaskingLevel={useSettingsStore.getState().dataMaskingLevel ?? 'relaxed'}
  onDataMaskingChange={async (level) => {
    useSettingsStore.getState().setDataMaskingLevel(level)
    try {
      const current = await getSettings()
      await updateSettings({ ...current, dataMaskingLevel: level })
    } catch (err) {
      console.error('Failed to persist dataMaskingLevel:', err)
    }
  }}
/>
```

> SettingsModal 里其他 settings 字段（如 fontScale）也是用 `useSettingsStore.getState()` 或 hook 读取，保持一致即可。如果现有代码用的是 hook（`useSettingsStore((s) => s.xxx)`），则改为：
> ```tsx
> const dataMaskingLevel = useSettingsStore((s) => s.dataMaskingLevel ?? 'relaxed')
> ```
> 并把 `onDataMaskingChange` 回调里的 `setDataMaskingLevel` 改为 `useSettingsStore.getState().setDataMaskingLevel(level)`。

- [ ] **Step 3: 确认 TypeScript 无报错**

```bash
pnpm exec tsc --noEmit
```
Expected: 无 error。

- [ ] **Step 4: 运行全部前端测试**

```bash
pnpm exec vitest run src/components/settings
```
Expected: 全部 PASS。

- [ ] **Step 5: Commit**

```bash
git add src/components/settings/SettingsModal.tsx
git commit -m "feat(privacy): wire dataMaskingLevel toggle in SettingsModal"
```

---

### Task 6: 端到端冒烟

- [ ] **Step 1: 启动 dev 模式**

```bash
pnpm tauri:dev
```

- [ ] **Step 2: 验证默认值是 relaxed**

打开设置 → 关于 AI 小家，确认"隐私保护增强"开关默认处于**关闭**状态。

- [ ] **Step 3: 开启开关并确认持久化**

打开开关 → 重启应用 → 再次打开设置 → 确认开关仍为**开启**状态。

- [ ] **Step 4: 关闭开关确认可逆**

关闭开关 → 重启 → 确认开关回到**关闭**。

- [ ] **Step 5: 最终 Commit（如有未提交改动）**

```bash
git status
# 如有遗漏：
git add -A && git commit -m "chore: final cleanup for data masking privacy toggle"
```
