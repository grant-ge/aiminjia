# Inline Skill Token Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render slash skill commands such as `/dingtalk-workspace` as inline Chinese skill tokens in the rich composer and send the selected skill id structurally to the backend so the runtime reliably loads the skill.

**Architecture:** Add a TipTap inline atom `skillToken`, modeled after the existing attachment token. The serializer collects `skills` separately from markdown; `useChat` sends a `skillCommand` IPC payload; the backend carries it on `ChatTurnRequest`, persists it in user message metadata, and injects a deterministic instruction telling the model to call `Skill` with the selected `skill_id`.

**Tech Stack:** React, TipTap, Zustand, Vitest/JSDOM, Tauri IPC, Rust runtime chat pipeline.

---

## File Structure

- Create `src/components/rich-composer/SkillTokenView.tsx`: React NodeView rendering the inline skill tag with `Blocks` icon and remove button.
- Create `src/components/rich-composer/skillTokenExtension.ts`: TipTap node, attrs parser/renderer, command `insertSkillToken`, and input rule converting `/skill-id` to token using a skill lookup map.
- Modify `src/components/rich-composer/types.ts`: add `ComposerSkillToken`, `skills` on submit payload, and `skillToken` node type.
- Modify `src/components/rich-composer/composerSchema.ts`: include `SkillTokenExtension`, parameterized by available skills.
- Modify `src/components/rich-composer/serializer.ts`: collect skill tokens, omit them from markdown, and mark skill-only payloads as non-empty.
- Modify `src/components/rich-composer/RichComposer.tsx`: accept `skillTokens`, pass them to schema, expose `insertSkillToken`, remove legacy top-slot `skillCommand` chip use for this path.
- Modify `src/components/chat-scene/ChatBottomArea.tsx`: insert skill tokens on picker selection; pass current skills to composer; submit `payload.skills[0]` to `sendUserMessage`.
- Modify `src/components/home/HomeTaskComposerCard.tsx`: preserve existing behavior by passing skills and inserting skill tokens if this component uses the same composer skill flow.
- Modify `src/hooks/useChat.ts` and `src/lib/tauri.ts`: include `skillCommand` in optimistic message and IPC.
- Modify `src/types/message.ts` only if its `SkillCommandBreadcrumb` shape lacks needed fields.
- Modify `src-tauri/src/runtime/chat/chat_turn_driver.rs`: add serializable `SkillCommandRef`, `skill_command` on `ChatTurnRequest`, and persist `skillCommand` in user content JSON.
- Modify `src-tauri/src/commands/chat.rs` and `src-tauri/src/transport/tauri_commands/chat.rs`: accept and forward `skill_command`.
- Modify `src-tauri/src/runtime/chat/context_builder.rs` or `src-tauri/src/transport/tauri_commands/chat.rs`: inject selected skill instruction into dynamic context.
- Add/modify tests beside the files above.

## Task 1: Composer skill token node and serializer

**Files:**
- Create: `src/components/rich-composer/SkillTokenView.tsx`
- Create: `src/components/rich-composer/skillTokenExtension.ts`
- Modify: `src/components/rich-composer/types.ts`
- Modify: `src/components/rich-composer/composerSchema.ts`
- Modify: `src/components/rich-composer/serializer.ts`
- Test: `src/components/rich-composer/__tests__/skillTokenExtension.test.ts`
- Test: `src/components/rich-composer/__tests__/serializer.test.ts`

- [ ] **Step 1: Add failing serializer tests**

Add tests to `src/components/rich-composer/__tests__/serializer.test.ts`:

```ts
const skill = (overrides: Partial<ComposerJsonNode['attrs']> = {}): ComposerJsonNode => ({
  type: 'skillToken',
  attrs: {
    id: 'dingtalk-workspace',
    label: '玩转钉钉',
    command: '/dingtalk-workspace',
    ...overrides,
  },
})

describe('serializeComposerDoc — skillToken', () => {
  it('skill token is collected but omitted from markdown', () => {
    const result = serializeComposerDoc(doc(p(skill(), t(' 帮我查今天日程'))))
    expect(result.markdown).toBe(' 帮我查今天日程')
    expect(result.skills).toEqual([
      { id: 'dingtalk-workspace', label: '玩转钉钉', command: '/dingtalk-workspace' },
    ])
    expect(result.isEmpty).toBe(false)
  })

  it('skill-only submit is not empty', () => {
    const result = serializeComposerDoc(doc(p(skill())))
    expect(result.markdown).toBe('')
    expect(result.skills).toHaveLength(1)
    expect(result.isEmpty).toBe(false)
  })

  it('duplicate skill tokens are collected once', () => {
    const result = serializeComposerDoc(doc(p(skill(), t(' and '), skill())))
    expect(result.skills).toEqual([
      { id: 'dingtalk-workspace', label: '玩转钉钉', command: '/dingtalk-workspace' },
    ])
  })
})
```

Also update existing exact `toEqual({ markdown, attachments, isEmpty })` expectations to include `skills: []`.

- [ ] **Step 2: Run serializer tests to verify RED**

Run: `pnpm vitest run src/components/rich-composer/__tests__/serializer.test.ts`

Expected: FAIL because `skillToken` is not a known `ComposerJsonNodeType`, `RichComposerSubmitPayload` has no `skills`, and `serializeComposerDoc` does not collect skill tokens.

- [ ] **Step 3: Add failing extension tests**

Create `src/components/rich-composer/__tests__/skillTokenExtension.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { Editor } from '@tiptap/core'
import StarterKit from '@tiptap/starter-kit'
import { SkillTokenExtension } from '../skillTokenExtension'
import type { ComposerSkillToken } from '../types'

const skill: ComposerSkillToken = {
  id: 'dingtalk-workspace',
  label: '玩转钉钉',
  command: '/dingtalk-workspace',
}

function makeEditor() {
  return new Editor({
    extensions: [StarterKit, SkillTokenExtension.configure({ skills: [skill] })],
    content: '<p></p>',
  })
}

describe('skillTokenExtension', () => {
  it('insertSkillToken inserts a skillToken node', () => {
    const editor = makeEditor()
    editor.commands.insertSkillToken(skill)
    const json = editor.getJSON()
    const para = (json.content as Array<{ content?: unknown[] }>)[0]
    const tokenNode = (para.content as Array<{ type: string; attrs: ComposerSkillToken }>).find(
      (node) => node.type === 'skillToken',
    )
    expect(tokenNode?.attrs).toMatchObject(skill)
    editor.destroy()
  })

  it('input rule converts slash id followed by a space into a skillToken', () => {
    const editor = makeEditor()
    editor.commands.insertContent('/dingtalk-workspace ')
    const json = editor.getJSON()
    const para = (json.content as Array<{ content?: Array<{ type: string; attrs?: ComposerSkillToken; text?: string }> }>)[0]
    expect((para.content ?? []).some((node) => node.type === 'skillToken' && node.attrs?.label === '玩转钉钉')).toBe(true)
    expect(JSON.stringify(json)).not.toContain('/dingtalk-workspace')
    editor.destroy()
  })

  it('unknown slash id stays as text', () => {
    const editor = makeEditor()
    editor.commands.insertContent('/unknown-skill ')
    expect(editor.getText()).toContain('/unknown-skill')
    editor.destroy()
  })

  it('HTML round-trip preserves skill attrs', () => {
    const editor = makeEditor()
    editor.commands.insertSkillToken(skill)
    const html = editor.getHTML()
    expect(html).toContain('data-rich-composer-skill-token')
    expect(html).toContain('data-id="dingtalk-workspace"')
    const editor2 = new Editor({
      extensions: [StarterKit, SkillTokenExtension.configure({ skills: [skill] })],
      content: html,
    })
    const para = (editor2.getJSON().content as Array<{ content?: Array<{ type: string; attrs?: ComposerSkillToken }> }>)[0]
    const node = (para.content ?? []).find((item) => item.type === 'skillToken')
    expect(node?.attrs?.label).toBe('玩转钉钉')
    editor2.destroy()
    editor.destroy()
  })
})
```

- [ ] **Step 4: Run extension tests to verify RED**

Run: `pnpm vitest run src/components/rich-composer/__tests__/skillTokenExtension.test.ts`

Expected: FAIL because `skillTokenExtension.ts` does not exist.

- [ ] **Step 5: Implement types**

Modify `src/components/rich-composer/types.ts`:

```ts
export interface ComposerSkillToken {
  id: string
  label: string
  command: string
}

export interface RichComposerSubmitPayload {
  markdown: string
  attachments: ComposerAttachmentToken[]
  skills: ComposerSkillToken[]
  isEmpty: boolean
}

export type ComposerJsonNodeType =
  | 'doc'
  | 'paragraph'
  | 'text'
  | 'hardBreak'
  | 'blockquote'
  | 'codeBlock'
  | 'bulletList'
  | 'orderedList'
  | 'listItem'
  | 'attachmentToken'
  | 'skillToken'
```

- [ ] **Step 6: Implement SkillTokenView**

Create `src/components/rich-composer/SkillTokenView.tsx`:

```tsx
import { NodeViewWrapper } from '@tiptap/react'
import { Blocks, X } from 'lucide-react'
import { cn } from '@/lib/utils'
import type { ComposerSkillToken } from './types'

interface SkillTokenViewProps {
  node: { attrs: ComposerSkillToken }
  deleteNode: () => void
}

export function SkillTokenView({ node, deleteNode }: SkillTokenViewProps) {
  const attrs = node.attrs
  return (
    <NodeViewWrapper
      as="span"
      data-skill-chip
      contentEditable={false}
      className={cn(
        'inline-flex max-w-[180px] items-center gap-1 rounded-md border border-border bg-muted px-1.5 py-0.5 align-middle text-xs leading-none text-foreground',
      )}
      title={attrs.command}
    >
      <Blocks aria-label="skill" className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
      <span className="truncate">{attrs.label}</span>
      <button
        type="button"
        aria-label={`remove skill ${attrs.label}`}
        onMouseDown={(event) => event.preventDefault()}
        onClick={(event) => {
          event.preventDefault()
          event.stopPropagation()
          deleteNode()
        }}
        className="ml-0.5 inline-flex h-4 w-4 shrink-0 items-center justify-center rounded hover:bg-background"
      >
        <X className="h-3 w-3" />
      </button>
    </NodeViewWrapper>
  )
}
```

- [ ] **Step 7: Implement SkillTokenExtension**

Create `src/components/rich-composer/skillTokenExtension.ts` using the attachment token pattern:

```ts
import { InputRule, Node, mergeAttributes } from '@tiptap/core'
import { ReactNodeViewRenderer } from '@tiptap/react'
import type { ReactNodeViewProps } from '@tiptap/react'
import type { ComponentType } from 'react'
import { SkillTokenView } from './SkillTokenView'
import type { ComposerSkillToken } from './types'

declare module '@tiptap/core' {
  interface Commands<ReturnType> {
    skillToken: {
      insertSkillToken: (token: ComposerSkillToken) => ReturnType
    }
  }
}

const DATA_ATTR = 'data-rich-composer-skill-token'
const CARET_BOUNDARY = '\u200B'

export interface SkillTokenExtensionOptions {
  skills: ComposerSkillToken[]
}

function normalizeCommand(value: string) {
  return value.startsWith('/') ? value : `/${value}`
}

function findSkill(skills: ComposerSkillToken[], slashText: string): ComposerSkillToken | null {
  const command = normalizeCommand(slashText.trim())
  return skills.find((skill) => skill.command === command || `/${skill.id}` === command) ?? null
}

export const SkillTokenExtension = Node.create<SkillTokenExtensionOptions>({
  name: 'skillToken',
  group: 'inline',
  inline: true,
  atom: true,
  selectable: false,
  draggable: true,

  addOptions() {
    return { skills: [] }
  },

  addAttributes() {
    return {
      id: { default: null },
      label: { default: null },
      command: { default: null },
    }
  },

  parseHTML() {
    return [{
      tag: `span[${DATA_ATTR}]`,
      getAttrs: (el) => {
        if (!(el instanceof HTMLElement)) return false
        const id = el.getAttribute('data-id')
        const label = el.getAttribute('data-label')
        const command = el.getAttribute('data-command')
        if (!id || !label || !command) return false
        return { id, label, command }
      },
    }]
  },

  renderHTML({ HTMLAttributes, node }) {
    const attrs = node.attrs as Partial<ComposerSkillToken>
    if (!attrs.id || !attrs.label || !attrs.command) {
      return ['span', mergeAttributes(HTMLAttributes, { [DATA_ATTR]: '' })]
    }
    return ['span', mergeAttributes(HTMLAttributes, {
      [DATA_ATTR]: '',
      'data-id': attrs.id,
      'data-label': attrs.label,
      'data-command': attrs.command,
    })]
  },

  addNodeView() {
    return ReactNodeViewRenderer(
      SkillTokenView as unknown as ComponentType<ReactNodeViewProps>,
    )
  },

  addCommands() {
    return {
      insertSkillToken:
        (token: ComposerSkillToken) =>
        ({ chain, state }) => {
          let c = chain()
          if (state.selection.$from.parentOffset === 0) {
            c = c.insertContent({ type: 'text', text: CARET_BOUNDARY })
          }
          return c.insertContent({ type: 'skillToken', attrs: token }).run()
        },
    }
  },

  addInputRules() {
    return [
      new InputRule({
        find: /(?:^|\s)(\/[a-z0-9][a-z0-9_-]{1,63})\s$/,
        handler: ({ state, range, match, chain }) => {
          const skill = findSkill(this.options.skills, match[1])
          if (!skill) return
          const leading = match[0].startsWith(' ') ? ' ' : ''
          const from = range.from + leading.length
          chain()
            .deleteRange({ from, to: range.to })
            .insertContentAt(from, { type: 'skillToken', attrs: skill })
            .run()
        },
      }),
    ]
  },
})
```

- [ ] **Step 8: Wire schema options**

Modify `src/components/rich-composer/composerSchema.ts`:

```ts
import type { ComposerSkillToken } from './types'
import { SkillTokenExtension } from './skillTokenExtension'

export interface BuildComposerExtensionsOptions {
  placeholder?: string
  skills?: ComposerSkillToken[]
}

export function buildComposerExtensions(options: BuildComposerExtensionsOptions = {}) {
  return [
    StarterKit.configure({ ... }),
    Placeholder.configure({ placeholder: options.placeholder ?? '' }),
    AttachmentTokenExtension,
    SkillTokenExtension.configure({ skills: options.skills ?? [] }),
  ]
}
```

- [ ] **Step 9: Implement serializer support**

Modify `src/components/rich-composer/serializer.ts`:

```ts
import type { ComposerSkillToken } from './types'

export function serializeComposerDoc(doc: ComposerJsonNode): RichComposerSubmitPayload {
  const attachments: ComposerAttachmentToken[] = []
  const skills: ComposerSkillToken[] = []
  const markdown = renderBlocks(doc.content ?? [], attachments, skills)
  const isEmpty = markdown.trim().length === 0 && attachments.length === 0 && skills.length === 0
  return { markdown, attachments, skills, isEmpty }
}
```

Thread `skills` through `renderBlocks`, `renderBlock`, `renderInline`, list and blockquote helpers. In `renderInline`, add:

```ts
} else if (node.type === 'skillToken') {
  collectSkillToken(node, skills)
}
```

Then add:

```ts
function collectSkillToken(node: ComposerJsonNode, skills: ComposerSkillToken[]): void {
  const token = readSkillTokenAttrs(node)
  if (!token) return
  if (!skills.some((existing) => existing.id === token.id)) {
    skills.push(token)
  }
}

function readSkillTokenAttrs(node: ComposerJsonNode): ComposerSkillToken | null {
  const attrs = node.attrs ?? {}
  const id = typeof attrs.id === 'string' ? attrs.id : null
  const label = typeof attrs.label === 'string' ? attrs.label : null
  const command = typeof attrs.command === 'string' ? attrs.command : null
  if (!id || !label || !command) return null
  return { id, label, command }
}
```

- [ ] **Step 10: Run Task 1 tests to verify GREEN**

Run: `pnpm vitest run src/components/rich-composer/__tests__/serializer.test.ts src/components/rich-composer/__tests__/skillTokenExtension.test.ts`

Expected: PASS.

## Task 2: Composer UI integration and chat submit path

**Files:**
- Modify: `src/components/rich-composer/RichComposer.tsx`
- Modify: `src/components/chat-scene/ChatBottomArea.tsx`
- Modify: `src/components/home/HomeTaskComposerCard.tsx`
- Modify: `src/hooks/useChat.ts`
- Modify: `src/lib/tauri.ts`
- Test: `src/components/rich-composer/__tests__/RichComposer.test.tsx`
- Test: `src/components/chat-scene/__tests__/ChatBottomArea.test.tsx`
- Test: `src/hooks/useChat.test.ts`

- [ ] **Step 1: Add failing RichComposer tests**

Add to `src/components/rich-composer/__tests__/RichComposer.test.tsx`:

```ts
it('ref.insertSkillToken inserts inline skill and submit includes skills without markdown command', async () => {
  const onSubmit = vi.fn()
  const handleRef = createRef<RichComposerHandle>()
  render(<RichComposer ref={handleRef} onSubmit={onSubmit} />)
  await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
  act(() => {
    handleRef.current?.insertSkillToken({
      id: 'dingtalk-workspace',
      label: '玩转钉钉',
      command: '/dingtalk-workspace',
    })
  })
  const editor = document.querySelector('.ProseMirror') as HTMLElement
  expect(editor.textContent).toContain('玩转钉钉')
  fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })
  await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1))
  const payload = onSubmit.mock.calls[0][0]
  expect(payload.skills).toEqual([
    { id: 'dingtalk-workspace', label: '玩转钉钉', command: '/dingtalk-workspace' },
  ])
  expect(payload.markdown).not.toContain('/dingtalk-workspace')
})

it('typing slash command converts to inline skill token', async () => {
  const user = userEvent.setup()
  render(
    <RichComposer
      onSubmit={() => {}}
      skillTokens={[{ id: 'dingtalk-workspace', label: '玩转钉钉', command: '/dingtalk-workspace' }]}
    />,
  )
  await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
  const editor = document.querySelector('.ProseMirror') as HTMLElement
  await user.click(editor)
  await user.type(editor, '/dingtalk-workspace ')
  await waitFor(() => expect(editor.textContent).toContain('玩转钉钉'))
  expect(editor.textContent).not.toContain('/dingtalk-workspace')
})
```

- [ ] **Step 2: Run RichComposer tests to verify RED**

Run: `pnpm vitest run src/components/rich-composer/__tests__/RichComposer.test.tsx`

Expected: FAIL because `insertSkillToken` and `skillTokens` props do not exist.

- [ ] **Step 3: Implement RichComposer props and ref**

Modify `src/components/rich-composer/RichComposer.tsx`:

```ts
import type { ComposerAttachmentToken, ComposerJsonNode, ComposerSkillToken, RichComposerSubmitPayload } from './types'

export interface RichComposerProps {
  ...
  skillTokens?: ComposerSkillToken[]
}

export interface RichComposerHandle {
  focus: () => void
  insertAttachmentTokens: (tokens: ComposerAttachmentToken[]) => void
  insertSkillToken: (token: ComposerSkillToken) => void
  clear: () => void
  getEditor: () => Editor | null
}
```

Pass skills to schema:

```ts
const editor = useEditor({
  extensions: buildComposerExtensions({ placeholder, skills: skillTokens ?? [] }),
  ...
})
```

Expose command:

```ts
insertSkillToken: (token) => {
  editor?.commands.insertSkillToken(token)
},
```

- [ ] **Step 4: Add failing ChatBottomArea test**

Extend `src/components/chat-scene/__tests__/ChatBottomArea.test.tsx` by importing `useSkillStore` and adding setup skill state. Add test:

```ts
import { useSkillStore } from '@/stores/skillStore'

// in beforeEach:
useSkillStore.setState({
  skills: [{
    id: 'dingtalk-workspace',
    displayName: '玩转钉钉',
    displayNameEn: 'DingTalk Workspace',
    description: 'desc',
    source: 'global',
    hasWorkflow: false,
    icon: '',
    shortDescription: 'desc',
    shortDescriptionEn: 'desc',
    triggerText: '/dingtalk-workspace',
    category: 'general',
    updatedAt: null,
  }],
})

it('picking a skill inserts inline token and submit passes skill metadata', async () => {
  const user = userEvent.setup()
  const { container } = render(<ChatBottomArea />)
  await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
  const skillButton = container.querySelector('[aria-label="composer.openSkillPicker"]') as HTMLElement
  await user.click(skillButton)
  await user.click(await screen.findByText('玩转钉钉'))
  const editor = document.querySelector('.ProseMirror') as HTMLElement
  expect(editor.textContent).toContain('玩转钉钉')
  expect(editor.textContent).not.toContain('/dingtalk-workspace')
  await user.click(editor)
  await user.type(editor, ' 查今天日程')
  fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })
  await waitFor(() => expect(mockSendUserMessage).toHaveBeenCalledTimes(1))
  expect(mockSendUserMessage.mock.calls[0][0]).toBe(' 查今天日程')
  expect(mockSendUserMessage.mock.calls[0][2]).toEqual({
    id: 'dingtalk-workspace',
    label: '玩转钉钉',
    command: '/dingtalk-workspace',
  })
})
```

- [ ] **Step 5: Run ChatBottomArea test to verify RED**

Run: `pnpm vitest run src/components/chat-scene/__tests__/ChatBottomArea.test.tsx`

Expected: FAIL because picker still inserts slash text and payload has no `skills`.

- [ ] **Step 6: Implement ChatBottomArea token insertion**

Modify `src/components/chat-scene/ChatBottomArea.tsx`:

```ts
const skills = useSkillStore((s) => s.skills)

const skillTokens = useMemo(() => skills.map((skill) => ({
  id: skill.id,
  label: skill.displayName || skill.id,
  command: skill.triggerText || `/${skill.id}`,
})), [skills])

const handleSkillPick = useCallback((skillId: string) => {
  const skill = getSkillById(skillId)
  const token = {
    id: skillId,
    label: skill?.displayName || skill?.id || skillId,
    command: skill?.triggerText || `/${skillId}`,
  }
  composerRef.current?.insertSkillToken(token)
  composerRef.current?.focus()
  setShowSkillPopover(false)
}, [getSkillById])
```

Pass into `RichComposer`:

```tsx
<RichComposer
  ...
  skillTokens={skillTokens}
/>
```

In submit:

```ts
const skillForThisTurn = payload.skills[0] ?? null
...
await sendUserMessage(markdownToSend, fileInfos.length > 0 ? fileInfos : undefined, skillForThisTurn)
```

Remove or stop using the old `selectedSkill` state in this component.

- [ ] **Step 7: Update useChat tests for skill IPC**

Modify `src/hooks/useChat.test.ts`; replace the old “does not pass skill id param” expectation with:

```ts
it('passes selected skill command to sendMessage IPC', async () => {
  const { result } = renderHook(() => useChat())

  await act(async () => {
    await result.current.sendUserMessage('查日程', undefined, {
      id: 'dingtalk-workspace',
      label: '玩转钉钉',
      command: '/dingtalk-workspace',
    })
  })

  expect(tauriMock.sendMessage).toHaveBeenCalledWith(
    'conv-test',
    '查日程',
    undefined,
    null,
    expect.any(String),
    { id: 'dingtalk-workspace', label: '玩转钉钉', command: '/dingtalk-workspace' },
  )
})
```

- [ ] **Step 8: Run useChat test to verify RED**

Run: `pnpm vitest run src/hooks/useChat.test.ts`

Expected: FAIL because `sendMessage` signature does not accept skill command.

- [ ] **Step 9: Implement frontend IPC plumbing**

Modify `src/lib/tauri.ts`:

```ts
export interface SkillCommandPayload {
  id: string
  label?: string
  command?: string
}

export function sendMessage(
  conversationId: string,
  content: string,
  attachments?: ChatAttachmentPayload[],
  agentName?: string | null,
  clientMessageId?: string,
  skillCommand?: SkillCommandPayload | null,
): Promise<void> {
  return invoke<void>('send_message', {
    conversationId,
    content,
    attachments: attachments ?? [],
    agentName: agentName ?? null,
    clientMessageId: clientMessageId ?? null,
    skillCommand: skillCommand ?? null,
  })
}
```

Modify `src/hooks/useChat.ts`:

```ts
import type { SkillCommandPayload } from '@/lib/tauri'
...
skill?: SkillCommandPayload | null,
```

Add optimistic message metadata:

```ts
content: {
  text,
  skillCommand: skill ? {
    id: skill.id,
    label: skill.label ?? skill.id,
    command: skill.command ?? `/${skill.id}`,
  } : undefined,
  files: ...
}
```

Call IPC:

```ts
await sendMessage(conversationId, text, files, null, messageId, skill ?? null)
```

- [ ] **Step 10: Run Task 2 tests to verify GREEN**

Run: `pnpm vitest run src/components/rich-composer/__tests__/RichComposer.test.tsx src/components/chat-scene/__tests__/ChatBottomArea.test.tsx src/hooks/useChat.test.ts`

Expected: PASS.

## Task 3: Backend skill command plumbing and prompt injection

**Files:**
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Modify: `src-tauri/src/commands/chat.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`
- Test: add unit tests in `src-tauri/src/runtime/chat/chat_turn_driver.rs` or an integration test under `src-tauri/tests/skill_command_request_test.rs`

- [ ] **Step 1: Add failing Rust tests for content JSON**

Add tests near `build_user_content_json` tests in `src-tauri/src/runtime/chat/chat_turn_driver.rs`:

```rust
#[test]
fn build_user_content_json_includes_skill_command() {
    let skill = SkillCommandRef {
        id: "dingtalk-workspace".to_string(),
        label: Some("玩转钉钉".to_string()),
        command: Some("/dingtalk-workspace".to_string()),
    };
    let value = build_user_content_json_with_skill("查日程", &[], Some(&skill));
    assert_eq!(value["text"], "查日程");
    assert_eq!(value["skillCommand"]["id"], "dingtalk-workspace");
    assert_eq!(value["skillCommand"]["label"], "玩转钉钉");
    assert_eq!(value["skillCommand"]["command"], "/dingtalk-workspace");
}

#[test]
fn selected_skill_instruction_mentions_skill_tool_and_id() {
    let skill = SkillCommandRef {
        id: "dingtalk-workspace".to_string(),
        label: Some("玩转钉钉".to_string()),
        command: Some("/dingtalk-workspace".to_string()),
    };
    let text = selected_skill_instruction(Some(&skill)).expect("instruction");
    assert!(text.contains("玩转钉钉"));
    assert!(text.contains("dingtalk-workspace"));
    assert!(text.contains("Skill"));
    assert!(text.contains("skill_id"));
}
```

- [ ] **Step 2: Run Rust test to verify RED**

Run: `cd src-tauri && cargo test build_user_content_json_includes_skill_command selected_skill_instruction_mentions_skill_tool_and_id --lib`

Expected: FAIL because `SkillCommandRef`, `build_user_content_json_with_skill`, and `selected_skill_instruction` do not exist.

- [ ] **Step 3: Implement Rust request types and helpers**

Modify `src-tauri/src/runtime/chat/chat_turn_driver.rs`:

```rust
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillCommandRef {
    pub id: String,
    pub label: Option<String>,
    pub command: Option<String>,
}
```

Add field to `ChatTurnRequest`:

```rust
pub skill_command: Option<SkillCommandRef>,
```

Initialize to `None` in `ChatTurnRequest::new`.

Replace `build_user_content_json` implementation with a wrapper:

```rust
pub fn build_user_content_json(
    content: &str,
    attachments: &[ChatAttachmentRef],
) -> serde_json::Value {
    build_user_content_json_with_skill(content, attachments, None)
}

pub fn build_user_content_json_with_skill(
    content: &str,
    attachments: &[ChatAttachmentRef],
    skill_command: Option<&SkillCommandRef>,
) -> serde_json::Value {
    let mut value = serde_json::json!({ "text": content });
    if let Some(skill) = skill_command {
        value["skillCommand"] = serde_json::to_value(skill).unwrap_or(serde_json::Value::Null);
    }
    // existing files logic unchanged
    value
}

pub fn selected_skill_instruction(skill_command: Option<&SkillCommandRef>) -> Option<String> {
    let skill = skill_command?;
    let id = skill.id.trim();
    if id.is_empty() { return None; }
    let label = skill.label.as_deref().unwrap_or(id);
    Some(format!(
        "用户已在输入框选择专项技能：{label}。请第一步使用 Skill 工具加载详细指令，参数 skill_id=\"{id}\"，然后再处理用户请求。"
    ))
}
```

- [ ] **Step 4: Wire persistence and dynamic context**

Modify `src-tauri/src/transport/tauri_commands/chat.rs`:

- Change `persist_user_message` signature to accept `skill_command: Option<&SkillCommandRef>` and call `build_user_content_json_with_skill(content, attachments, skill_command)`.
- Wherever `persist_user_message` is called with a `request`, pass `request.skill_command.as_ref()`.
- In `build_user_message_content`, keep content only; do not put skill instruction in user markdown.
- In dynamic context construction before the LLM step, append `selected_skill_instruction(request.skill_command.as_ref())` to the dynamic context or turn prompt snapshot. The concrete minimal implementation: locate where `build_iteration_context` receives dynamic context inputs in `RuntimeChatTurnDriver`; include the selected skill instruction in the context string before skill catalog, so it is visible to the model but not persisted as user text.

- [ ] **Step 5: Wire Tauri command parameters**

Modify `src-tauri/src/commands/chat.rs`:

```rust
use crate::runtime::chat::chat_turn_driver::SkillCommandRef;
...
skill_command: Option<SkillCommandRef>,
```

Pass it to adapter `.send_message(...)`.

Modify adapter `send_message` signature in `src-tauri/src/transport/tauri_commands/chat.rs`:

```rust
skill_command: Option<crate::runtime::chat::chat_turn_driver::SkillCommandRef>,
```

Set:

```rust
request.skill_command = skill_command;
```

Pending queue note: if session is busy, queued `PendingItem` currently has no skill field. Keep current behavior for now: selected skill only applies to direct sends. Add a log warning when `skill_command.is_some()` and message is queued, or extend `PendingItem` in a follow-up. Since the current user request is composer/direct behavior, do not expand queue scope in this task.

- [ ] **Step 6: Run Task 3 tests to verify GREEN**

Run: `cd src-tauri && cargo test build_user_content_json_includes_skill_command selected_skill_instruction_mentions_skill_tool_and_id --lib`

Expected: PASS.

## Task 4: End-to-end verification and review

**Files:**
- No required production files; fixes only if verification reveals issues.

- [ ] **Step 1: Run focused frontend tests**

Run:

```bash
pnpm vitest run \
  src/components/rich-composer/__tests__/serializer.test.ts \
  src/components/rich-composer/__tests__/skillTokenExtension.test.ts \
  src/components/rich-composer/__tests__/RichComposer.test.tsx \
  src/components/chat-scene/__tests__/ChatBottomArea.test.tsx \
  src/hooks/useChat.test.ts
```

Expected: PASS.

- [ ] **Step 2: Run backend focused tests**

Run:

```bash
cd src-tauri && cargo test build_user_content_json_includes_skill_command selected_skill_instruction_mentions_skill_tool_and_id --lib
```

Expected: PASS.

- [ ] **Step 3: Run typecheck/build check**

Run:

```bash
pnpm lint
pnpm test -- --runInBand
```

If these scripts do not exist or are too broad for this repo, run:

```bash
pnpm vitest run
cd src-tauri && cargo check
```

Expected: PASS or report exact existing failures unrelated to this change.

- [ ] **Step 4: Manual dev verification**

Run with proxy if needed:

```bash
export https_proxy=http://127.0.0.1:7897 http_proxy=http://127.0.0.1:7897 all_proxy=socks5://127.0.0.1:7897
pnpm run tauri:dev
```

Manual checks:

1. Open chat composer.
2. Type `/dingtalk-workspace `.
3. Expected: inline token appears with label `玩转钉钉`; `/dingtalk-workspace` disappears from visible text.
4. Type `帮我查今天日程` and send.
5. Expected: user bubble shows skill metadata/tag; backend logs or model behavior show it first loads `Skill` with `skill_id="dingtalk-workspace"`.

- [ ] **Step 5: Request code review**

Use `superpowers:requesting-code-review` or dispatch a reviewer subagent with:

```text
Review the inline skill token implementation. Requirements: /skill-id converts to a Chinese inline token, serializer sends markdown separately from skill metadata, IPC carries skillCommand, backend persists skillCommand and injects a deterministic Skill(skill_id) instruction. Focus on regressions in attachments, pending queue behavior, and prompt pollution.
```

Expected: no Critical/Important issues remain.

---

## Self-Review

- Spec coverage: covers inline tag, `/xxx` conversion, structured backend skill id, and no manual Chinese label matching.
- Placeholder scan: no TBD/TODO placeholders; each task has exact files, tests, and commands.
- Type consistency: frontend uses `ComposerSkillToken`/`SkillCommandPayload`; backend uses `SkillCommandRef`; all include `id`, optional/required label as appropriate, and `command`.
