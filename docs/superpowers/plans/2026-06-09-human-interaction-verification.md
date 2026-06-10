# Human Interaction Verification

## Automated

- `cargo test --manifest-path src-tauri/Cargo.toml --lib human_interaction -- --nocapture`: PASS
- `cargo test --manifest-path src-tauri/Cargo.toml --lib runtime::run_registry -- --nocapture`: PASS
  - Note: poison-mutex tests intentionally print panic lines; final test result passed.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib pending::queue_manager -- --nocapture`: PASS
- `cargo test --manifest-path src-tauri/Cargo.toml --lib connector::im::shared::ask_coordinator -- --nocapture`: PASS
- `cargo test --manifest-path src-tauri/Cargo.toml --lib connector::im::shared::reply_manager -- --nocapture`: PASS
- `cargo check --manifest-path src-tauri/Cargo.toml`: PASS

## Manual

- DingTalk allow once: Not run in this automated pass.
- DingTalk allow always and `permissions.json` persistence: Not run in this automated pass.
- DingTalk abandon permission into a new AskUserQuestion turn: Not run in this automated pass.
- DingTalk AskUserQuestion free-text answer: Not run in this automated pass.
- APP-only output does not go to IM: Covered by reply-manager unit test `app_only_run_does_not_lazy_create_im_card_from_session_credentials`; live manual not run.
- Non-DingTalk shared channel path: Not configured locally; shared-envelope tests cover all platform metadata.

## Notes

- Live IM scenarios require an active configured external channel and user messages.
- This pass records automated coverage and leaves live DingTalk verification for an interactive dev-server run.
