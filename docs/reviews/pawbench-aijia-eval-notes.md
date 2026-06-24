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
