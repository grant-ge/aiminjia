//! 群聊事件流模块。
//!
//! 把一个 conversation 目录下的 `messages.jsonl` + `teammates/*.meta.json` 解析
//! 成"群聊视图"消费的 `TeamEvent` 流。详见 [`parser`] 模块文档。
//!
//! 模块边界：本模块只**读**已有数据，不写、不改、不维持额外状态。所有的"群"
//! 概念在这里都是从 transcript 派生出的视图，conversation 是单一真相源。

pub mod parser;

pub use parser::{parse_team_view, MemberInfo, TeamEvent, TeamRoster, TeamView};
