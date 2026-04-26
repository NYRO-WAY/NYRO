//! 4 core scenarios with per-protocol request body templates.
//!
//! Each `Scenario` carries:
//! - `name`        — kebab-case identifier appearing in `replay_model`
//! - `anchor`      — fixed token embedded in the user prompt; ALSO used by pytest as a substring assertion against the recorded response stream
//! - `stream`      — whether the request asks for streaming (drives endpoint selection for google-content)
//! - `bodies`      — per-protocol JSON body templates. `{{MODEL}}` is replaced by `record` at runtime
//! - `expected_fields` — field names pytest looks for inside the protocol-converted response (orthogonal to anchor)
//!
//! Body templates are JSON literals so `record` can do a single `serde_json::from_str` followed by model substitution.

pub const ANCHOR_BASIC_NONSTREAM: &str = "NYRO_PROBE_BASIC_NONSTREAM";
pub const ANCHOR_BASIC_STREAM: &str = "NYRO_PROBE_BASIC_STREAM";
pub const ANCHOR_TOOL_USE_STREAM: &str = "NYRO_PROBE_TOOL_USE_STREAM";
pub const ANCHOR_REASONING_STREAM: &str = "NYRO_PROBE_REASONING_STREAM";

/// Placeholder substituted with the real LLM model name during recording.
pub const MODEL_PLACEHOLDER: &str = "{{MODEL}}";

#[derive(Debug)]
pub struct Scenario {
    pub name: &'static str,
    pub anchor: &'static str,
    pub stream: bool,
    pub uses_reasoning_model: bool,
    pub bodies: ScenarioBodies,
    pub expected_fields: ScenarioExpectedFields,
}

#[derive(Debug)]
pub struct ScenarioBodies {
    pub openai_chat: Option<&'static str>,
    pub openai_responses: Option<&'static str>,
    pub anthropic_messages: Option<&'static str>,
    pub google_content: Option<&'static str>,
}

#[derive(Debug)]
pub struct ScenarioExpectedFields {
    pub openai_chat: &'static [&'static str],
    pub openai_responses: &'static [&'static str],
    pub anthropic_messages: &'static [&'static str],
    pub google_content: &'static [&'static str],
}

// ===========================================================================
// basic-nonstream
// ===========================================================================
const OPENAI_CHAT_BASIC_NONSTREAM: &str = r#"{
  "model": "{{MODEL}}",
  "stream": false,
  "messages": [
    {"role": "user", "content": "Reply with the literal token NYRO_PROBE_BASIC_NONSTREAM and nothing else."}
  ]
}"#;

const OPENAI_RESPONSES_BASIC_NONSTREAM: &str = r#"{
  "model": "{{MODEL}}",
  "stream": false,
  "input": "Reply with the literal token NYRO_PROBE_BASIC_NONSTREAM and nothing else."
}"#;

const ANTHROPIC_BASIC_NONSTREAM: &str = r#"{
  "model": "{{MODEL}}",
  "stream": false,
  "max_tokens": 256,
  "messages": [
    {"role": "user", "content": "Reply with the literal token NYRO_PROBE_BASIC_NONSTREAM and nothing else."}
  ]
}"#;

const GOOGLE_BASIC_NONSTREAM: &str = r#"{
  "contents": [
    {"role": "user", "parts": [{"text": "Reply with the literal token NYRO_PROBE_BASIC_NONSTREAM and nothing else."}]}
  ]
}"#;

// ===========================================================================
// basic-stream
// ===========================================================================
const OPENAI_CHAT_BASIC_STREAM: &str = r#"{
  "model": "{{MODEL}}",
  "stream": true,
  "messages": [
    {"role": "user", "content": "Reply with the literal token NYRO_PROBE_BASIC_STREAM and nothing else."}
  ]
}"#;

const OPENAI_RESPONSES_BASIC_STREAM: &str = r#"{
  "model": "{{MODEL}}",
  "stream": true,
  "input": "Reply with the literal token NYRO_PROBE_BASIC_STREAM and nothing else."
}"#;

const ANTHROPIC_BASIC_STREAM: &str = r#"{
  "model": "{{MODEL}}",
  "stream": true,
  "max_tokens": 256,
  "messages": [
    {"role": "user", "content": "Reply with the literal token NYRO_PROBE_BASIC_STREAM and nothing else."}
  ]
}"#;

const GOOGLE_BASIC_STREAM: &str = r#"{
  "contents": [
    {"role": "user", "parts": [{"text": "Reply with the literal token NYRO_PROBE_BASIC_STREAM and nothing else."}]}
  ]
}"#;

// ===========================================================================
// tool-use-stream — function calling, streaming
// ===========================================================================
const OPENAI_CHAT_TOOL_USE_STREAM: &str = r#"{
  "model": "{{MODEL}}",
  "stream": true,
  "messages": [
    {"role": "user", "content": "Probe NYRO_PROBE_TOOL_USE_STREAM. Use the get_current_weather tool to find the weather in Tokyo."}
  ],
  "tools": [
    {
      "type": "function",
      "function": {
        "name": "get_current_weather",
        "description": "Get the current weather in a city",
        "parameters": {
          "type": "object",
          "properties": {
            "city": {"type": "string", "description": "City name"}
          },
          "required": ["city"]
        }
      }
    }
  ],
  "tool_choice": "auto"
}"#;

const OPENAI_RESPONSES_TOOL_USE_STREAM: &str = r#"{
  "model": "{{MODEL}}",
  "stream": true,
  "input": "Probe NYRO_PROBE_TOOL_USE_STREAM. Use the get_current_weather tool to find the weather in Tokyo.",
  "tools": [
    {
      "type": "function",
      "name": "get_current_weather",
      "description": "Get the current weather in a city",
      "parameters": {
        "type": "object",
        "properties": {
          "city": {"type": "string", "description": "City name"}
        },
        "required": ["city"]
      }
    }
  ]
}"#;

const ANTHROPIC_TOOL_USE_STREAM: &str = r#"{
  "model": "{{MODEL}}",
  "stream": true,
  "max_tokens": 512,
  "messages": [
    {"role": "user", "content": "Probe NYRO_PROBE_TOOL_USE_STREAM. Use the get_current_weather tool to find the weather in Tokyo."}
  ],
  "tools": [
    {
      "name": "get_current_weather",
      "description": "Get the current weather in a city",
      "input_schema": {
        "type": "object",
        "properties": {
          "city": {"type": "string", "description": "City name"}
        },
        "required": ["city"]
      }
    }
  ]
}"#;

const GOOGLE_TOOL_USE_STREAM: &str = r#"{
  "contents": [
    {"role": "user", "parts": [{"text": "Probe NYRO_PROBE_TOOL_USE_STREAM. Use the get_current_weather tool to find the weather in Tokyo."}]}
  ],
  "tools": [
    {
      "function_declarations": [
        {
          "name": "get_current_weather",
          "description": "Get the current weather in a city",
          "parameters": {
            "type": "object",
            "properties": {
              "city": {"type": "string", "description": "City name"}
            },
            "required": ["city"]
          }
        }
      ]
    }
  ]
}"#;

// ===========================================================================
// reasoning-stream — chain-of-thought with provider-specific reasoning toggle
// ===========================================================================
const OPENAI_CHAT_REASONING_STREAM: &str = r#"{
  "model": "{{MODEL}}",
  "stream": true,
  "reasoning_effort": "low",
  "messages": [
    {"role": "user", "content": "Solve step by step then say NYRO_PROBE_REASONING_STREAM as the final line: what is 17 times 23?"}
  ]
}"#;

const OPENAI_RESPONSES_REASONING_STREAM: &str = r#"{
  "model": "{{MODEL}}",
  "stream": true,
  "reasoning": {"effort": "low"},
  "input": "Solve step by step then say NYRO_PROBE_REASONING_STREAM as the final line: what is 17 times 23?"
}"#;

const ANTHROPIC_REASONING_STREAM: &str = r#"{
  "model": "{{MODEL}}",
  "stream": true,
  "max_tokens": 2048,
  "thinking": {"type": "enabled", "budget_tokens": 1024},
  "messages": [
    {"role": "user", "content": "Solve step by step then say NYRO_PROBE_REASONING_STREAM as the final line: what is 17 times 23?"}
  ]
}"#;

const GOOGLE_REASONING_STREAM: &str = r#"{
  "contents": [
    {"role": "user", "parts": [{"text": "Solve step by step then say NYRO_PROBE_REASONING_STREAM as the final line: what is 17 times 23?"}]}
  ],
  "generationConfig": {
    "thinkingConfig": {"includeThoughts": true, "thinkingBudget": 1024}
  }
}"#;

// ===========================================================================
// scenario table
// ===========================================================================
pub const SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "basic-nonstream",
        anchor: ANCHOR_BASIC_NONSTREAM,
        stream: false,
        uses_reasoning_model: false,
        bodies: ScenarioBodies {
            openai_chat: Some(OPENAI_CHAT_BASIC_NONSTREAM),
            openai_responses: Some(OPENAI_RESPONSES_BASIC_NONSTREAM),
            anthropic_messages: Some(ANTHROPIC_BASIC_NONSTREAM),
            google_content: Some(GOOGLE_BASIC_NONSTREAM),
        },
        expected_fields: ScenarioExpectedFields {
            openai_chat: &["choices", "message", "content"],
            openai_responses: &["output", "content"],
            anthropic_messages: &["content", "type"],
            google_content: &["candidates", "content", "parts"],
        },
    },
    Scenario {
        name: "basic-stream",
        anchor: ANCHOR_BASIC_STREAM,
        stream: true,
        uses_reasoning_model: false,
        bodies: ScenarioBodies {
            openai_chat: Some(OPENAI_CHAT_BASIC_STREAM),
            openai_responses: Some(OPENAI_RESPONSES_BASIC_STREAM),
            anthropic_messages: Some(ANTHROPIC_BASIC_STREAM),
            google_content: Some(GOOGLE_BASIC_STREAM),
        },
        expected_fields: ScenarioExpectedFields {
            openai_chat: &["choices", "delta"],
            openai_responses: &["delta"],
            anthropic_messages: &["content_block_delta", "delta"],
            google_content: &["candidates", "content"],
        },
    },
    Scenario {
        name: "tool-use-stream",
        anchor: ANCHOR_TOOL_USE_STREAM,
        stream: true,
        uses_reasoning_model: false,
        bodies: ScenarioBodies {
            openai_chat: Some(OPENAI_CHAT_TOOL_USE_STREAM),
            openai_responses: Some(OPENAI_RESPONSES_TOOL_USE_STREAM),
            anthropic_messages: Some(ANTHROPIC_TOOL_USE_STREAM),
            google_content: Some(GOOGLE_TOOL_USE_STREAM),
        },
        expected_fields: ScenarioExpectedFields {
            openai_chat: &["tool_calls", "function"],
            openai_responses: &["function_call"],
            anthropic_messages: &["tool_use", "input_json_delta"],
            google_content: &["functionCall", "name"],
        },
    },
    Scenario {
        name: "reasoning-stream",
        anchor: ANCHOR_REASONING_STREAM,
        stream: true,
        uses_reasoning_model: true,
        bodies: ScenarioBodies {
            openai_chat: Some(OPENAI_CHAT_REASONING_STREAM),
            openai_responses: Some(OPENAI_RESPONSES_REASONING_STREAM),
            anthropic_messages: Some(ANTHROPIC_REASONING_STREAM),
            google_content: Some(GOOGLE_REASONING_STREAM),
        },
        expected_fields: ScenarioExpectedFields {
            openai_chat: &["choices", "delta"],
            openai_responses: &["delta"],
            anthropic_messages: &["content_block_delta"],
            google_content: &["candidates", "content"],
        },
    },
];

impl Scenario {
    pub fn body_for(&self, protocol: crate::protocol::ProtocolKind) -> Option<&'static str> {
        use crate::protocol::ProtocolKind::*;
        match protocol {
            OpenAiChat => self.bodies.openai_chat,
            OpenAiResponses => self.bodies.openai_responses,
            AnthropicMessages => self.bodies.anthropic_messages,
            GoogleContent => self.bodies.google_content,
        }
    }

    pub fn expected_fields_for(
        &self,
        protocol: crate::protocol::ProtocolKind,
    ) -> &'static [&'static str] {
        use crate::protocol::ProtocolKind::*;
        match protocol {
            OpenAiChat => self.expected_fields.openai_chat,
            OpenAiResponses => self.expected_fields.openai_responses,
            AnthropicMessages => self.expected_fields.anthropic_messages,
            GoogleContent => self.expected_fields.google_content,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ProtocolKind;

    #[test]
    fn all_scenarios_have_bodies_for_all_protocols() {
        for s in SCENARIOS {
            for p in [
                ProtocolKind::OpenAiChat,
                ProtocolKind::OpenAiResponses,
                ProtocolKind::AnthropicMessages,
                ProtocolKind::GoogleContent,
            ] {
                let body = s.body_for(p).unwrap_or_else(|| panic!("scenario {} missing body for {p}", s.name));
                let parsed: serde_json::Value = serde_json::from_str(body)
                    .unwrap_or_else(|e| panic!("scenario {} body for {p} not valid JSON: {e}", s.name));
                assert!(parsed.is_object(), "body for {}/{p} must be a JSON object", s.name);
                assert!(body.contains(MODEL_PLACEHOLDER) || matches!(p, ProtocolKind::GoogleContent),
                        "scenario {} body for {p} must contain {{{{MODEL}}}} placeholder unless google-content (which uses path)", s.name);
                assert!(body.contains(s.anchor), "scenario {} body for {p} must mention anchor {}", s.name, s.anchor);
            }
        }
    }

    #[test]
    fn scenario_names_have_no_double_dash() {
        for s in SCENARIOS {
            assert!(!s.name.contains("--"), "scenario name `{}` must not contain `--`", s.name);
        }
    }

    #[test]
    fn anchor_strings_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for s in SCENARIOS {
            assert!(seen.insert(s.anchor), "duplicate anchor: {}", s.anchor);
        }
    }
}
