use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize, Serialize)]
pub struct GoogleRequest {
    pub contents: Vec<GoogleContent>,
    #[serde(rename = "systemInstruction")]
    pub system_instruction: Option<GoogleContent>,
    #[serde(rename = "generationConfig")]
    pub generation_config: Option<GoogleGenerationConfig>,
    pub tools: Option<Vec<GoogleTool>>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GoogleContent {
    pub role: Option<String>,
    pub parts: Vec<GooglePart>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum GooglePart {
    Text {
        text: String,
    },
    InlineData {
        #[serde(rename = "inlineData")]
        inline_data: GoogleInlineData,
    },
    FunctionCall {
        #[serde(rename = "functionCall")]
        function_call: GoogleFunctionCall,
    },
    FunctionResponse {
        #[serde(rename = "functionResponse")]
        function_response: GoogleFunctionResponse,
    },
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GoogleInlineData {
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    pub data: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GoogleFunctionCall {
    pub name: String,
    pub args: Value,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GoogleFunctionResponse {
    pub name: String,
    pub response: Value,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GoogleGenerationConfig {
    pub temperature: Option<f64>,
    #[serde(rename = "maxOutputTokens")]
    pub max_output_tokens: Option<u32>,
    #[serde(rename = "topP")]
    pub top_p: Option<f64>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GoogleTool {
    #[serde(rename = "functionDeclarations")]
    pub function_declarations: Vec<GoogleFunctionDecl>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GoogleFunctionDecl {
    pub name: String,
    pub description: Option<String>,
    pub parameters: Option<Value>,
}
