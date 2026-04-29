use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize, Serialize)]
pub struct GeminiRequest {
    pub contents: Vec<GeminiContent>,
    #[serde(rename = "systemInstruction")]
    pub system_instruction: Option<GeminiContent>,
    #[serde(rename = "generationConfig")]
    pub generation_config: Option<GeminiGenerationConfig>,
    pub tools: Option<Vec<GeminiTool>>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GeminiContent {
    pub role: Option<String>,
    pub parts: Vec<GeminiPart>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum GeminiPart {
    Text {
        text: String,
    },
    InlineData {
        #[serde(rename = "inlineData")]
        inline_data: GeminiInlineData,
    },
    FunctionCall {
        #[serde(rename = "functionCall")]
        function_call: GeminiFunctionCall,
    },
    FunctionResponse {
        #[serde(rename = "functionResponse")]
        function_response: GeminiFunctionResponse,
    },
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GeminiInlineData {
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    pub data: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GeminiFunctionCall {
    pub name: String,
    pub args: Value,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GeminiFunctionResponse {
    pub name: String,
    pub response: Value,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GeminiGenerationConfig {
    pub temperature: Option<f64>,
    #[serde(rename = "maxOutputTokens")]
    pub max_output_tokens: Option<u32>,
    #[serde(rename = "topP")]
    pub top_p: Option<f64>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GeminiTool {
    #[serde(rename = "functionDeclarations")]
    pub function_declarations: Vec<GeminiFunctionDecl>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GeminiFunctionDecl {
    pub name: String,
    pub description: Option<String>,
    pub parameters: Option<Value>,
}
