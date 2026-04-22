# test-progress.md — masking-level 传递链路执行记录

## 状态

| 意图 | 状态 | 备注 |
|---|---|---|
| 意图 1：relaxed 下 PII 不被脱敏 | ✅ 已通过 | `masking_level_relaxed_turn_keeps_id_card_unmasked_for_llm` |
| 意图 2：strict 下 PII 全部被脱敏 | ✅ 已通过 | `masking_level_strict_turn_masks_all_pii_for_llm` |
| 意图 3：空值回退 strict | ✅ 已通过 | `masking_level_invalid_storage_values_fall_back_to_strict` |
| 意图 4：多轮循环中 masking_level 保持一致 | ✅ 已通过 | `masking_level_snapshot_is_reused_across_multi_step_turn` |

## 执行记录

- 读取上下文后先跑现有 `src-tauri/tests/review_masking_level_settings_test.rs`，发现其引用了一个已不存在的 `ResolvedLlmSettings.masking_level` 字段，说明测试与实现已漂移，无法直接验证规则。
- 根因定位：`/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/runtime/chat/chat_turn_driver.rs` 仍把 `TurnConfig.masking_level` 硬编码为 `"strict"`；`/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_commands/chat.rs` 解析设置时也没有把 `data_masking_level` 带入 `ResolvedLlmSettings`，导致 masking_level 传递链路实际断裂。
- 按 TDD 重写 `src-tauri/tests/review_masking_level_settings_test.rs`，新增 4 条规则对应的 turn 级回归测试，并额外补了 2 条 settings 分层测试：workspace 覆盖 global，以及 workspace 配置损坏时静默回退 global。
- 实现修复：
  - 在 `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/runtime/chat/turn_config.rs` 给 `ResolvedLlmSettings` 补回 `masking_level` 字段，默认值为 `strict`。
  - 在 `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_commands/chat.rs` 将 `AppSettings.data_masking_level` 规范化后写入 `ResolvedLlmSettings.masking_level`。
  - 在 `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/runtime/chat/chat_turn_driver.rs` 使用 `llm_settings.masking_level` 初始化 `TurnConfig.masking_level`，不再硬编码 `strict`。
  - 在 `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/storage/file_store/workspace_settings.rs` 与 `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/storage/file_store/mod.rs` 增加 `dataMaskingLevel` 的 workspace-level 覆盖支持，并保持解析失败时静默回退 global。
- 验证命令：
  - `cd src-tauri && cargo test --test review_masking_level_settings_test -- --nocapture` → 7 个测试全部通过。
  - `cd src-tauri && cargo test --test plan_ae_config_layers_test -- --nocapture` → 17 个测试全部通过，确认这次 settings 分层扩展未破坏既有约束。
- 结论：当前 masking_level 已能从 settings 正确解析、在 turn 内快照一次，并稳定传递到每轮 `run_llm_step`；strict / relaxed / fallback strict / workspace 覆盖与静默回退行为均已有回归测试覆盖。
