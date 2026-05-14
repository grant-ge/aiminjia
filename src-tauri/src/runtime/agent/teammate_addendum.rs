//! TEAMMATE_ADDENDUM (Chinese): boot-time prompt fragment appended to a
//! Teammate's system prompt so the LLM understands its role inside the Team.
//!
//! Per v4 §3.B.1 the **Lead** does NOT get an addendum — collaboration
//! guidance for the Lead lives entirely in the tool descriptions.  Only
//! Teammates carry this block.
//!
//! Rendering substitutes two placeholders:
//! - `{team_name}` -> the Team's display name (from TeamRegistry / Team)
//! - `{teammate_name}` -> the Teammate's name (from AgentNameRegistry)

/// Raw addendum body with `{team_name}` / `{teammate_name}` placeholders.
pub const TEAMMATE_ADDENDUM_ZH: &str = r#"

## 你正在以 Teammate 身份运行

你不是独自工作。你属于 `{team_name}` 团队的一员，名字是 `{teammate_name}`。
当前团队还有一位 Lead（`team-lead`）与可能存在的其他 Teammate。

### 关键交付纪律（必读）

**团队中的任何人（Lead 与其他 Teammate）都看不到你的 assistant 文本输出**。你在 turn 里直接说的话只会留在你自己的 transcript 里，对外**完全不可见**。

任何你希望让团队中其他人看到的内容（立论、报告、回答、反驳、汇报、确认……）**必须**通过 `SendMessage` 工具送出，否则等于没说。

收到他人指令时的工作流：
1. 收到 user 消息（通常来自 Lead 或其他 Teammate 通过 SendMessage 转发）
2. 思考并准备答复
3. **调用 `SendMessage(to="...", message={"type":"text","content":"你的答复全文"})` 把答复显式发送给接收者**（通常是 `team-lead`，也可以是其他 teammate 名字）
4. 如不调 SendMessage，对方会一直等不到你的回应

### 与 Lead / 其他 Teammate 通信
- 用 `SendMessage(to=..., message={"type":"text","content":"..."})` 给具体名字的成员发消息。
- 给 Lead 发 → `to: "team-lead"`。
- 广播给所有 Teammate（不含 Lead）→ `to: "*"`。
- **不要**用 SendMessage 报告 task 进度；用 `TaskUpdate(status=in_progress|completed)`。

### 任务市场
- 你可以用 `TaskList()` 查看 Team 的共享任务清单。
- 看到 owner 为空 / "*" 的任务，如果适合你的能力，用 `TaskClaim(task_id)` 认领。
- 不要重复认领别人已经 owner 的 task。

### 优雅关闭
- 你可能收到 `shutdown_request`���`{"type":"shutdown_request","reason":"..."}`）。
- 你**必须**用 `SendMessage(to="team-lead", message={"type":"shutdown_response","request_id":"...","approve":<bool>,"reason":"..."})` 显式回应。
- 如果工作已收尾且无未保存状态，approve=true；否则 approve=false 并简述原因（Lead 可以 retry 或 TaskStop 强制关闭）。

### 协作纪律
- **不要**询问用户（Ask）。你是 async，任何 ask 会被自动 deny。
- 跨 turn 你的 conversation history 会被保留；但其他 Teammate 的 history 你看不见 — 该交换的信息**显式** SendMessage。
- 完成阶段性产出：`TaskUpdate(status=completed)` + `SendMessage(to=team-lead, message={"type":"text","content":"..."})` 通报。
"#;

/// Render the addendum with the team / teammate names substituted in.
///
/// Empty arguments are accepted — they will leave a literal `` `` (empty back-tick
/// pair) in the output, which is harmless but ugly.  Callers should pass
/// non-empty values whenever possible.
pub fn render(team_name: &str, teammate_name: &str) -> String {
    TEAMMATE_ADDENDUM_ZH
        .replace("{team_name}", team_name)
        .replace("{teammate_name}", teammate_name)
}

/// Compose a final boot system prompt by appending the addendum to the
/// Employee's existing `system_prompt_extra` (if any).  Returns just the
/// addendum when `base` is empty.  A single blank line separates the two so
/// the LLM sees a clean section break.
pub fn compose_boot_prompt(base: &str, team_name: &str, teammate_name: &str) -> String {
    let addendum = render(team_name, teammate_name);
    if base.trim().is_empty() {
        addendum
    } else {
        format!("{base}\n{addendum}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_substitutes_both_placeholders() {
        let out = render("研究小队", "researcher");
        assert!(out.contains("`研究小队`"), "team_name not substituted: {out}");
        assert!(out.contains("`researcher`"), "teammate_name not substituted: {out}");
        assert!(
            !out.contains("{team_name}") && !out.contains("{teammate_name}"),
            "raw placeholder leaked: {out}"
        );
    }

    #[test]
    fn render_contains_send_message_guidance() {
        let out = render("t", "n");
        assert!(out.contains("SendMessage"), "missing SendMessage section");
        assert!(out.contains("team-lead"), "missing team-lead routing hint");
        assert!(out.contains("shutdown_request"), "missing shutdown handshake");
        assert!(out.contains("TaskClaim"), "missing task market guidance");
    }

    #[test]
    fn compose_boot_prompt_appends_to_existing_base() {
        let composed = compose_boot_prompt("You are X.", "team-a", "alice");
        assert!(composed.starts_with("You are X."));
        assert!(composed.contains("Teammate 身份"));
    }

    #[test]
    fn compose_boot_prompt_handles_empty_base() {
        let composed = compose_boot_prompt("", "team-a", "alice");
        assert!(composed.trim_start().starts_with("## 你正在以 Teammate 身份"));
    }

    #[test]
    fn compose_boot_prompt_treats_whitespace_only_base_as_empty() {
        let composed = compose_boot_prompt("   \n  ", "team-a", "alice");
        // No prepended whitespace garbage — addendum starts at the top.
        assert!(composed.trim_start().starts_with("## 你正在以 Teammate"));
    }
}
