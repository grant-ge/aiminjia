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
