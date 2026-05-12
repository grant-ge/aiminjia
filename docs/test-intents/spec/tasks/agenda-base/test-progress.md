# test-progress.md — Agenda 基座意图执行记录

## 状态

- 意图规格：`docs/test-intents/spec/tasks/agenda-base/rules.md`
- 已实现测试文件：`src-tauri/tests/review_agenda_base_test.rs`（意图 47-48 的 4 条源码扫描断言）
- 当前覆盖方式：48 条意图全部被 cargo test 锁住，分布在：lib unit tests（53 + 1 新增 = 54 条）、`agenda_commands_test.rs`（5 条）、`review_agenda_runner_scope.rs`（1 条）、`review_agenda_command_thinness.rs`（1 条）、`review_agenda_base_test.rs`（4 条），合计 65 条
- 执行命令：
  - `cd src-tauri && cargo test --lib runtime::agenda --no-fail-fast`
  - `cd src-tauri && cargo test --lib transport::tauri_commands::agenda --no-fail-fast`
  - `cd src-tauri && cargo test --test agenda_commands_test --test review_agenda_runner_scope --test review_agenda_command_thinness --test review_agenda_base_test --no-fail-fast`
- 最近执行结果：65 passed；agenda 相关功能全绿

| 意图 | 状态 | 当前覆盖测试 |
|---|---|---|
| 意图 1：合法 item 落盘后能读回 | ✅ 已覆盖 | `runtime::agenda::store::tests::create_persists_item` |
| 意图 2：participants 长度不为 1 时拒绝 | ✅ 已覆盖 | `runtime::agenda::store::tests::rejects_participants_len_not_one` |
| 意图 3：organizer 不在 participants[0] 时拒绝 | ✅ 已覆盖 | `runtime::agenda::store::tests::rejects_organizer_not_in_participants` |
| 意图 4：override_of 非空时拒绝 | ✅ 已覆盖 | `runtime::agenda::store::tests::rejects_override_of_set` |
| 意图 5：rule.by_day 非空时拒绝 | ✅ 已覆盖 | `runtime::agenda::store::tests::rejects_rule_with_by_day` |
| 意图 6：rule.by_month_day 非空时拒绝 | ✅ 已覆盖 | `runtime::agenda::store::tests::rejects_rule_with_by_month_day` |
| 意图 7：一次性 item 设 skip_dates 时拒绝 | ✅ 已覆盖 | `runtime::agenda::store::tests::rejects_skip_dates_on_one_shot` |
| 意图 8：update 时 organizer 不可改 | ✅ 已覆盖 | `runtime::agenda::store::tests::update_rejects_organizer_change_when_not_orphaned` |
| 意图 9：Orphaned 时允许改 organizer | ✅ 已覆盖 | `runtime::agenda::store::tests::update_allows_organizer_change_when_orphaned` |
| 意图 10：item id 含路径穿越字符时拒绝 | ✅ 已覆盖 | `get_rejects_path_traversal_id` / `create_rejects_path_traversal_id_without_writing_outside_file` / `delete_rejects_path_traversal_id_without_removing_outside_file` / `append_occurrence_rejects_path_traversal_item_id_without_writing_outside_dir` / `list_occurrences_rejects_path_traversal_item_id` |
| 意图 11：occurrence 写入按 yyyy-mm 分片，多次追加只读最后一条 | ✅ 已覆盖 | `append_occurrence_creates_jsonl_shard` + `read_occurrences_returns_last_state_per_id` |
| 意图 12：mark_orphaned_by_organizer 只翻 Active/Paused | ✅ 已覆盖 | `mark_orphaned_flips_status_for_matching_organizer` + `mark_orphaned_skips_already_completed` |
| 意图 13：一次性 future next_fire_at = start_at | ✅ 已覆盖 | `trigger_eval::tests::one_shot_future_returns_start_at` |
| 意图 14：一次性 equal-now 仍返回 start_at | ✅ 已覆盖 | `trigger_eval::tests::one_shot_equal_now_returns_start_at` |
| 意图 15：一次性 past 返回 None | ✅ 已覆盖 | `trigger_eval::tests::one_shot_past_returns_none` |
| 意图 16：一次性已触发返回 None | ✅ 已覆盖 | `trigger_eval::tests::one_shot_already_fired_returns_none` |
| 意图 17：Daily 返回未来第一个 | ✅ 已覆盖 | `trigger_eval::tests::daily_returns_first_future_occurrence` |
| 意图 18：Daily interval=2 跳过中间日 | ✅ 已覆盖 | `trigger_eval::tests::daily_interval_2_skips_every_other_day` |
| 意图 19：Weekly 步进 7 天 | ✅ 已覆盖 | `trigger_eval::tests::weekly_steps_seven_days` |
| 意图 20：Monthly 步进 1 月 | ✅ 已覆盖 | `trigger_eval::tests::monthly_steps_one_month` |
| 意图 21：Yearly 步进 1 年 | ✅ 已覆盖 | `trigger_eval::tests::yearly_steps_one_year` |
| 意图 22：Yearly 闰日跳到下一闰年 | ✅ 已覆盖 | `trigger_eval::tests::yearly_leap_day_skips_invalid_years` |
| 意图 23：长间隔 catch-up 不卡死 | ✅ 已覆盖 | `trigger_eval::tests::daily_long_catch_up_returns_next_future_occurrence` |
| 意图 24：Count 达 N 后 None | ✅ 已覆盖 | `trigger_eval::tests::count_returns_none_after_n_occurrences` |
| 意图 25：Count 未达 N 时仍返回 | ✅ 已覆盖 | `trigger_eval::tests::count_returns_some_when_under_n` |
| 意图 26：Count 不消耗错过的时间槽 | ✅ 已覆盖 | `trigger_eval::tests::count_does_not_consume_missed_scheduled_slots` |
| 意图 27：Until 超过 until 后 None | ✅ 已覆盖 | `trigger_eval::tests::until_returns_none_after_until_at` |
| 意图 28：skip_dates 命中跳到下一个 | ✅ 已覆盖 | `trigger_eval::tests::skip_dates_skips_to_next` |
| 意图 29：take_due 只取 Active+due 的 item | ✅ 已覆盖 | `runtime::agenda::store::tests::take_due_returns_active_items_with_past_next_fire_at` |
| 意图 30：take_due 跳过 Paused/Completed/Orphaned | ✅ 已覆盖 | `runtime::agenda::store::tests::take_due_skips_paused_completed_orphaned` |
| 意图 31：advance_after_fire 推进 count 并重算 | ✅ 已覆盖 | `runtime::agenda::store::tests::advance_after_fire_increments_count_and_recomputes` |
| 意图 32：一次性 advance 后翻 Completed | ✅ 已覆盖 | `runtime::agenda::store::tests::advance_after_fire_one_shot_marks_completed` |
| 意图 33：advance_after_fire 在非 Active 时拒绝且不改字段 | ✅ 已覆盖 | `runtime::agenda::store::tests::advance_after_fire_rejects_non_active_without_mutating` |
| 意图 34：set_skip 仅在 rule.is_some 时允许 | ✅ 已覆盖 | `runtime::agenda::store::tests::set_skip_rejects_one_shot` |
| 意图 35：set_skip 加入 skip_dates 后可 unset | ✅ 已覆盖 | `set_skip_adds_to_skip_dates` + `unset_skip_removes_from_skip_dates` |
| 意图 36：runner 每 tick 重 resolve scope | ✅ 已覆盖 | `tests/review_agenda_runner_scope.rs::runner_module_re_resolves_scope_in_loop` |
| 意图 37：run_due_once 派发 Scheduled trigger_source | ✅ 已覆盖 | `runtime::agenda::runner::tests::run_due_once_dispatches_active_items` 锁数量；`runtime::agenda::runner::tests::run_due_once_marks_dispatch_with_scheduled_trigger_source` 锁序列化值 `"scheduled"` |
| 意图 38：run_due_once 无 due 时不派发 | ✅ 已覆盖 | `runtime::agenda::runner::tests::run_due_once_skips_when_no_due` |
| 意图 39：每个 #[tauri::command] 函数体 < 30 行 | ✅ 已覆盖 | `tests/review_agenda_command_thinness.rs::agenda_commands_only_delegate_to_store_or_dispatcher` |
| 意图 40：title 为空拒绝 | ✅ 已覆盖 | `transport::tauri_commands::agenda::tests::build_create_item_rejects_blank_title` |
| 意图 41：prompt 为空拒绝 | ✅ 已覆盖 | `transport::tauri_commands::agenda::tests::build_create_item_rejects_blank_prompt` |
| 意图 42：organizer_persona_id 为空拒绝 | ✅ 已覆盖 | `transport::tauri_commands::agenda::tests::build_create_item_rejects_blank_organizer` |
| 意图 43：timezone 非 IANA 拒绝 | ✅ 已覆盖 | `transport::tauri_commands::agenda::tests::build_create_item_rejects_invalid_timezone` |
| 意图 44：timezone 空白时默认 Asia/Shanghai 且 trim 字段 | ✅ 已覆盖 | `transport::tauri_commands::agenda::tests::build_create_item_trims_required_fields_and_defaults_blank_timezone` |
| 意图 45：apply_update 字段 None 不动、空字符串拒绝 | ✅ 已覆盖 | `apply_update_trims_fields_and_recomputes_next_fire` + `apply_update_rejects_blank_title` + `apply_update_rejects_blank_prompt` + `apply_update_rejects_blank_timezone` + `apply_update_rejects_invalid_timezone` |
| 意图 46：rule:null vs rule 缺失语义区分 | ✅ 已覆盖 | `update_request_json_rule_null_means_clear_rule` + `update_request_json_missing_rule_means_leave_unchanged` |
| 意图 47：run_agenda_item_now 出参 String | ✅ 已覆盖 | `tests/review_agenda_base_test.rs::run_agenda_item_now_backend_returns_string_occurrence_id` + `…::run_agenda_item_now_frontend_wrapper_returns_promise_string` |
| 意图 48：list_agenda_occurrences 入参不含 before | ✅ 已覆盖 | `tests/review_agenda_base_test.rs::list_agenda_occurrences_backend_takes_only_item_id_and_limit` + `…::list_agenda_occurrences_frontend_wrapper_takes_only_item_id_and_limit` |

## 执行记录

- 2026-05-07：起草本规格，与已落盘的 60+ cargo 测试逐条比对：46 条已覆盖、1 条部分覆盖（意图 37 未锁 trigger_source 序列化值）、2 条未覆盖（意图 47-48 是 PR-2 收尾新引入的签名收窄）
- 2026-05-07：补齐 3 条空白：
  - 意图 37：在 `runtime::agenda::runner::tests` 增加 `run_due_once_marks_dispatch_with_scheduled_trigger_source`，扩展 `RecordingDispatcher` 记 `triggers: Vec<TriggerSource>`，断言 `serde_json::to_string(&triggers[0]) == "\"scheduled\""`
  - 意图 47-48：新建 `src-tauri/tests/review_agenda_base_test.rs`，4 条源码扫描断言（前后端 × run_now / list_occurrences）
  - 用 mutation 验证 4 条新断言都有牙：把 `TriggerSource::Scheduled` → `ManualRunNow` / `Promise<string>` → `Promise<Occurrence>` / 在 list_occurrences 加 `before` 参数，对应测试都精确变红，恢复后全绿
- 已知坑：
  - **`Mutex<()>` 内部锁住单进程并发，但没有显式多线程并发写测试**。spec §10.1 提到"并发安全"，目前缺。
  - **runner 端到端集成测试**（runner spawn → tick → dispatcher → occurrence 落盘）属 PR-4 任务 56，本期 store + dispatcher 分别测，未串成一条
  - **`review_sub_agent_background_reachability_test`** 用字面字符串 grep `"background: false"` 来探测硬编码，目前靠把 fixture 改成 `Default::default()` 绕过；这是字面扫描的弱点，将来应改为忽略 `#[cfg(test)]` 块

## 待 Codex 实现的测试新增点

（本期 48 条意图全部覆盖完成，无遗留。下方为 PR-3/PR-4 接入后的回填工作，不属于 PR-2。）

- spec §10.2 列出但归 PR-4 任务 56 的：`tests/agenda_runner_scope_test.rs`（runner 端到端切 scope）、`tests/review_agenda_session_id.rs`、`tests/review_agenda_phase1_constraints.rs`
- spec §9 + PR-4 任务 55 的 `tests/agenda_persona_delete_test.rs`（persona 删除联动）
- PR-2 收尾删除 `build_user_content_json_includes_selected_skill_metadata` 留下的前端 normalize 回归（在 plan F2 B 组 follow-up TODO 中登记）
