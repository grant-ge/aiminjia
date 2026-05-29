# Image Artifact Preview Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make generated image artifacts (PNG/JPG/WebP/GIF/BMP/SVG) preview inside the app from report/product file cards.

**Architecture:** Keep the existing generated-file preview flow. Extend frontend previewability detection, extend Tauri `get_file_preview` with an `image` variant that returns a safe data URL, and render that variant in `FilePreviewPane` without exposing local filesystem paths.

**Tech Stack:** React + TypeScript + Vitest, Tauri Rust command tests.

---

### Task 1: Frontend Preview Classification

**Files:**
- Modify: `src/components/chat/generatedFileActions.ts`
- Test: `src/hooks/__tests__/useTurnRenderModel.test.ts`

- [ ] Step 1: Add a failing test that a PNG generated file with preview action is previewable and uses preview as primary action.
- [ ] Step 2: Run `pnpm vitest run src/hooks/__tests__/useTurnRenderModel.test.ts --runInBand` or the project-supported equivalent and confirm the new test fails.
- [ ] Step 3: Add image extensions/types to `PREVIEWABLE_FILE_TYPES`.
- [ ] Step 4: Re-run the targeted test and confirm it passes.

### Task 2: Tauri Image Preview Contract

**Files:**
- Modify: `src-tauri/src/commands/file.rs`
- Modify: `src/lib/tauri.ts`

- [ ] Step 1: Add Rust failing tests for PNG preview serialization and extension fallback.
- [ ] Step 2: Run a targeted Rust test command for the file preview module and confirm the image tests fail.
- [ ] Step 3: Add `FilePreview::Image { fileName, mimeType, dataUrl }`, image kind normalization, MIME mapping, and base64 data URL encoding.
- [ ] Step 4: Add `image` to the TypeScript `FilePreview` union.
- [ ] Step 5: Re-run the targeted Rust tests and confirm they pass.

### Task 3: Frontend Image Rendering

**Files:**
- Modify: `src/components/chat/FilePreviewPane.tsx`
- Test: `src/components/chat/FilePreviewPane.test.tsx`

- [ ] Step 1: Add a failing Vitest case that an image preview response renders an `<img>` with the data URL and accessible alt text.
- [ ] Step 2: Run the targeted preview pane test and confirm it fails.
- [ ] Step 3: Render `kind: "image"` as an in-app image preview.
- [ ] Step 4: Re-run targeted frontend tests and confirm they pass.

### Task 4: Verification

**Files:**
- Verify only; no writes expected.

- [ ] Run targeted frontend tests for `FilePreviewPane` and `useTurnRenderModel`.
- [ ] Run targeted Rust preview tests or `cargo test preview_tests --lib` if acceptable.
- [ ] Review `git diff` to ensure only intended files changed and no user changes were reverted.
