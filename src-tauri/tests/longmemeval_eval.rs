//! LongMemEval 端到端上下文压缩评测（A 期：longmemeval_s + 强制触发）。
//!
//! 设计文档：docs/superpowers/specs/2026-06-04-context-compaction-eval-design.md
//!
//! 这是一个 `#[ignore]` 集成测试：需要真实联网 + 本地 `~/.renlijia` 登录态 +
//! 真实计费，因此排除在常规 CI 之外，必须显式运行：
//!
//! ```text
//! # 跑前请先在 app 内登录一次，确保 JWT 新鲜
//! set LME_LIMIT=3
//! cargo test --test longmemeval_eval -- --ignored --nocapture
//! ```
//!
//! 环境变量：
//! - `LME_DATA`  数据集 json 路径（默认指向本机 longmemeval_s_cleaned.json）
//! - `LME_LIMIT` 只跑前 N 条（默认 3，冒烟；设大跑全量）
//! - `LME_OUT`   输出目录（默认数据集同目录）

use std::sync::Arc;

use app_lib::auth::AuthManager;
use app_lib::llm::compact_summary_client::COMPACT_SYSTEM_PROMPT;
use app_lib::llm::gateway::LlmGateway;
use app_lib::llm::masking::MaskingLevel;
use app_lib::llm::streaming::ChatMessage;
use app_lib::models::settings::AppSettings;
use app_lib::runtime::chat::compaction::AutoCompactState;
use app_lib::runtime::chat::preprocess::{
    prepare_messages_for_llm, PreprocessConfig, PreprocessRuntimeState, PreprocessTrigger,
};
use app_lib::runtime::chat::turn_config::TurnError;
use app_lib::runtime::RuntimeRunRegistry;
use app_lib::storage::crypto::SecureStorage;
use app_lib::storage::file_store::AppStorage;
use app_lib::storage::{AiJiaHome, GlobalConfigStore};

use futures::stream::{self, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};

// 摘要 system prompt 直接复用生产真身：app_lib::llm::compact_summary_client::COMPACT_SYSTEM_PROMPT
// （见顶部 import）。单一真相源，避免副本漂移——评测测的就是生产线上的压缩行为。

// ---------------------------------------------------------------------------
// 数据模型（LongMemEval 实例）
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct Turn {
    role: String,
    // content 在数据集中偶尔不是字符串（如整数），用 Value 容错。
    content: Value,
    #[allow(dead_code)]
    #[serde(default)]
    has_answer: Option<bool>,
}

/// 把任意 JSON 值取成纯文本（字符串原样，其余 to_string）。
fn value_to_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[derive(Debug, Deserialize)]
struct Instance {
    question_id: String,
    question_type: String,
    // question / answer 在数据集中偶尔是非字符串（如整数），用 Value 容错。
    question: Value,
    answer: Value,
    #[serde(default)]
    question_date: String,
    #[serde(default)]
    haystack_dates: Vec<String>,
    haystack_sessions: Vec<Vec<Turn>>,
}

impl Instance {
    fn is_abstention(&self) -> bool {
        self.question_id.ends_with("_abs")
    }
    fn q_text(&self) -> String {
        value_to_text(&self.question)
    }
    fn a_text(&self) -> String {
        value_to_text(&self.answer)
    }
}

// ---------------------------------------------------------------------------
// 网关重建（headless，复刻 lib.rs startup 最小装配链）
// ---------------------------------------------------------------------------

async fn build_gateway() -> (Arc<LlmGateway>, Arc<AppSettings>) {
    let home = AiJiaHome::from_home();

    let secure_storage = SecureStorage::new(&home.crypto_dir())
        .ok()
        .map(Arc::new);
    let global_store = Arc::new(GlobalConfigStore::new(home.global_dir()));

    let auth_manager = Arc::new(AuthManager::new(
        global_store,
        secure_storage,
        &home,
    ));
    // 从本地 ~/.renlijia 读持久化的 JWT
    auth_manager.restore().await;

    let info = auth_manager.get_auth_info().await;
    assert!(
        info.logged_in,
        "未检测到登录态：请先在 AIjia app 内登录一次，确保 ~/.renlijia 下有有效 JWT。"
    );

    // 网关的 db 仅用于内部记账，用临时目录即可（session_key 来自 auth_manager）。
    let tmp = std::env::temp_dir().join(format!("lme-eval-db-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).expect("create temp db dir");
    let db = Arc::new(AppStorage::new(&tmp).expect("AppStorage::new"));

    let run_registry = Arc::new(RuntimeRunRegistry::new());
    let gateway = Arc::new(
        LlmGateway::new_with_registry(db, run_registry).with_auth_manager(auth_manager),
    );

    let settings = Arc::new(AppSettings::default());
    (gateway, settings)
}

// 简单封装一次「发一条提示拿文本」的网关调用。
async fn ask(
    gateway: &LlmGateway,
    settings: &AppSettings,
    system_prompt: Option<&str>,
    messages: Vec<ChatMessage>,
) -> Result<String, String> {
    gateway
        .send_message(
            settings,
            messages,
            MaskingLevel::Relaxed,
            system_prompt,
            None,
            None,
        )
        .await
        .map(|resp| resp.content)
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// 历史转换
// ---------------------------------------------------------------------------

/// haystack_sessions → 结构化 messages [{role, content}]，并在每个 session 的
/// 首轮注入时间戳（temporal-reasoning 题需要）。
fn sessions_to_messages(inst: &Instance) -> Vec<Value> {
    let mut out = Vec::new();
    for (si, session) in inst.haystack_sessions.iter().enumerate() {
        let date = inst.haystack_dates.get(si).cloned().unwrap_or_default();
        for (ti, turn) in session.iter().enumerate() {
            let text = value_to_text(&turn.content);
            let content = if ti == 0 && !date.is_empty() {
                format!("(对话时间：{})\n{}", date, text)
            } else {
                text
            };
            out.push(json!({ "role": turn.role, "content": content }));
        }
    }
    out
}

fn msg_role(v: &Value) -> &str {
    v.get("role").and_then(|r| r.as_str()).unwrap_or("user")
}

/// 从一个消息 Value 中抽取纯文本（兼容 str / {text} / blocks）。
fn msg_text(v: &Value) -> String {
    let Some(content) = v.get("content") else {
        return String::new();
    };
    if let Some(s) = content.as_str() {
        return s.to_string();
    }
    if let Some(s) = content.get("text").and_then(|t| t.as_str()) {
        return s.to_string();
    }
    if let Some(blocks) = content.as_array() {
        return blocks
            .iter()
            .filter_map(|b| {
                (b.get("type").and_then(|t| t.as_str()) == Some("text"))
                    .then(|| b.get("text").and_then(|t| t.as_str()))
                    .flatten()
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    String::new()
}

/// 把结构化 messages 还原成真实多轮对话（ChatMessage），并合并相邻同角色
/// （对齐 Anthropic 的 user/assistant 交替约束）。role 归一：assistant 保留，
/// 其余（system/tool/user）一律按 user 处理。最后追加问题作为末轮 user。
fn build_answer_messages(history: &[Value], question: &str, question_date: &str) -> Vec<ChatMessage> {
    let mut pairs: Vec<(String, String)> = Vec::new();
    for v in history {
        let role = if msg_role(v) == "assistant" {
            "assistant"
        } else {
            "user"
        };
        let text = msg_text(v);
        if text.trim().is_empty() {
            continue;
        }
        if let Some(last) = pairs.last_mut() {
            if last.0 == role {
                last.1.push('\n');
                last.1.push_str(&text);
                continue;
            }
        }
        pairs.push((role.to_string(), text));
    }

    // 追加问题（带提问时间）。若末轮已是 user 则并入。
    let q = if question_date.is_empty() {
        question.to_string()
    } else {
        format!("(当前时间：{})\n{}", question_date, question)
    };
    if let Some(last) = pairs.last_mut() {
        if last.0 == "user" {
            last.1.push_str("\n\n");
            last.1.push_str(&q);
        } else {
            pairs.push(("user".to_string(), q));
        }
    } else {
        pairs.push(("user".to_string(), q));
    }

    pairs
        .into_iter()
        .map(|(role, text)| ChatMessage::text(&role, text))
        .collect()
}

const ANSWER_SYSTEM_PROMPT: &str = "你是一个助手。请**仅根据**下面对话历史中的信息回答用户的问题。\
如果历史中没有足够信息来回答，请明确说明你不知道 / 历史中没有相关信息，不要编造。回答简洁。";

fn estimate_total_chars(messages: &[Value]) -> usize {
    messages.iter().map(|m| m.to_string().len()).sum()
}

fn truncate(s: &str, n: usize) -> String {
    let t: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        format!("{}…", t)
    } else {
        t
    }
}

// ---------------------------------------------------------------------------
// 判分（移植自 LongMemEval evaluate_qa.py::get_anscheck_prompt）
// ---------------------------------------------------------------------------

fn anscheck_prompt(qtype: &str, question: &str, answer: &str, response: &str, abstention: bool) -> String {
    if abstention {
        return format!(
            "I will give you an unanswerable question, an explanation, and a response from a model. Please answer yes if the model correctly identifies the question as unanswerable. The model could say that the information is incomplete, or some other information is given but the asked information is not.\n\nQuestion: {q}\n\nExplanation: {a}\n\nModel Response: {r}\n\nDoes the model correctly identify the question as unanswerable? Answer yes or no only.",
            q = question, a = answer, r = response
        );
    }
    match qtype {
        "single-session-user" | "single-session-assistant" | "multi-session" => format!(
            "I will give you a question, a correct answer, and a response from a model. Please answer yes if the response contains the correct answer. Otherwise, answer no. If the response is equivalent to the correct answer or contains all the intermediate steps to get the correct answer, you should also answer yes. If the response only contains a subset of the information required by the answer, answer no. \n\nQuestion: {q}\n\nCorrect Answer: {a}\n\nModel Response: {r}\n\nIs the model response correct? Answer yes or no only.",
            q = question, a = answer, r = response
        ),
        "temporal-reasoning" => format!(
            "I will give you a question, a correct answer, and a response from a model. Please answer yes if the response contains the correct answer. Otherwise, answer no. If the response is equivalent to the correct answer or contains all the intermediate steps to get the correct answer, you should also answer yes. If the response only contains a subset of the information required by the answer, answer no. In addition, do not penalize off-by-one errors for the number of days. If the question asks for the number of days/weeks/months, etc., and the model makes off-by-one errors (e.g., predicting 19 days when the answer is 18), the model's response is still correct. \n\nQuestion: {q}\n\nCorrect Answer: {a}\n\nModel Response: {r}\n\nIs the model response correct? Answer yes or no only.",
            q = question, a = answer, r = response
        ),
        "knowledge-update" => format!(
            "I will give you a question, a correct answer, and a response from a model. Please answer yes if the response contains the correct answer. Otherwise, answer no. If the response contains some previous information along with an updated answer, the response should be considered as correct as long as the updated answer is the required answer.\n\nQuestion: {q}\n\nCorrect Answer: {a}\n\nModel Response: {r}\n\nIs the model response correct? Answer yes or no only.",
            q = question, a = answer, r = response
        ),
        "single-session-preference" => format!(
            "I will give you a question, a rubric for desired personalized response, and a response from a model. Please answer yes if the response satisfies the desired response. Otherwise, answer no. The model does not need to reflect all the points in the rubric. The response is correct as long as it recalls and utilizes the user's personal information correctly.\n\nQuestion: {q}\n\nRubric: {a}\n\nModel Response: {r}\n\nIs the model response correct? Answer yes or no only.",
            q = question, a = answer, r = response
        ),
        _ => format!(
            "I will give you a question, a correct answer, and a response from a model. Please answer yes if the response contains the correct answer. Otherwise, answer no.\n\nQuestion: {q}\n\nCorrect Answer: {a}\n\nModel Response: {r}\n\nIs the model response correct? Answer yes or no only.",
            q = question, a = answer, r = response
        ),
    }
}

async fn judge(
    gateway: &LlmGateway,
    settings: &AppSettings,
    inst: &Instance,
    response: &str,
) -> bool {
    let prompt = anscheck_prompt(
        &inst.question_type,
        &inst.q_text(),
        &inst.a_text(),
        response,
        inst.is_abstention(),
    );
    match ask(gateway, settings, None, vec![ChatMessage::text("user", prompt)]).await {
        Ok(verdict) => verdict.to_lowercase().contains("yes"),
        Err(e) => {
            eprintln!("[judge] 失败（按 no 计）：{}", e);
            false
        }
    }
}

// ---------------------------------------------------------------------------
// 分数聚合
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Scoreboard {
    // qtype -> (correct, total)
    by_type: std::collections::BTreeMap<String, (usize, usize)>,
    correct: usize,
    total: usize,
}

impl Scoreboard {
    fn record(&mut self, qtype: &str, ok: bool) {
        let e = self.by_type.entry(qtype.to_string()).or_insert((0, 0));
        e.1 += 1;
        self.total += 1;
        if ok {
            e.0 += 1;
            self.correct += 1;
        }
    }

    fn acc(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.correct as f64 / self.total as f64
        }
    }

    fn print(&self, title: &str) {
        println!("\n===== {} =====", title);
        println!("总正确率: {:.4} ({}/{})", self.acc(), self.correct, self.total);
        for (qtype, (c, t)) in &self.by_type {
            let a = if *t == 0 { 0.0 } else { *c as f64 / *t as f64 };
            println!("  {:<28} {:.4} ({}/{})", qtype, a, c, t);
        }
    }
}

fn mark(b: bool) -> &'static str {
    if b {
        "✓"
    } else {
        "✗"
    }
}

fn save_pct(pre: usize, post: usize) -> f64 {
    if pre == 0 {
        0.0
    } else {
        (1.0 - post as f64 / pre as f64) * 100.0
    }
}

/// 单条样本的评测产出（在并发任务里计算，事后统一聚合）。
struct Outcome {
    qtype: String,
    ok_full: bool,
    /// 压缩后结果：Some((是否答对, 压缩后字符数))；None = 压缩/答题基础设施失败（不计入 B 组）。
    comp: Option<(bool, usize)>,
    pre: usize,
    detail: Value,
}

/// 评测单条样本：A 组（全量答题）+ B 组（真实压缩后答题）。
///
/// 摘要走 `send_message` + 生产 `COMPACT_SYSTEM_PROMPT`（顶部 import，单一真相源）。
/// 不走 `LlmCompactSummaryClient`，因其内部强制 v2 网关，在 headless 测试环境会空返回。
async fn eval_one(gw: &LlmGateway, st: &AppSettings, inst: &Instance, idx: usize, n: usize) -> Option<Outcome> {
    let history = sessions_to_messages(inst);
    let pre = estimate_total_chars(&history);

    // -------- A 组：全量历史（不压缩，天花板）--------
    let full_msgs = build_answer_messages(&history, &inst.q_text(), &inst.question_date);
    let hyp_full = match ask(gw, st, Some(ANSWER_SYSTEM_PROMPT), full_msgs).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[{}] full 答题失败，跳过：{}", inst.question_id, e);
            return None;
        }
    };
    let ok_full = judge(gw, st, inst, &hyp_full).await;

    // -------- B 组：真实压缩后答题（压缩失败不影响已记录的 A 组）--------
    let mut compact_state = AutoCompactState::new();
    let mut runtime_state = PreprocessRuntimeState::default();
    let config = PreprocessConfig::default();
    let summary_fn = move |msgs: Vec<Value>| async move {
        let transcript = msgs
            .iter()
            .map(|m| format!("{}: {}", msg_role(m), msg_text(m)))
            .collect::<Vec<_>>()
            .join("\n");
        gw.send_message(
            st,
            vec![ChatMessage::text("user", transcript)],
            MaskingLevel::Relaxed,
            Some(COMPACT_SYSTEM_PROMPT), // 生产真身（pub），改生产 prompt 自动生效
            None,
            None,
        )
        .await
        .map(|r| r.content)
        .map_err(|e| TurnError::LlmError(e.to_string()))
    };

    let mut comp: Option<(bool, usize)> = None;
    let mut hyp_comp = String::new();
    let mut summary_text = String::new();
    let mut executed_stages = String::new();
    match prepare_messages_for_llm(
        history.clone(),
        &inst.question_id,
        PreprocessTrigger::ManualCompact,
        &config,
        &mut compact_state,
        &mut runtime_state,
        false,
        summary_fn,
    )
    .await
    {
        Ok(prepared) => {
            executed_stages = format!("{:?}", prepared.executed_stages);
            let compressed = prepared.messages;
            let post = estimate_total_chars(&compressed);
            summary_text = compressed
                .iter()
                .find(|m| m.get("isCompactSummary").and_then(|v| v.as_bool()) == Some(true))
                .map(msg_text)
                .unwrap_or_default();
            let comp_msgs = build_answer_messages(&compressed, &inst.q_text(), &inst.question_date);
            match ask(gw, st, Some(ANSWER_SYSTEM_PROMPT), comp_msgs).await {
                Ok(s) => {
                    hyp_comp = s;
                    let ok_comp = judge(gw, st, inst, &hyp_comp).await;
                    comp = Some((ok_comp, post));
                }
                Err(e) => eprintln!("[{}] compressed 答题失败：{}", inst.question_id, e),
            }
        }
        Err(e) => eprintln!("[{}] 压缩失败：{:?}", inst.question_id, e),
    }

    let comp_mark = match comp {
        Some((ok, _)) => mark(ok),
        None => "—",
    };
    println!(
        "[{}/{}] {} ({}) | A:{} B:{} | stages={}",
        idx + 1,
        n,
        inst.question_id,
        inst.question_type,
        mark(ok_full),
        comp_mark,
        executed_stages
    );

    let detail = json!({
        "question_id": inst.question_id,
        "question_type": inst.question_type,
        "is_abstention": inst.is_abstention(),
        "question": inst.q_text(),
        "gold_answer": inst.a_text(),
        "hyp_full": hyp_full,
        "ok_full": ok_full,
        "hyp_compressed": hyp_comp,
        "ok_compressed": comp.map(|(ok, _)| ok),
        "summary_text": summary_text,
        "executed_stages": executed_stages,
        "pre_chars": pre,
        "post_chars": comp.map(|(_, p)| p),
    });

    Some(Outcome {
        qtype: inst.question_type.clone(),
        ok_full,
        comp,
        pre,
        detail,
    })
}

// ---------------------------------------------------------------------------
// 主测试
// ---------------------------------------------------------------------------

fn default_data_path() -> String {
    r"C:\Users\Administrator\Desktop\github\LongMemEval\data\longmemeval_s_cleaned.json".to_string()
}

#[tokio::test]
#[ignore = "需要真实网关 + 本地登录态 + 计费，手动运行"]
async fn longmemeval_compaction_eval() {
    let data_path = std::env::var("LME_DATA").unwrap_or_else(|_| default_data_path());
    let limit: usize = std::env::var("LME_LIMIT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3);
    // 均衡采样：每个 question_type 取前 N 条。设置后覆盖 LME_LIMIT 的取前 N 逻辑。
    let per_type: Option<usize> = std::env::var("LME_PER_TYPE")
        .ok()
        .and_then(|s| s.parse().ok());

    let raw = std::fs::read_to_string(&data_path)
        .unwrap_or_else(|e| panic!("读取数据集失败 {}: {}", data_path, e));
    let instances: Vec<Instance> = serde_json::from_str(&raw).expect("解析 LongMemEval json");

    // 构建本次评测的样本：均衡模式按类各取 per_type 条；否则取前 limit 条。
    let selected: Vec<&Instance> = if let Some(k) = per_type {
        let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        let mut picked = Vec::new();
        for inst in &instances {
            let c = counts.entry(inst.question_type.clone()).or_insert(0);
            if *c < k {
                picked.push(inst);
                *c += 1;
            }
        }
        picked
    } else {
        instances.iter().take(limit).collect()
    };
    let n = selected.len();
    println!(
        "加载 {} 条实例，本次评测 {} 条（{}）。数据集: {}",
        instances.len(),
        n,
        match per_type {
            Some(k) => format!("均衡：每类 {} 条", k),
            None => format!("取前 {} 条", limit.min(instances.len())),
        },
        data_path
    );

    let (gateway, settings) = build_gateway().await;

    // 并发度：默认 5，可用 LME_CONCURRENCY 调。并发只影响速度，不影响结果（每条样本独立）。
    let concurrency: usize = std::env::var("LME_CONCURRENCY")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5)
        .max(1);
    println!("并发度: {}（每条样本独立，仅影响速度）", concurrency);

    let outcomes: Vec<Outcome> = stream::iter(selected.iter().copied().enumerate())
        .map(|(idx, inst)| {
            let gw = gateway.as_ref();
            let st = settings.as_ref();
            async move { eval_one(gw, st, inst, idx, n).await }
        })
        .buffer_unordered(concurrency)
        .filter_map(|o| async move { o })
        .collect()
        .await;

    // 聚合（并发跑完后统一计分，结果与串行一致）。
    let mut board_full = Scoreboard::default();
    let mut board_comp = Scoreboard::default();
    let mut total_pre = 0usize;
    let mut total_post = 0usize;
    let mut details: Vec<Value> = Vec::with_capacity(outcomes.len());
    for o in &outcomes {
        board_full.record(&o.qtype, o.ok_full);
        // 仅当压缩成功时计入 B 组与压缩比（基础设施失败不污染分数）。
        if let Some((ok_comp, post)) = o.comp {
            board_comp.record(&o.qtype, ok_comp);
            total_pre += o.pre;
            total_post += post;
        }
        details.push(o.detail.clone());
    }

    // 写出逐条明细 jsonl，供离线核验裁判。
    let out_dir = std::env::var("LME_OUT").unwrap_or_else(|_| {
        std::path::Path::new(&data_path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string())
    });
    let out_path = std::path::Path::new(&out_dir).join("lme_eval_details.jsonl");
    let body = details
        .iter()
        .map(|d| d.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    if let Err(e) = std::fs::write(&out_path, body) {
        eprintln!("写明细失败: {}", e);
    } else {
        println!("\n逐条明细已写入: {}", out_path.display());
    }

    board_full.print("A 组：不压缩（天花板）");
    board_comp.print("B 组：压缩后（生产 prompt）");

    if total_pre > 0 {
        println!(
            "\n平均压缩比: {:.1}% (累计 {} -> {} 字符)",
            (1.0 - total_post as f64 / total_pre as f64) * 100.0,
            total_pre,
            total_post
        );
    }
    let dmg = (board_full.acc() - board_comp.acc()) * 100.0;
    println!(
        "\n压缩损伤 = A − B = {:.1}pp（A {:.1}% → B {:.1}%）。重点看 knowledge-update / temporal-reasoning。",
        dmg,
        board_full.acc() * 100.0,
        board_comp.acc() * 100.0
    );
}
