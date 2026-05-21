# 下一个 session 接力指引（2026-05-20 留）

> 上一个 session 跑完 5 个 task（项目记忆 / 搜索 / 技能 / 待办队列 / 登录）+ 部分日程，上下文将满，重启续跑。

## 第一步：验证新 CLI 装上了

```bash
ls -la $(which tauri-pilot)
# mtime 应 > 2026-05-18 15:48
tauri-pilot aijia --help | grep -E 'login|goto|handle-dialog|tool-calls|tool-bubble|agenda-|employee-'
# 应该列出 ~10 个新命令
```

如果还是老版（16 命令），让用户去 tauri-pilot 仓库 `cargo install --path crates/tauri-pilot-cli --force`。

## 第二步：dev server 状态

跑测前确认：
```bash
ls /tmp/tauri-pilot-com.aijia.app.sock        # 在 → 应用还活着
tauri-pilot aijia health-check --json         # readyState: complete
```

如果不在，重启：
```bash
rm -f /tmp/tauri-pilot-com.aijia.app.sock
pnpm dev:with-pilot > /tmp/aijia-tauri-dev.log 2>&1 &
# 等 socket：for i in $(seq 1 80); do [ -S /tmp/tauri-pilot-com.aijia.app.sock ] && break; sleep 3; done
```

注意 `src-tauri/build.rs` 已修死循环（未 commit），重启不会再死循环。

## 已跑过的（不要重跑，除非用户说有 fix）

详见 `docs/test-intents/run-reports/2026-05-20.md`：

| Task | 状态 | 已知 FAIL（可等修复后回头复测） |
|---|---|---|
| 日程 | 1/7 跑过，6 条未跑 | rules.md 大面积漂移，需要 author 先重写 |
| 项目记忆 | 4/4 跑过 | ReadFile 路径 bug、同名覆盖 hash bug、dynamic context 注入未生效 |
| 搜索 | 2/3 跑过 | 命名分歧 web_search/WebSearch；意图 2 SKIPPED 待无登录环境 |
| 技能 | 3/3 跑过 ✅ | 仅命名分歧 load_skill/Skill |
| 待办队列 | 0/3 触发到 pending | 需要确认 pending 触发条件 |
| 登录 | 仅意图 4 被动验证 ✅ | 1/2/3/5 等新 CLI |

## 建议下一个跑测顺序（用新 CLI）

按"产出价值高、CLI 现成够用"排序：

1. **登录 task 意图 1/2/3/5** — `aijia login` 已实现，4 条意图能直接跑通
2. **日程 task** — 但**先**用 `test-intents-author` skill 重写 rules.md（按 UI 实际"定时任务"形态，参考报告 Task 1 列出的 8 处偏差），再用 `aijia agenda-new / agenda-run-now / agenda-toggle` 跑
3. **数字员工 task** — `aijia employee-dispatch` 已实现，rules 没看过先快速读一遍判断 UI 复杂度
4. **专家团队 task** — 多员工协作，依赖数字员工 task 通过
5. **崩溃恢复** — 用户授权过才能跑（每条都要 kill -9）；CLI 用 `aijia restart-app` + 现有命令应该够
6. **工作空间 task** — `aijia select-workspace` 上次说没实现，看新 CLI help 是不是补了
7. **人格 task** — CLAUDE.md 提示已废弃（persona deprecation 2026-05-10），跑前先和用户确认意义

## 重启复测候选（如用户说产品有 fix）

只有这几条值得专门重跑验证：

| 之前 FAIL | 验证点 |
|---|---|
| 项目记忆-2 ReadFile 缺 bucket 前缀 | 跑同样 query，看 ReadFile 是否成功 |
| 项目记忆-3 dynamic context 注入 | 写完 memory 下一轮 AI 是否不再调 SearchMemory |
| 项目记忆-4 同名覆盖 | 两次同 name 不同 desc 看 entries 数是否还增长 |
| 待办队列 | 用户给出 pending 触发的稳定方法后重跑 |

## 已知系统级状态（短记）

- scope: `t_28__u_54`（pzc / 18267316753 / pzctest 租户）
- 当前 dev binary：v0.5.26 dev / feature/e2e-toolchain 分支
- 4 个仓库内文件未 commit：`src-tauri/build.rs`（死循环修复）、`docs/test-intents/{cli-gap.md,skill-suggestions.md,run-reports/2026-05-20.md}`、`.claude/skills/test-intents-runner/SKILL.md`（§5.12 沉淀）
- 项目记忆 entries 增长 5 → 8（保留有效）
- 多个跑测 conv 留在 `~/.renlijia/users/t_28__u_54/conversations/`（用户决定归档或保留）

## skill-suggestions 待用户审核

`docs/test-intents/skill-suggestions.md` 6 条候选项中：
- 候选 1（重启自动恢复）✅ 推荐沉
- 候选 2（ui-message 过滤）⚠️ 不沉、留 cli-gap
- 候选 3（kill aijia 一并杀 cargo）✅ 推荐沉
- 候选 4 已沉为 §5.12
- 候选 5（session/conv id 命名混乱）⚠️ 不沉、是对齐工作
- 候选 6（LLM 反问截断）✅ 推荐沉

下一个 session 接到用户审核结果后，把 ✅ 的迁到 runner skill。

## 工作风格备注（用户偏好）

1. **报告落盘**：用户要求所有 task 跑完都写到 `docs/test-intents/run-reports/YYYY-MM-DD.md`（违反 runner skill §3 不持久化原则，但用户优先）
2. **skill 不能擅自改**：所有要进 runner skill 的经验先写到 `skill-suggestions.md` 待审
3. **CLI 缺失**：写到 `cli-gap.md`（不是新 skill 经验）
4. **铁则**：e2e 必须走 `aijia` 子命令，遇到没包装的不绕过、记下来等 CLI 升级
5. **简洁回复**：用户上下文紧张，回复尽量短；表格/清单优先于段落
