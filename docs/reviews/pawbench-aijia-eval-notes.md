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

Follow-up verification after commit `2d2b5005`:

- Task: `task_00016_moltbook_auto_post_skill_creation`
- Result path: `C:\Users\Administrator\Desktop\github\PawBench\2d2b5005_task00016_shellwriteguard_c1_j1_20260624_220650\20260624_220655\pawbench\deepseek-v4-flash\aijia\20260624_220655.json`
- Score: `0.47583333333333333`
- Status: `success`
- Key improvement: `SKILL.md` and `diagnosis-report.md` were present before grading.
- Remaining miss: the report did not analyze `post-history.json` timing/rate-limit evidence and falsely said file access was blocked.

Second runtime adjustment:

- Keep the immediate guard when the assistant tries to finish with named deliverables missing.
- Add a three-tool-iteration grace period before prompting after tool rounds, so the assistant can read source, config, logs, and history before the delivery guard starts forcing file writes.
- Once the delivery guard has started, continue blocking non-write calls until the named deliverables are ready.

Verification:

- `wsl -d Ubuntu-24.04 -u root -- bash -lc "cd /mnt/c/Users/Administrator/.codex/worktrees/70e8/lotus-app/src-tauri && cargo test --lib guard"`
- Result: 28 focused tests passed.

Follow-up verification after commit `e6fb7c67`:

- Task: `task_00016_moltbook_auto_post_skill_creation`
- Result path: `C:\Users\Administrator\Desktop\github\PawBench\e6fb7c67_task00016_guardgrace_c1_j1_20260624_221407\20260624_221408\pawbench\deepseek-v4-flash\aijia\20260624_221409.json`
- Score: `0.7293333333333334`
- Status: `success`
- Key improvement: the agent read `post-history.json` and `config.json` before writing the final report.
- Remaining miss: it compared the rate-limit timestamps incorrectly, concluding the opposite of the expected `148 minutes < 180 minutes` result.

Prompt adjustment experiment:

- Add an explicit time-window calculation contract to `system.md`: normalize time zones, write `last_event_time`, `current_time`, `elapsed`, `required_interval`, `remaining_wait`, and `next_eligible_time`, then derive the rate-limit/cooldown conclusion from `elapsed >= required_interval`.

Regression check after commit `10eb36f5`:

- Task: `task_00016_moltbook_auto_post_skill_creation`
- Result path: `C:\Users\Administrator\Desktop\github\PawBench\10eb36f5_task00016_timecalc_c1_j1_20260624_221851\20260624_221853\pawbench\deepseek-v4-flash\aijia\20260624_221853.json`
- Score: `0.6513333333333333`
- Status: `success`
- Regression: score dropped from `0.7293333333333334` to `0.6513333333333333`, tokens rose from `24,921` to `50,031`, and the report still misinterpreted UTC/rate-limit timing.
- Decision: revert the extra global time-window prompt text. The effective fix for this sample is the runtime delivery-guard grace period, not more global prompt detail.

Verification after reverting the prompt experiment at commit `82640222`:

- Task: `task_00016_moltbook_auto_post_skill_creation`
- Result path: `C:\Users\Administrator\Desktop\github\PawBench\82640222_task00016_revertedprompt_c1_j1_20260624_222405\20260624_222406\pawbench\deepseek-v4-flash\aijia\20260624_222407.json`
- Score: `0.8013333333333333`
- Status: `error` due to `EXIT_CODE_NONZERO`, but files were present and graded.
- Improvement: the report identified the 180-minute rate-limit conflict and evidence coverage scored `1.0`.
- Remaining issue: the final report still lacked the exact `148 minutes` / `18:55 Asia/Shanghai` next-eligible-time detail, and the turn continued into extra verification instead of cleanly stopping after a usable report.

Placeholder-check experiment after commit `b9a07cad`:

- Task: `task_00016_moltbook_auto_post_skill_creation`
- Result path: `C:\Users\Administrator\Desktop\github\PawBench\b9a07cad_task00016_unfinishedreport_c1_j1_20260624_223935\20260624_223937\pawbench\deepseek-v4-flash\aijia\20260624_223937.json`
- Score: `0.6993333333333334`
- Status: `error` due to `EXIT_CODE_NONZERO`.
- Regression: score dropped from `0.8013333333333333` to `0.6993333333333334`; the report still missed the correct next eligible time and introduced unsupported claims such as `npm install` succeeding.
- Decision: revert the expanded placeholder detector. It is too blunt for this task and does not solve the timestamp reasoning issue.

Verification:

- `wsl -d Ubuntu-24.04 -u root -- bash -lc "cd /mnt/c/Users/Administrator/.codex/worktrees/70e8/lotus-app/src-tauri && cargo test --lib guard"`
- Result after reverting the placeholder detector: 28 focused tests passed.

## 2026-06-24 full-run regression after reverting placeholder detector

Full verification at commit `454d4637`:

- Result path: `C:\Users\Administrator\Desktop\github\PawBench\454d4637_full_150_c16_j4_20260624_225655\20260624_225659\pawbench\deepseek-v4-flash\aijia\20260624_225700.json`
- Score: `0.6216406289156756`
- Comparable baseline: `7e0399a4_full_150_c16_j4_relogin_20260624_144446`, score `0.6604642811744125`
- Delta: `-0.0388236522587369`
- Status distribution: `success=125`, `error=25`
- API-invalid-like count by notes/errors: current `6`, baseline `2`

Major regressions:

- `task_00003_a_stock_announcements_scheduled_fetch`: `0.964 -> 0.0`, likely dominated by AI service error / short transcript rather than a useful prompt signal.
- `T103_schema_migration`: `0.96 -> 0.0`.
- `task_00066_svpwm_implementation_for_edge_aligned_pwm_motor_controller`: `0.927 -> 0.0`, no implementation files were produced.
- `dialogue-parser`: `0.904 -> 0.0`, skill was read but `solution.py`, `dialogue.json`, and `dialogue.dot` were not produced.
- Visual artifact group regressed heavily because `output/output.html` was not generated after PNG metadata checks: `M005`, `M006`, `M007`, `M010`, `M011`, and `M012`.
- Several skill/report tasks still show the old failure shape where source files are read but named deliverables are not created, such as `task_00016`, `task_00028`, and `task_00069`.

Major improvements:

- `task_00095_prompt_injection_defense_framework_with_skill_creation`: `0.0 -> 0.946`.
- `task_meeting_gov_controversy`: `0.0 -> 0.858`.
- `r2r-mpc-control`: `0.2 -> 0.936`.
- Skill / cron / agent composition tasks improved substantially: `234-doc-butler`, `233-translator`, `227-weekly-report`, `225-multi-config`, and `230-study-buddy`.
- `T101_wal_recovery`, `task_video_transcript_extraction`, `task_earnings_analysis`, and several safety/data tasks improved from very low baselines.

Interpretation:

- The full-run drop is not explained by one reverted experiment. It combines external API instability, several implementation tasks stopping before file creation, and a concentrated visual artifact failure group.
- The strongest prompt-level opportunity is not a task-specific music rule. It is a general artifact-first rule for media-to-output tasks: when the user already gives precise structure, fields, data, layout, or interaction requirements, failure to view the original media must not block creation of the requested file.
- This aligns with the QoderWork prompt design pattern: artifact tasks have explicit file-creation triggers, single-file HTML/SVG guidance, and a visible final artifact contract. The lotus prompt already has delivery and visual-fallback rules, but the HTML/SVG first-write requirement was too implicit.

Prompt change planned after this run:

- Strengthen `system.md` so HTML/SVG/React and other visual artifacts must be written as a usable first version, not a placeholder.
- Strengthen media fallback so one reasonable view/parse attempt is enough before writing a target artifact when the user supplied explicit content and acceptance requirements.
- Keep the rule global: it applies to visualizations, UI reproductions, diagrams, reports, and interactive pages, not to any PawBench task ID, fixed answer, or benchmark path.

Expected effect:

- Improve media-to-artifact tasks that currently stop after binary/metadata probing.
- Reduce zero-score outcomes where a requested `output/output.html` or similar artifact is never created.
- Avoid increasing deep reading loops; the first version should be valid, inspectable, and later refinable if visual evidence becomes available.

## 2026-06-24 tool-description and tool-error prompt alignment

Follow-up adjustment after commit `bde63fbf`:

- Changed the `Read` tool description so binary media limitations are visible before the model calls the tool: media files do not return raw content, so the model should use metadata/OCR/screenshot/parser paths or explicit user specs.
- Changed the binary `Read` tool result message so it says the read was not a successful visual/content inspection, and gives the next action: if the user already supplied explicit structure, fields, layout, data, or acceptance criteria for a required output file, create that artifact now and mark only unverified visual details.
- Changed the `ImageTask` tool description to state that it creates/edits images but is not an image viewer, OCR tool, chart parser, or visual QA tool.
- Changed shell command-not-found feedback (`exit_code=127`) so the next step is to use an installed alternative, a small script in an available runtime, or continue from verified evidence rather than retrying the same missing command.
- Added truncation-aware hints to the `Read` tool description, truncated `Read` results, context-decayed older tool results, and persisted large tool-result previews. These tell the model that previews are incomplete and that relevant omitted content must be read before relying on the result.

QoderWork reference pattern:

- QoderWork separates global behavior from tool-local protocols. Its prompt explicitly describes tool selection preference, shell path semantics, artifact creation rules, and file-sharing behavior near the relevant tool section rather than relying only on one global instruction block.
- The lotus change follows that pattern at a smaller scope: keep the global system prompt as the general contract, but make tool descriptions and tool error messages carry the immediate recovery instruction at the point where the model is most likely to branch incorrectly.

Boundary:

- This is a generic tool-usability improvement, not a PawBench task-specific adaptation. It does not mention any task id, sample file, benchmark path, or fixed answer.
- It changes prompt/error text only. It does not alter tool execution, grading, permissions, or file detection behavior.

Expected effect:

- Reduce loops after missing local commands such as `file` or `identify`.
- Reduce misuse of image-generation tooling as a viewing/OCR mechanism.
- Improve artifact tasks where the user has already supplied enough concrete content to produce a useful first output even when independent media viewing is unavailable.
- Reduce false conclusions from truncated file or tool-output previews by making the recovery action explicit at the point of truncation.
