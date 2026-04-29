use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize, Serialize)]
pub struct OpenAIRequest {
    pub model: String,
    pub messages: Vec<OpenAIMessage>,
    #[serde(default)]
    pub stream: bool,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub top_p: Option<f64>,
    pub tools: Option<Vec<Value>>,
    pub tool_choice: Option<Value>,
    pub stream_options: Option<Value>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct OpenAIMessage {
    pub role: String,
    pub content: Option<OpenAIContent>,
    pub tool_calls: Option<Vec<OpenAIToolCall>>,
    pub tool_call_id: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum OpenAIContent {
    Text(String),
    Parts(Vec<OpenAIContentPart>),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum OpenAIContentPart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    ImageUrl { image_url: ImageUrl },
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ImageUrl {
    pub url: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OpenAIToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: Option<String>,
    pub function: OpenAIFunctionCall,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OpenAIFunctionCall {
    pub name: String,
    pub arguments: String,
}
