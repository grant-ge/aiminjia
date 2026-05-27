use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AijiaResponseRequest {
    pub schema_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    pub intent: String,
    pub stream: bool,
    pub model_policy: ModelPolicy,
    pub context: CanonicalContext,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolDefinition>,
    pub options: GenerationOptions,
    pub client: ClientInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPolicy {
    pub mode: String,
    pub logical_model: String,
    pub allowed_capabilities: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_affinity: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalContext {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub system: Vec<SystemSegment>,
    pub messages: Vec<CanonicalMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemSegment {
    #[serde(rename = "type")]
    pub kind: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalMessage {
    pub role: String,
    pub content: Vec<ContentBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub is_error: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentBlock {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opaque: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationOptions {
    pub max_output_tokens: u32,
    pub temperature: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
    pub platform: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_schema_version() {
        let req = AijiaResponseRequest {
            schema_version: "aijia.ai.response.v1".to_string(),
            conversation_id: Some("conv".to_string()),
            run_id: None,
            trace_id: None,
            intent: "chat".to_string(),
            stream: true,
            model_policy: ModelPolicy {
                mode: "auto".to_string(),
                logical_model: "default-chat".to_string(),
                allowed_capabilities: vec!["text".to_string()],
                reasoning: Some("medium".to_string()),
                provider_affinity: None,
            },
            context: CanonicalContext {
                system: vec![],
                messages: vec![CanonicalMessage {
                    role: "user".to_string(),
                    content: vec![ContentBlock {
                        kind: "text".to_string(),
                        text: Some("hi".to_string()),
                        mime_type: None,
                        data: None,
                        url: None,
                        id: None,
                        name: None,
                        arguments: None,
                        signature: None,
                        opaque: None,
                        source: None,
                    }],
                    tool_call_id: None,
                    tool_name: None,
                    is_error: false,
                    provider: None,
                    usage: None,
                    stop_reason: None,
                    created_at: None,
                }],
            },
            tools: vec![],
            options: GenerationOptions {
                max_output_tokens: 1024,
                temperature: 0.7,
            },
            client: ClientInfo {
                name: "aijia-desktop".to_string(),
                version: "test".to_string(),
                platform: "test".to_string(),
            },
        };

        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("aijia.ai.response.v1"));
        assert!(json.contains("default-chat"));
    }
}
