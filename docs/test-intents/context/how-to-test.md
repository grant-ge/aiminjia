# how-to-test.md — 操作规范

## 执行测试意图的标准流程

1. 测试文件命名：`review_<需求名>_test.rs`，放在 `src-tauri/tests/` 下
2. 每条意图对应一个独立测试函数
3. 跑命令：`cd src-tauri && cargo test --test review_<需求名>_test -- --nocapture`
4. 把结果（通过/失败/遇到的坑）写进对应的 `spec/tasks/<需求>/test-progress.md`

## 测试函数命名规范

```rust
// 格式：<被测功能>_<场景>_<预期结果>
#[test]
fn masking_level_relaxed_does_not_mask_id_card() { ... }

#[tokio::test]
async fn skill_listing_injected_once_per_conversation() { ... }
```

## 先验证旧测试是否漂移

- 不要默认现有 `review_*.rs` 仍然贴合当前实现
- 执行前先运行一次旧测试，确认是否编译通过、是否反映当前规则
- 如果旧测试失败，先判断是**测试漂移**（测试没跟上实现）还是**实现漂移**（实现没跟上规则）

## 改 settings 分层后必须补跑关联回归

- 修改了 global/workspace settings 合并逻辑后，不能只跑目标测试文件
- 必须补跑 `plan_ae_config_layers_test`，避免破坏其他 settings merge 约束

## 多轮 ToolCalls 测试注意事项

- `ToolCalls { tool_calls: vec![] }` 会让 driver 继续循环
- 最后必须补一个 `ContentComplete` 才能正常收口
- 多轮测试除断言每轮值一致外，还要记录 `load_llm_settings_for_turn()` 调用次数，确认只读了一次

## TempDir 注意事项

```rust
// 错误：dir 被提前 drop，目录消失
let storage = AppStorage::new(TempDir::new().unwrap().path()).unwrap();

// 正确：用变量保持 dir 存活
let dir = TempDir::new().unwrap();
let storage = AppStorage::new(dir.path()).unwrap();
```

## 清理

- TempDir 测试结束自动清理，不需要手动删除
- MockExecutor 是内存对象，测试结束自动释放
