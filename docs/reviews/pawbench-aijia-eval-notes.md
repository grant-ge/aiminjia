# PawBench AIjia eval notes

## 2026-06-24 runtime stall guard

Previous focused run:

- Lotus commit: `685e2fc2`
- Task: `task_00016_moltbook_auto_post_skill_creation`
- Result path: `C:\Users\Administrator\Desktop\github\PawBench\685e2fc2_task00016_unreadyguard_c1_j1_20260624_172251\20260624_172252\pawbench\deepseek-v4-flash\aijia\20260624_172253.json`
- Score: `0.07666666666666666`
- Elapsed: about `628s`
- Evidence: PawBench log showed `maxIterations=120`, so the remaining failure was not caused by the old PawBench `--max-iterations 40` cap or by the earlier CLI default of `15`.

Diagnosis:

- The model stream could stay alive by emitting hidden `thinking.delta` events without visible text or tool calls.
- Normal chunk timeout did not fire in that state, so one model step could consume the whole PawBench timeout before the delivery guard got another useful chance.
- The default turn output budget was effectively unbounded (`1_000_000`), which made this worse for models that spend a long time in hidden reasoning.
- The delivery guard was also throttled by iteration count after the first reminder, so a timeout or empty model completion could still leave a named deliverable missing.

Change:

- Bound the default per-step output budget to `32_768` tokens.
- Add a thinking-only streaming timeout when there is no visible content and no tool call.
- Let the named-deliverable guard repeat while the file remains missing or placeholder-like, up to the existing guard limit.

Boundary:

- This is a global runtime reliability fix, not a PawBench task-specific prompt adaptation.
- Next verification must rebuild the current `aijia-cli` and rerun the focused PawBench task before any broader full-suite claim.

## 2026-06-24 evidence writeback prompt

Focused verification after commit `80ba2c52`:

- Task: `task_00016_moltbook_auto_post_skill_creation`
- Result path: `C:\Users\Administrator\Desktop\github\PawBench\80ba2c52_task00016_thinkingguard_c1_j1_20260624_194417\20260624_194418\pawbench\deepseek-v4-flash\aijia\20260624_194420.json`
- Score: `0.7833333333333333`
- Previous comparable score: `0.07666666666666666`

What improved:

- `SKILL.md` and `diagnosis-report.md` were both generated in the workspace root.
- Automated grading gave full credit for skill structure/content, report existence, issue identification, and manual-vs-fixed separation.

Remaining failure pattern:

- The model read `config.json`, `package.json`, `post-history.json`, and additional script content after writing `diagnosis-report.md`.
- It identified missing `node_modules`, missing `node`, empty access token, and a rate-limit timing conflict in later reasoning.
- It did not write those later confirmed findings back into `diagnosis-report.md`; the report still contained outdated "not read / not verified" statements.
- The final run also exited with PawBench `status=error` after timing out, so there is still a runtime exit issue to inspect separately.

Prompt change:

- Added a global evidence-writeback contract to `system.md`: after a report or other named deliverable is written, later evidence must be edited back into that deliverable before final response.
- Added a global analysis rule for config/history/log/time-window tasks: compute concrete values such as cooldown remaining, quota remaining, and next eligible time, and write those values into the required report/result.

Boundary:

- This is not a task-specific patch. It targets all report, audit, cron/config, time-window, quota, log, and data-analysis tasks where later evidence can invalidate an earlier draft.

## 2026-06-24 evidence checkpoint prompt

Focused verification after commit `1bea3502`:

- Task: `task_00016_moltbook_auto_post_skill_creation`
- Result path: `C:\Users\Administrator\Desktop\github\PawBench\1bea3502_task00016_evidencewriteback_retry_c1_j1_20260624_212522\20260624_212527\pawbench\deepseek-v4-flash\aijia\20260624_212528.json`
- Score: `0.6093333333333334`
- Status: `error`
- Tokens: `16631`

What the sample proves:

- The agent created both required root deliverables: `SKILL.md` and `diagnosis-report.md`.
- Automated checks scored full credit for skill structure/content, report existence, issue identification, and fixed-vs-manual separation.
- The model read the relevant files and confirmed key facts in the transcript: `config.json`, `package.json`, `post-history.json`, `post.js`, missing `node`, missing `node_modules`, and `node-fetch`/`dayjs` dependencies.

Remaining failure pattern:

- `diagnosis-report.md` was written too early as a pending/speculative report and was not edited after the key evidence was discovered.
- The transcript shows the model recognized core findings, then launched more exploratory Bash checks instead of first updating the deliverable.
- The task reached the 600s PawBench timeout before those later findings were written back.
- The judge gave rate-limit analysis only partial credit because the final report did not cross-reference `2026-02-10T07:55:12Z`, `minIntervalMinutes: 180`, 148 minutes elapsed, 32 minutes remaining, and next eligible time `2026-02-10T10:55:12Z` / `18:55 Asia/Shanghai`.

Prompt change:

- Strengthened `system.md` so "key evidence found" becomes an explicit writeback checkpoint.
- If new tool results confirm a core conclusion, config field, dependency state, history timestamp, time window, test result, or manual action, the next step must update the named deliverable before more exploration.
- Early report skeletons are now framed as short in-progress structures only; they must not become long speculative reports that look complete.

Boundary:

- This remains a global delivery-quality rule, not a task-specific grader patch.

## 2026-06-24 delivery guard shell-write alignment

Focused verification after commit `d7e65806`:

- Task: `task_00016_moltbook_auto_post_skill_creation`
- Result path: `C:\Users\Administrator\Desktop\github\PawBench\d7e65806_task00016_evidencecheckpoint_c1_j1_20260624_214555\20260624_214556\pawbench\deepseek-v4-flash\aijia\20260624_214557.json`
- Score: `0.07666666666666666`
- Status: `error`
- Elapsed: about `608s`

What the sample proves:

- The service route in the gate log was `route_model=deepseek-v4-flash`; the earlier `deepseek-v3` observation came from an invalid API-error result and is not evidence of real model routing.
- The named-file delivery guard did extract `SKILL.md` and `diagnosis-report.md`, and repeatedly injected the blocking prompt.
- The run still failed to create the root deliverables, so prompt-only evidence checkpointing is not sufficient for this failure mode.

Remaining failure pattern:

- The guard message says `Write`, `Edit`, or equivalent file writing is acceptable, but the runtime allow-check only treated literal `Write`/`Edit` tool calls as delivery progress.
- The model repeatedly said it would write the files while the guard kept reporting that non-write tool calls were skipped.
- This mismatch can trap the turn in a loop where shell-based file creation is never allowed to execute, and the required files remain missing.

Runtime change:

- Treat a shell command as satisfying the delivery guard only when it clearly writes to one of the missing named targets, such as `cat > SKILL.md`, `tee diagnosis-report.md`, `Set-Content`, `Out-File`, or common programmatic write helpers.
- Continue blocking ordinary shell reads or exploration commands that merely mention the target file.
- Also accept absolute `Write`/`Edit` paths that end in the requested workspace target, which is common inside Docker workspaces.

Verification:

- `wsl -d Ubuntu-24.04 -u root -- bash -lc "cd /mnt/c/Users/Administrator/.codex/worktrees/70e8/lotus-app/src-tauri && cargo test --lib delivery_guard_"`
- Result: 6 focused tests passed.
