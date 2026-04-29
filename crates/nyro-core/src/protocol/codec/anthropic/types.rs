use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize, Serialize)]
pub struct AnthropicRequest {
    pub model: String,
    pub messages: Vec<AnthropicMessage>,
    pub max_tokens: u32,
    #[serde(default)]
    pub stream: bool,
    pub system: Option<AnthropicSystem>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub tools: Option<Vec<AnthropicToolDef>>,
    pub tool_choice: Option<Value>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum AnthropicSystem {
    Text(String),
    Blocks(Vec<AnthropicContentBlock>),
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AnthropicMessage {
    pub role: String,
    pub content: AnthropicContent,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum AnthropicContent {
    Text(String),
    Blocks(Vec<AnthropicContentBlock>),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum AnthropicContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "thinking")]
    Thinking {
        thinking: String,
        #[serde(default)]
        signature: Option<String>,
    },
    #[serde(rename = "image")]
    Image { source: AnthropicImageSource },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: Option<Value>,
    },
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AnthropicImageSource {
    #[serde(rename = "type")]
    pub source_type: String,
    pub media_type: String,
    pub data: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AnthropicToolDef {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Value,
}
