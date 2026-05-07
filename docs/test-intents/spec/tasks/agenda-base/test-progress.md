# test-progress.md — Agenda 基座意图执行记录

## 状态

- 意图规格：`docs/test-intents/spec/tasks/agenda-base/rules.md`
- 计划测试文件（尚未创建）：`src-tauri/tests/review_agenda_base_test.rs`
- 当前覆盖方式：意图 1-46 的产品承诺已经被现有 cargo test 套件锁住（散落在 lib unit tests、`agenda_commands_test.rs`、`review_agenda_runner_scope.rs`、`review_agenda_command_thinness.rs` 中），意图 47-48 是 PR-2 收尾新引入的签名收窄，需新增源码扫描断言。
- 执行命令：
  - `cd src-tauri && cargo test --lib runtime::agenda --no-fail-fast`
  - `cd src-tauri && cargo test --lib transport::tauri_commands::agenda --no-fail-fast`
  - `cd src-tauri && cargo test --test agenda_commands_test --test review_agenda_runner_scope --test review_agenda_command_thinness --no-fail-fast`
- 最近执行结果：60 passed；agenda 相关功能全绿

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
| 意图 37：run_due_once 派发 Scheduled trigger_source | ⚠️ 部分覆盖 | `runtime::agenda::runner::tests::run_due_once_dispatches_active_items` 只断言派发数量，**未断言 trigger_source 序列化值为 `"scheduled"`**。建议补一条 |
| 意图 38：run_due_once 无 due 时不派发 | ✅ 已覆盖 | `runtime::agenda::runner::tests::run_due_once_skips_when_no_due` |
| 意图 39：每个 #[tauri::command] 函数体 < 30 行 | ✅ 已覆盖 | `tests/review_agenda_command_thinness.rs::agenda_commands_only_delegate_to_store_or_dispatcher` |
| 意图 40：title 为空拒绝 | ✅ 已覆盖 | `transport::tauri_commands::agenda::tests::build_create_item_rejects_blank_title` |
| 意图 41：prompt 为空拒绝 | ✅ 已覆盖 | `transport::tauri_commands::agenda::tests::build_create_item_rejects_blank_prompt` |
| 意图 42：organizer_persona_id 为空拒绝 | ✅ 已覆盖 | `transport::tauri_commands::agenda::tests::build_create_item_rejects_blank_organizer` |
| 意图 43：timezone 非 IANA 拒绝 | ✅ 已覆盖 | `transport::tauri_commands::agenda::tests::build_create_item_rejects_invalid_timezone` |
| 意图 44：timezone 空白时默认 Asia/Shanghai 且 trim 字段 | ✅ 已覆盖 | `transport::tauri_commands::agenda::tests::build_create_item_trims_required_fields_and_defaults_blank_timezone` |
| 意图 45：apply_update 字段 None 不动、空字符串拒绝 | ✅ 已覆盖 | `apply_update_trims_fields_and_recomputes_next_fire` + `apply_update_rejects_blank_title` + `apply_update_rejects_blank_prompt` + `apply_update_rejects_blank_timezone` + `apply_update_rejects_invalid_timezone` |
| 意图 46：rule:null vs rule 缺失语义区分 | ✅ 已覆盖 | `update_request_json_rule_null_means_clear_rule` + `update_request_json_missing_rule_means_leave_unchanged` |
| 意图 47：run_agenda_item_now 出参 String | ⏳ 未覆盖 | spec §6 已加签名收窄说明，但**无源码扫描断言**锁住前后端类型一致。Codex 实现 `review_agenda_base_test.rs` 时新增 |
| 意图 48：list_agenda_occurrences 入参不含 before | ⏳ 未覆盖 | 同上，新增源码扫描断言 |

## 执行记录

- 2026-05-07：起草本规格，与已落盘的 60+ cargo 测试逐条比对：46 条已覆盖、1 条部分覆盖（意图 37 未锁 trigger_source 序列化值）、2 条未覆盖（意图 47-48 是 PR-2 收尾新引入的签名收窄）
- 已知坑：
  - **`Mutex<()>` 内部锁住单进程并发，但没有显式多线程并发写测试**。spec §10.1 提到"并发安全"，目前缺。
  - **runner 端到端集成测试**（runner spawn → tick → dispatcher → occurrence 落盘）属 PR-4 任务 56，本期 store + dispatcher 分别测，未串成一条
  - **`review_sub_agent_background_reachability_test`** 用字面字符串 grep `"background: false"` 来探测硬编码，目前靠把 fixture 改成 `Default::default()` 绕过；这是字面扫描的弱点，将来应改为忽略 `#[cfg(test)]` 块

## 待 Codex 实现的测试新增点

1. 在 `runtime::agenda::runner::tests` 增补一条断言 trigger_source 序列化值为 `"scheduled"`，覆盖意图 37 的协议字段完整性
2. 新增 `src-tauri/tests/review_agenda_base_test.rs`：
   - 意图 47：源码扫描 `src/transport/tauri_commands/agenda.rs` 含 `run_agenda_item_now` 且签名包含 `Result<String, String>`；扫描 `src/lib/tauri.ts` 含 `runAgendaItemNow` 且返回类型字面 `Promise<string>`
   - 意图 48：源码扫描 Rust 端 `list_agenda_occurrences` 参数不含 `before` 字符串；扫描 TS 端 `listAgendaOccurrences` 参数列表不含 `before`
