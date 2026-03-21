use serde::{Deserialize, Serialize};

// ── New internal-app types ──────────────────────────────────────────

/// Internal app info from Lotus API (snake_case JSON from Go backend).
/// Also serialized to frontend via Tauri IPC in snake_case.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InternalAppInfo {
    pub id: u64,
    pub name: String,
    pub description: String,
    pub icon_url: Option<String>,
    pub base_url: String,
    pub login_url: String,
    pub success_url_prefix: String,
    // Go stores these as JSON text strings; deserialize flexibly
    #[serde(deserialize_with = "deserialize_string_or_vec", default)]
    pub allowed_methods: Vec<String>,
    #[serde(deserialize_with = "deserialize_string_or_vec", default)]
    pub blocked_paths: Vec<String>,
    pub max_requests_per_turn: u32,
    #[serde(deserialize_with = "deserialize_string_or_hints", default)]
    pub hints: Vec<AppHint>,
    pub status: String,
    #[serde(default)]
    pub connected: bool,
    #[serde(default)]
    pub conn_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppHint {
    pub name: String,
    pub description: String,
    pub clues: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKnowledgeItem {
    pub id: u64,
    pub name: String,
    pub method: String,
    pub path: String,
    pub params_doc: Option<String>,
    pub response_doc: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InternalAppConfig {
    pub apps: Vec<InternalAppInfo>,
    pub last_synced: Option<String>,
}

// ── Custom deserializers for Go JSON-text fields ─────────────────────

/// Deserializes either a JSON array or a JSON-encoded string containing an array.
/// Go stores `allowed_methods` as `text` with value like `["GET","POST"]`.
fn deserialize_string_or_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;

    struct StringOrVec;
    impl<'de> de::Visitor<'de> for StringOrVec {
        type Value = Vec<String>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a JSON array or a string containing a JSON array")
        }
        fn visit_str<E: de::Error>(self, s: &str) -> Result<Vec<String>, E> {
            serde_json::from_str(s).map_err(de::Error::custom)
        }
        fn visit_seq<A: de::SeqAccess<'de>>(self, seq: A) -> Result<Vec<String>, A::Error> {
            Deserialize::deserialize(de::value::SeqAccessDeserializer::new(seq))
        }
    }
    deserializer.deserialize_any(StringOrVec)
}

/// Deserializes hints from either a JSON array of AppHint or a JSON-encoded string.
fn deserialize_string_or_hints<'de, D>(deserializer: D) -> Result<Vec<AppHint>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;

    struct StringOrHints;
    impl<'de> de::Visitor<'de> for StringOrHints {
        type Value = Vec<AppHint>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a JSON array of hints or a string containing a JSON array")
        }
        fn visit_str<E: de::Error>(self, s: &str) -> Result<Vec<AppHint>, E> {
            serde_json::from_str(s).map_err(de::Error::custom)
        }
        fn visit_seq<A: de::SeqAccess<'de>>(self, seq: A) -> Result<Vec<AppHint>, A::Error> {
            Deserialize::deserialize(de::value::SeqAccessDeserializer::new(seq))
        }
    }
    deserializer.deserialize_any(StringOrHints)
}
