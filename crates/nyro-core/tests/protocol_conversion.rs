use nyro_core::protocol::anthropic::stream::AnthropicResponseFormatter;
use nyro_core::protocol::anthropic::decoder::AnthropicDecoder;
use nyro_core::protocol::anthropic::encoder::AnthropicEncoder;
use nyro_core::protocol::gemini::encoder::GeminiEncoder;
use nyro_core::protocol::gemini::stream::GeminiStreamFormatter;
use nyro_core::protocol::openai::stream::OpenAIStreamFormatter;
use nyro_core::protocol::openai::encoder::OpenAIEncoder;
use nyro_core::protocol::openai::responses::decoder::ResponsesDecoder;
use nyro_core::protocol::openai::responses::encoder::ResponsesEncoder;
use nyro_core::protocol::openai::responses::formatter::ResponsesResponseFormatter;
use nyro_core::protocol::openai::responses::parser::{
    ResponsesResponseParser, ResponsesStreamParser,
};
use nyro_core::protocol::semantic::reasoning::normalize_response_reasoning;
use nyro_core::protocol::semantic::tool_correlation::normalize_request_tool_results;
use nyro_core::protocol::types::{
    ContentBlock, InternalMessage, InternalRequest, InternalResponse, MessageContent, ResponseItem, Role,
    StreamDelta,
    TokenUsage, ToolCall, ToolDef,
};
use nyro_core::protocol::ids::{
    ANTHROPIC_MESSAGES_2023_06_01, GOOGLE_GENERATE_V1BETA, OPENAI_CHAT_V1, OPENAI_RESPONSES_V1,
};
use nyro_core::protocol::{
    EgressEncoder, IngressDecoder, ResponseFormatter, ResponseParser, StreamFormatter, StreamParser,
};

#[test]
fn openai_to_anthropic_thinking_blocks() {
    let resp = InternalResponse {
        id: "msg_1".to_string(),
        model: "minimax-m2.7".to_string(),
        content: "hello".to_string(),
        reasoning_content: Some("reasoning summary".to_string()),
        tool_calls: vec![],
        response_items: None,
        stop_reason: Some("stop".to_string()),
        usage: TokenUsage {
            input_tokens: 10,
            output_tokens: 20,
        },
    };

    let out = AnthropicResponseFormatter.format_response(&resp);
    let content = out
        .get("content")
        .and_then(|v| v.as_array())
        .expect("content should be array");
    assert_eq!(content[0].get("type").and_then(|v| v.as_str()), Some("thinking"));
    assert_eq!(
        content[0].get("thinking").and_then(|v| v.as_str()),
        Some("reasoning summary")
    );
}

#[test]
fn openai_to_responses_reasoning_and_function_call_items() {
    let resp = InternalResponse {
        id: "resp_1".to_string(),
        model: "minimax-m2.7".to_string(),
        content: "done".to_string(),
        reasoning_content: Some("chain".to_string()),
        tool_calls: vec![ToolCall {
            id: "call_123".to_string(),
            name: "ls".to_string(),
            arguments: "{\"path\":\".\"}".to_string(),
        }],
        response_items: Some(vec![
            ResponseItem::Reasoning {
                text: "chain".to_string(),
            },
            ResponseItem::FunctionCall {
                call_id: "call_123".to_string(),
                name: "ls".to_string(),
                arguments: "{\"path\":\".\"}".to_string(),
            },
            ResponseItem::Message {
                text: "done".to_string(),
            },
        ]),
        stop_reason: Some("stop".to_string()),
        usage: TokenUsage::default(),
    };

    let out = ResponsesResponseFormatter.format_response(&resp);
    let output = out
        .get("output")
        .and_then(|v| v.as_array())
        .expect("output should be array");
    assert!(
        output
            .iter()
            .any(|item| item.get("type").and_then(|v| v.as_str()) == Some("reasoning"))
    );
    assert!(
        output
            .iter()
            .any(|item| item.get("type").and_then(|v| v.as_str()) == Some("function_call"))
    );
    assert!(
        output
            .iter()
            .any(|item| item.get("type").and_then(|v| v.as_str()) == Some("message"))
    );
}

#[test]
fn openai_formatter_sets_tool_calls_finish_reason_when_tool_calls_present() {
    let resp = InternalResponse {
        id: "gen_1".to_string(),
        model: "gemini-2.5-flash".to_string(),
        content: String::new(),
        reasoning_content: None,
        tool_calls: vec![ToolCall {
            id: "call_1".to_string(),
            name: "bash".to_string(),
            arguments: "{\"command\":\"ls\"}".to_string(),
        }],
        response_items: None,
        stop_reason: Some("stop".to_string()),
        usage: TokenUsage {
            input_tokens: 44,
            output_tokens: 13,
        },
    };

    let out = nyro_core::protocol::openai::stream::OpenAIResponseFormatter.format_response(&resp);
    let finish_reason = out
        .get("choices")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|c| c.get("finish_reason"))
        .and_then(|v| v.as_str());
    assert_eq!(finish_reason, Some("tool_calls"));
}

#[test]
fn openai_stream_formatter_sets_tool_calls_finish_reason_when_tool_calls_seen() {
    let mut fmt = OpenAIStreamFormatter::new();
    let events = fmt.format_deltas(&[
        StreamDelta::MessageStart {
            id: "gen_1".to_string(),
            model: "gemini-2.5-flash".to_string(),
        },
        StreamDelta::ToolCallStart {
            index: 0,
            id: "call_1".to_string(),
            name: "bash".to_string(),
        },
        StreamDelta::ToolCallDelta {
            index: 0,
            arguments: "{\"command\":\"ls\"}".to_string(),
        },
        StreamDelta::Done {
            stop_reason: "stop".to_string(),
        },
    ]);
    let last_json = events
        .iter()
        .filter_map(|e| serde_json::from_str::<serde_json::Value>(&e.data).ok())
        .last()
        .expect("has final json");
    let finish_reason = last_json
        .get("choices")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|c| c.get("finish_reason"))
        .and_then(|v| v.as_str());
    assert_eq!(finish_reason, Some("tool_calls"));
}

#[test]
fn gemini_tool_result_correlation_success() {
    let mut req = InternalRequest {
        messages: vec![
            InternalMessage {
                role: Role::Assistant,
                content: MessageContent::Text(String::new()),
                tool_calls: Some(vec![ToolCall {
                    id: "call_abc".to_string(),
                    name: "read_file".to_string(),
                    arguments: "{\"path\":\"src/main.rs\"}".to_string(),
                }]),
                tool_call_id: None,
            },
            InternalMessage {
                role: Role::Tool,
                content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                    tool_use_id: "read_file".to_string(),
                    content: serde_json::json!({"ok": true}),
                }]),
                tool_calls: None,
                tool_call_id: None,
            },
        ],
        model: "minimax-m2.7".to_string(),
        stream: false,
        temperature: None,
        max_tokens: None,
        top_p: None,
        tools: None,
        tool_choice: None,
        source_protocol: GOOGLE_GENERATE_V1BETA,
        extra: Default::default(),
    };

    normalize_request_tool_results(&mut req);
    assert_eq!(
        req.messages[1].tool_call_id.as_deref(),
        Some("call_abc"),
        "tool result should be correlated to previous assistant tool_call id"
    );
}

#[test]
fn gemini_tool_result_id_hint_matches_out_of_order_calls() {
    let mut req = InternalRequest {
        messages: vec![
            InternalMessage {
                role: Role::Assistant,
                content: MessageContent::Text(String::new()),
                tool_calls: Some(vec![
                    ToolCall {
                        id: "call_a".to_string(),
                        name: "Glob".to_string(),
                        arguments: "{}".to_string(),
                    },
                    ToolCall {
                        id: "call_b".to_string(),
                        name: "Bash".to_string(),
                        arguments: "{}".to_string(),
                    },
                ]),
                tool_call_id: None,
            },
            InternalMessage {
                role: Role::Tool,
                content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                    tool_use_id: "call_b".to_string(),
                    content: serde_json::json!({"ok": true}),
                }]),
                tool_calls: None,
                tool_call_id: None,
            },
            InternalMessage {
                role: Role::Tool,
                content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                    tool_use_id: "call_a".to_string(),
                    content: serde_json::json!({"ok": true}),
                }]),
                tool_calls: None,
                tool_call_id: None,
            },
        ],
        model: "minimax-m2.7".to_string(),
        stream: false,
        temperature: None,
        max_tokens: None,
        top_p: None,
        tools: None,
        tool_choice: None,
        source_protocol: GOOGLE_GENERATE_V1BETA,
        extra: Default::default(),
    };

    normalize_request_tool_results(&mut req);
    assert_eq!(req.messages[1].tool_call_id.as_deref(), Some("call_b"));
    assert_eq!(req.messages[2].tool_call_id.as_deref(), Some("call_a"));
}

#[test]
fn minimax_reasoning_split_fallback_think_tag() {
    let mut resp = InternalResponse {
        id: "resp_2".to_string(),
        model: "minimax-m2.7".to_string(),
        content: "<think>plan first</think>run ls".to_string(),
        reasoning_content: None,
        tool_calls: vec![],
        response_items: None,
        stop_reason: Some("stop".to_string()),
        usage: TokenUsage::default(),
    };

    normalize_response_reasoning(&mut resp);
    assert_eq!(resp.reasoning_content.as_deref(), Some("plan first"));
    assert_eq!(resp.content, "run ls");
}

#[test]
fn non_reasoning_model_no_regression() {
    let mut resp = InternalResponse {
        id: "resp_3".to_string(),
        model: "plain-model".to_string(),
        content: "hello world".to_string(),
        reasoning_content: None,
        tool_calls: vec![],
        response_items: None,
        stop_reason: Some("stop".to_string()),
        usage: TokenUsage::default(),
    };

    normalize_response_reasoning(&mut resp);
    assert!(resp.reasoning_content.is_none());
    assert_eq!(resp.content, "hello world");
}

#[test]
fn anthropic_tool_result_decodes_to_tool_role() {
    let body = serde_json::json!({
        "model": "claude-sonnet",
        "max_tokens": 1024,
        "messages": [
            {
                "role": "assistant",
                "content": [
                    {
                        "type": "tool_use",
                        "id": "call_abc",
                        "name": "read_file",
                        "input": {"path": "Cargo.toml"}
                    }
                ]
            },
            {
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "call_abc",
                        "content": {"ok": true}
                    }
                ]
            }
        ]
    });

    let req = AnthropicDecoder.decode_request(body).expect("decode anthropic request");
    assert_eq!(req.messages.len(), 2);
    assert_eq!(req.messages[1].role, Role::Tool);
    assert_eq!(req.messages[1].tool_call_id.as_deref(), Some("call_abc"));
}

#[test]
fn anthropic_multi_tool_result_decodes_to_multiple_tool_messages() {
    let body = serde_json::json!({
        "model": "claude-sonnet",
        "max_tokens": 1024,
        "messages": [
            {
                "role": "assistant",
                "content": [
                    { "type": "tool_use", "id": "call_a", "name": "read_file", "input": {"path":"a"} },
                    { "type": "tool_use", "id": "call_b", "name": "read_file", "input": {"path":"b"} }
                ]
            },
            {
                "role": "user",
                "content": [
                    { "type": "tool_result", "tool_use_id": "call_a", "content": {"ok": true} },
                    { "type": "tool_result", "tool_use_id": "call_b", "content": {"ok": true} }
                ]
            }
        ]
    });
    let req = AnthropicDecoder.decode_request(body).expect("decode anthropic request");
    assert_eq!(req.messages.len(), 3);
    assert_eq!(req.messages[1].role, Role::Tool);
    assert_eq!(req.messages[2].role, Role::Tool);
    assert_eq!(req.messages[1].tool_call_id.as_deref(), Some("call_a"));
    assert_eq!(req.messages[2].tool_call_id.as_deref(), Some("call_b"));
}

#[test]
fn openai_encoder_injects_synthetic_tool_call_before_orphan_tool_result() {
    let req = InternalRequest {
        messages: vec![InternalMessage {
            role: Role::Tool,
            content: MessageContent::Text("{\"ok\":true}".to_string()),
            tool_calls: None,
            tool_call_id: Some("call_orphan_1".to_string()),
        }],
        model: "minimax-m2.7".to_string(),
        stream: false,
        temperature: None,
        max_tokens: None,
        top_p: None,
        tools: None,
        tool_choice: None,
        source_protocol: OPENAI_RESPONSES_V1,
        extra: Default::default(),
    };

    let (body, _) = OpenAIEncoder
        .encode_request(&req)
        .expect("encode openai body");
    let messages = body
        .get("messages")
        .and_then(|v| v.as_array())
        .expect("messages array");
    assert_eq!(messages.len(), 2);
    assert_eq!(
        messages[0].get("role").and_then(|v| v.as_str()),
        Some("assistant")
    );
    assert_eq!(messages[1].get("role").and_then(|v| v.as_str()), Some("tool"));
    assert_eq!(
        messages[1].get("tool_call_id").and_then(|v| v.as_str()),
        Some("call_orphan_1")
    );
}

#[test]
fn openai_encoder_injects_adjacent_tool_call_for_non_adjacent_match() {
    let req = InternalRequest {
        messages: vec![
            InternalMessage {
                role: Role::Assistant,
                content: MessageContent::Text("will call".to_string()),
                tool_calls: Some(vec![ToolCall {
                    id: "call_x".to_string(),
                    name: "ls".to_string(),
                    arguments: "{}".to_string(),
                }]),
                tool_call_id: None,
            },
            InternalMessage {
                role: Role::User,
                content: MessageContent::Text("intermediate".to_string()),
                tool_calls: None,
                tool_call_id: None,
            },
            InternalMessage {
                role: Role::Tool,
                content: MessageContent::Text("{\"ok\":true}".to_string()),
                tool_calls: None,
                tool_call_id: Some("call_x".to_string()),
            },
        ],
        model: "minimax-m2.7".to_string(),
        stream: false,
        temperature: None,
        max_tokens: None,
        top_p: None,
        tools: None,
        tool_choice: None,
        source_protocol: OPENAI_RESPONSES_V1,
        extra: Default::default(),
    };

    let (body, _) = OpenAIEncoder
        .encode_request(&req)
        .expect("encode openai body");
    let messages = body
        .get("messages")
        .and_then(|v| v.as_array())
        .expect("messages array");

    assert_eq!(messages.len(), 4);
    assert_eq!(
        messages[2].get("role").and_then(|v| v.as_str()),
        Some("assistant")
    );
    assert_eq!(messages[3].get("role").and_then(|v| v.as_str()), Some("tool"));
    let tool_id = messages[3]
        .get("tool_call_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(!tool_id.is_empty());
    let assistant_call_id = messages[2]
        .get("tool_calls")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|tc| tc.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert_eq!(assistant_call_id, tool_id);
}

#[test]
fn openai_encoder_drops_intermediate_assistant_text_before_tool_result() {
    let req = InternalRequest {
        messages: vec![
            InternalMessage {
                role: Role::Assistant,
                content: MessageContent::Text("plan".to_string()),
                tool_calls: Some(vec![ToolCall {
                    id: "call_keep".to_string(),
                    name: "exec_command".to_string(),
                    arguments: "{\"command\":\"ls -la\"}".to_string(),
                }]),
                tool_call_id: None,
            },
            InternalMessage {
                role: Role::Assistant,
                content: MessageContent::Text("extra text".to_string()),
                tool_calls: None,
                tool_call_id: None,
            },
            InternalMessage {
                role: Role::Tool,
                content: MessageContent::Text("{\"stdout\":\"...\"}".to_string()),
                tool_calls: None,
                tool_call_id: Some("call_keep".to_string()),
            },
        ],
        model: "MiniMax-M2.7".to_string(),
        stream: false,
        temperature: None,
        max_tokens: None,
        top_p: None,
        tools: None,
        tool_choice: None,
        source_protocol: OPENAI_RESPONSES_V1,
        extra: Default::default(),
    };

    let (body, _) = OpenAIEncoder
        .encode_request(&req)
        .expect("encode openai body");
    let messages = body
        .get("messages")
        .and_then(|v| v.as_array())
        .expect("messages array");

    // intermediate assistant text should be dropped to keep tool_result adjacent
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0].get("role").and_then(|v| v.as_str()), Some("assistant"));
    assert_eq!(
        messages[1]
            .get("tool_calls")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|tc| tc.get("id"))
            .and_then(|v| v.as_str()),
        Some("call_keep")
    );
    assert_eq!(messages[2].get("role").and_then(|v| v.as_str()), Some("tool"));
    assert_eq!(
        messages[2].get("tool_call_id").and_then(|v| v.as_str()),
        Some("call_keep")
    );
}

#[test]
fn openai_encoder_remaps_duplicate_tool_call_ids() {
    let req = InternalRequest {
        messages: vec![
            InternalMessage {
                role: Role::Assistant,
                content: MessageContent::Text(String::new()),
                tool_calls: Some(vec![ToolCall {
                    id: "call_dup".to_string(),
                    name: "exec_command".to_string(),
                    arguments: "{}".to_string(),
                }]),
                tool_call_id: None,
            },
            InternalMessage {
                role: Role::Assistant,
                content: MessageContent::Text(String::new()),
                tool_calls: Some(vec![ToolCall {
                    id: "call_dup".to_string(),
                    name: "exec_command".to_string(),
                    arguments: "{}".to_string(),
                }]),
                tool_call_id: None,
            },
            InternalMessage {
                role: Role::Tool,
                content: MessageContent::Text("{\"ok\":true}".to_string()),
                tool_calls: None,
                tool_call_id: Some("call_dup".to_string()),
            },
            InternalMessage {
                role: Role::Tool,
                content: MessageContent::Text("{\"ok\":true}".to_string()),
                tool_calls: None,
                tool_call_id: Some("call_dup".to_string()),
            },
        ],
        model: "MiniMax-M2.7".to_string(),
        stream: false,
        temperature: None,
        max_tokens: None,
        top_p: None,
        tools: None,
        tool_choice: None,
        source_protocol: OPENAI_RESPONSES_V1,
        extra: Default::default(),
    };

    let (body, _) = OpenAIEncoder
        .encode_request(&req)
        .expect("encode openai body");
    let messages = body
        .get("messages")
        .and_then(|v| v.as_array())
        .expect("messages array");

    let ids: Vec<String> = messages
        .iter()
        .filter_map(|m| m.get("tool_calls").and_then(|v| v.as_array()).and_then(|arr| arr.first()))
        .filter_map(|tc| tc.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .collect();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1]);

    let tool_ids: Vec<String> = messages
        .iter()
        .filter(|m| m.get("role").and_then(|v| v.as_str()) == Some("tool"))
        .filter_map(|m| m.get("tool_call_id").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .collect();
    assert_eq!(tool_ids.len(), 2);
    assert!(ids.contains(&tool_ids[0]));
    assert!(ids.contains(&tool_ids[1]));
}

#[test]
fn anthropic_encoder_maps_required_tool_choice_to_any() {
    let req = InternalRequest {
        messages: vec![InternalMessage {
            role: Role::User,
            content: MessageContent::Text("hello".to_string()),
            tool_calls: None,
            tool_call_id: None,
        }],
        model: "MiniMax-M2.7".to_string(),
        stream: false,
        temperature: None,
        max_tokens: Some(256),
        top_p: None,
        tools: Some(vec![ToolDef {
            name: "exec_command".to_string(),
            description: Some("Execute command".to_string()),
            parameters: serde_json::json!({"type":"object","properties":{"command":{"type":"string"}}}),
        }]),
        tool_choice: Some(serde_json::json!("required")),
        source_protocol: OPENAI_RESPONSES_V1,
        extra: Default::default(),
    };

    let (body, _) = AnthropicEncoder
        .encode_request(&req)
        .expect("encode anthropic body");
    assert_eq!(
        body.get("tool_choice")
            .and_then(|v| v.get("type"))
            .and_then(|v| v.as_str()),
        Some("any")
    );
}

#[test]
fn anthropic_encoder_maps_function_tool_choice_to_tool_name() {
    let req = InternalRequest {
        messages: vec![InternalMessage {
            role: Role::User,
            content: MessageContent::Text("hello".to_string()),
            tool_calls: None,
            tool_call_id: None,
        }],
        model: "MiniMax-M2.7".to_string(),
        stream: false,
        temperature: None,
        max_tokens: Some(256),
        top_p: None,
        tools: Some(vec![ToolDef {
            name: "exec_command".to_string(),
            description: Some("Execute command".to_string()),
            parameters: serde_json::json!({"type":"object","properties":{"command":{"type":"string"}}}),
        }]),
        tool_choice: Some(serde_json::json!({
            "type":"function",
            "function":{"name":"exec_command"}
        })),
        source_protocol: OPENAI_RESPONSES_V1,
        extra: Default::default(),
    };

    let (body, _) = AnthropicEncoder
        .encode_request(&req)
        .expect("encode anthropic body");
    assert_eq!(
        body.get("tool_choice")
            .and_then(|v| v.get("type"))
            .and_then(|v| v.as_str()),
        Some("tool")
    );
    assert_eq!(
        body.get("tool_choice")
            .and_then(|v| v.get("name"))
            .and_then(|v| v.as_str()),
        Some("exec_command")
    );
}

#[test]
fn anthropic_encoder_merges_consecutive_roles_and_drops_empty_text() {
    let req = InternalRequest {
        messages: vec![
            InternalMessage {
                role: Role::User,
                content: MessageContent::Text("first".to_string()),
                tool_calls: None,
                tool_call_id: None,
            },
            InternalMessage {
                role: Role::User,
                content: MessageContent::Text("second".to_string()),
                tool_calls: None,
                tool_call_id: None,
            },
            InternalMessage {
                role: Role::Assistant,
                content: MessageContent::Text(String::new()),
                tool_calls: None,
                tool_call_id: None,
            },
            InternalMessage {
                role: Role::Assistant,
                content: MessageContent::Text("tool".to_string()),
                tool_calls: Some(vec![ToolCall {
                    id: "call_1".to_string(),
                    name: "exec_command".to_string(),
                    arguments: "{}".to_string(),
                }]),
                tool_call_id: None,
            },
            InternalMessage {
                role: Role::Tool,
                content: MessageContent::Text("result".to_string()),
                tool_calls: None,
                tool_call_id: Some("call_1".to_string()),
            },
        ],
        model: "MiniMax-M2.7".to_string(),
        stream: false,
        temperature: None,
        max_tokens: Some(256),
        top_p: None,
        tools: None,
        tool_choice: None,
        source_protocol: OPENAI_RESPONSES_V1,
        extra: Default::default(),
    };

    let (body, _) = AnthropicEncoder
        .encode_request(&req)
        .expect("encode anthropic body");
    let msgs = body
        .get("messages")
        .and_then(|v| v.as_array())
        .expect("messages array");
    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs[0].get("role").and_then(|v| v.as_str()), Some("user"));
    assert_eq!(msgs[1].get("role").and_then(|v| v.as_str()), Some("assistant"));
    assert_eq!(msgs[2].get("role").and_then(|v| v.as_str()), Some("user"));

    let first_blocks = msgs[0]
        .get("content")
        .and_then(|v| v.as_array())
        .expect("first content blocks");
    assert_eq!(first_blocks.len(), 2);
    assert_eq!(
        first_blocks[0].get("text").and_then(|v| v.as_str()),
        Some("first")
    );
    assert_eq!(
        first_blocks[1].get("text").and_then(|v| v.as_str()),
        Some("second")
    );
}

#[test]
fn anthropic_encoder_normalizes_tool_use_ids_for_tool_and_result() {
    let req = InternalRequest {
        messages: vec![
            InternalMessage {
                role: Role::Assistant,
                content: MessageContent::Text(String::new()),
                tool_calls: Some(vec![ToolCall {
                    id: "call_function_abc_1".to_string(),
                    name: "glob".to_string(),
                    arguments: "{}".to_string(),
                }]),
                tool_call_id: None,
            },
            InternalMessage {
                role: Role::Tool,
                content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                    tool_use_id: "call_function_abc_1".to_string(),
                    content: serde_json::json!({"ok": true}),
                }]),
                tool_calls: None,
                tool_call_id: Some("call_function_abc_1".to_string()),
            },
        ],
        model: "MiniMax-M2.7".to_string(),
        stream: false,
        temperature: None,
        max_tokens: Some(256),
        top_p: None,
        tools: Some(vec![ToolDef {
            name: "glob".to_string(),
            description: None,
            parameters: serde_json::json!({"type":"object","properties":{}}),
        }]),
        tool_choice: None,
        source_protocol: GOOGLE_GENERATE_V1BETA,
        extra: Default::default(),
    };

    let (body, _) = AnthropicEncoder.encode_request(&req).expect("encode anthropic body");
    let msgs = body
        .get("messages")
        .and_then(|v| v.as_array())
        .expect("messages");
    let tool_use_id = msgs[0]
        .get("content")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|b| b.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let tool_result_id = msgs[1]
        .get("content")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|b| b.get("tool_use_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(tool_use_id.starts_with("toolu_"));
    assert_eq!(tool_use_id, tool_result_id);
}

#[test]
fn responses_decoder_ignores_empty_message_content_item() {
    let body = serde_json::json!({
        "model": "MiniMax-M2.7-Code-Claude",
        "input": [
            { "type": "message", "role": "user", "content": [] },
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "帮我查看当前目录下有哪些文件" }]
            }
        ]
    });

    let req = ResponsesDecoder
        .decode_request(body)
        .expect("decode request should succeed");
    assert_eq!(req.messages.len(), 1);
    assert_eq!(req.messages[0].role, Role::User);
    assert_eq!(
        req.messages[0].content.as_text(),
        "帮我查看当前目录下有哪些文件"
    );
}

#[test]
fn openai_encoder_remaps_reused_tool_result_id_with_synthetic_adjacent_call() {
    let req = InternalRequest {
        messages: vec![
            InternalMessage {
                role: Role::Assistant,
                content: MessageContent::Text(String::new()),
                tool_calls: Some(vec![ToolCall {
                    id: "call_same".to_string(),
                    name: "exec_command".to_string(),
                    arguments: "{}".to_string(),
                }]),
                tool_call_id: None,
            },
            InternalMessage {
                role: Role::Tool,
                content: MessageContent::Text("ok1".to_string()),
                tool_calls: None,
                tool_call_id: Some("call_same".to_string()),
            },
            InternalMessage {
                role: Role::Assistant,
                content: MessageContent::Text("intermediate".to_string()),
                tool_calls: None,
                tool_call_id: None,
            },
            InternalMessage {
                role: Role::Tool,
                content: MessageContent::Text("ok2".to_string()),
                tool_calls: None,
                tool_call_id: Some("call_same".to_string()),
            },
        ],
        model: "gpt-4o-mini".to_string(),
        stream: false,
        temperature: None,
        max_tokens: None,
        top_p: None,
        tools: Some(vec![ToolDef {
            name: "exec_command".to_string(),
            description: None,
            parameters: serde_json::json!({"type":"object","properties":{}}),
        }]),
        tool_choice: None,
        source_protocol: OPENAI_CHAT_V1,
        extra: Default::default(),
    };

    let (body, _) = OpenAIEncoder.encode_request(&req).expect("encode");
    let msgs = body
        .get("messages")
        .and_then(|v| v.as_array())
        .expect("messages");
    let mut tool_ids: Vec<String> = Vec::new();
    for msg in msgs {
        if msg.get("role").and_then(|v| v.as_str()) == Some("tool") {
            let id = msg
                .get("tool_call_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            assert!(!id.is_empty());
            tool_ids.push(id);
        }
    }
    assert_eq!(tool_ids.len(), 2);
    assert_ne!(tool_ids[0], tool_ids[1]);
}

#[test]
fn openai_encoder_rewrites_multi_tool_call_history_to_adjacent_pairs() {
    let req = InternalRequest {
        messages: vec![
            InternalMessage {
                role: Role::Assistant,
                content: MessageContent::Text("".to_string()),
                tool_calls: Some(vec![
                    ToolCall {
                        id: "call_a".to_string(),
                        name: "Glob".to_string(),
                        arguments: "{}".to_string(),
                    },
                    ToolCall {
                        id: "call_b".to_string(),
                        name: "Bash".to_string(),
                        arguments: "{}".to_string(),
                    },
                ]),
                tool_call_id: None,
            },
            InternalMessage {
                role: Role::Tool,
                content: MessageContent::Text("r1".to_string()),
                tool_calls: None,
                tool_call_id: Some("call_a".to_string()),
            },
            InternalMessage {
                role: Role::Tool,
                content: MessageContent::Text("r2".to_string()),
                tool_calls: None,
                tool_call_id: Some("call_b".to_string()),
            },
        ],
        model: "MiniMax-M2.7".to_string(),
        stream: false,
        temperature: None,
        max_tokens: None,
        top_p: None,
        tools: Some(vec![ToolDef {
            name: "Glob".to_string(),
            description: None,
            parameters: serde_json::json!({"type":"object","properties":{}}),
        }]),
        tool_choice: None,
        source_protocol: ANTHROPIC_MESSAGES_2023_06_01,
        extra: Default::default(),
    };

    let (body, _) = OpenAIEncoder.encode_request(&req).expect("encode");
    let msgs = body
        .get("messages")
        .and_then(|v| v.as_array())
        .expect("messages");
    assert_eq!(msgs.len(), 4);
    assert_eq!(msgs[0].get("role").and_then(|v| v.as_str()), Some("assistant"));
    assert_eq!(msgs[1].get("role").and_then(|v| v.as_str()), Some("tool"));
    assert_eq!(msgs[2].get("role").and_then(|v| v.as_str()), Some("assistant"));
    assert_eq!(msgs[3].get("role").and_then(|v| v.as_str()), Some("tool"));
    let id1 = msgs[1].get("tool_call_id").and_then(|v| v.as_str()).unwrap_or("");
    let id2 = msgs[3].get("tool_call_id").and_then(|v| v.as_str()).unwrap_or("");
    let prev1 = msgs[0]
        .get("tool_calls")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|tc| tc.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let prev2 = msgs[2]
        .get("tool_calls")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|tc| tc.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert_eq!(id1, prev1);
    assert_eq!(id2, prev2);
}

#[test]
fn openai_encoder_drops_orphan_assistant_tool_calls_without_results() {
    let req = InternalRequest {
        messages: vec![
            InternalMessage {
                role: Role::System,
                content: MessageContent::Text("sys".to_string()),
                tool_calls: None,
                tool_call_id: None,
            },
            InternalMessage {
                role: Role::Assistant,
                content: MessageContent::Text(String::new()),
                tool_calls: Some(vec![
                    ToolCall {
                        id: "call_old_1".to_string(),
                        name: String::new(),
                        arguments: "{}".to_string(),
                    },
                    ToolCall {
                        id: "call_old_2".to_string(),
                        name: "list_directory".to_string(),
                        arguments: "{}".to_string(),
                    },
                ]),
                tool_call_id: None,
            },
            InternalMessage {
                role: Role::Assistant,
                content: MessageContent::Text(String::new()),
                tool_calls: Some(vec![ToolCall {
                    id: "call_new".to_string(),
                    name: "glob".to_string(),
                    arguments: "{}".to_string(),
                }]),
                tool_call_id: None,
            },
            InternalMessage {
                role: Role::Tool,
                content: MessageContent::Text("{\"ok\":true}".to_string()),
                tool_calls: None,
                tool_call_id: Some("call_new".to_string()),
            },
        ],
        model: "MiniMax-M2.7".to_string(),
        stream: false,
        temperature: None,
        max_tokens: None,
        top_p: None,
        tools: Some(vec![ToolDef {
            name: "glob".to_string(),
            description: None,
            parameters: serde_json::json!({"type":"object","properties":{}}),
        }]),
        tool_choice: None,
        source_protocol: GOOGLE_GENERATE_V1BETA,
        extra: Default::default(),
    };

    let (body, _) = OpenAIEncoder.encode_request(&req).expect("encode");
    let msgs = body
        .get("messages")
        .and_then(|v| v.as_array())
        .expect("messages");
    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs[0].get("role").and_then(|v| v.as_str()), Some("system"));
    assert_eq!(msgs[1].get("role").and_then(|v| v.as_str()), Some("assistant"));
    assert_eq!(msgs[2].get("role").and_then(|v| v.as_str()), Some("tool"));
    let call_id = msgs[1]
        .get("tool_calls")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|tc| tc.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert_eq!(call_id, "call_new");
}

#[test]
fn gemini_stream_formatter_keeps_tool_name_for_argument_deltas() {
    let mut fmt = GeminiStreamFormatter::new();
    let deltas = vec![
        StreamDelta::MessageStart {
            id: "x".to_string(),
            model: "m".to_string(),
        },
        StreamDelta::ToolCallStart {
            index: 0,
            id: "call_1".to_string(),
            name: "run_shell_command".to_string(),
        },
        StreamDelta::ToolCallDelta {
            index: 0,
            arguments: "{\"command\":\"ls -la\"}".to_string(),
        },
    ];
    let events = fmt.format_deltas(&deltas);
    let mut saw_named_call = false;
    let mut saw_command_arg = false;
    for ev in events {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&ev.data) else {
            continue;
        };
        let part = v
            .get("candidates")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|c| c.get("content"))
            .and_then(|c| c.get("parts"))
            .and_then(|p| p.as_array())
            .and_then(|arr| arr.first())
            .and_then(|p| p.get("functionCall"));
        if let Some(fc) = part {
            if fc.get("name").and_then(|n| n.as_str()) == Some("run_shell_command") {
                saw_named_call = true;
            }
            if fc
                .get("args")
                .and_then(|a| a.get("command"))
                .and_then(|c| c.as_str())
                == Some("ls -la")
            {
                saw_command_arg = true;
            }
        }
    }
    assert!(saw_named_call);
    assert!(saw_command_arg);
}

#[test]
fn gemini_stream_formatter_normalizes_common_tool_argument_aliases() {
    let mut fmt = GeminiStreamFormatter::new();
    let deltas = vec![
        StreamDelta::MessageStart {
            id: "x".to_string(),
            model: "m".to_string(),
        },
        StreamDelta::ToolCallStart {
            index: 0,
            id: "call_1".to_string(),
            name: "glob".to_string(),
        },
        StreamDelta::ToolCallDelta {
            index: 0,
            arguments: "{\"include_pattern\":\"**/*.py\",\"search_root\":\"/tmp/work\",\"exclude_pattern\":\"**/.venv/**\"}".to_string(),
        },
    ];
    let events = fmt.format_deltas(&deltas);
    let payload = events
        .iter()
        .filter_map(|e| serde_json::from_str::<serde_json::Value>(&e.data).ok())
        .find_map(|v| {
            v.get("candidates")
                .and_then(|c| c.as_array())
                .and_then(|arr| arr.first())
                .and_then(|c| c.get("content"))
                .and_then(|c| c.get("parts"))
                .and_then(|p| p.as_array())
                .and_then(|arr| arr.first())
                .and_then(|p| p.get("functionCall"))
                .cloned()
        })
        .expect("functionCall payload");

    assert_eq!(
        payload.get("name").and_then(|v| v.as_str()),
        Some("glob")
    );
    let args = payload.get("args").expect("args object");
    assert_eq!(args.get("pattern").and_then(|v| v.as_str()), Some("**/*.py"));
    assert_eq!(args.get("root_dir").and_then(|v| v.as_str()), Some("/tmp/work"));
    assert_eq!(
        args.get("exclude_patterns")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str()),
        Some("**/.venv/**")
    );
}

#[test]
fn gemini_encoder_sanitizes_unsupported_json_schema_fields() {
    let req = InternalRequest {
        messages: vec![InternalMessage {
            role: Role::User,
            content: MessageContent::Text("hello".to_string()),
            tool_calls: None,
            tool_call_id: None,
        }],
        model: "gemini-2.5-flash".to_string(),
        stream: false,
        temperature: None,
        max_tokens: None,
        top_p: None,
        tools: Some(vec![ToolDef {
            name: "glob".to_string(),
            description: Some("glob files".to_string()),
            parameters: serde_json::json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "pattern": {"type": "string"},
                    "items": {
                        "type": "array",
                        "items": {
                            "$ref": "#/$defs/entry",
                            "ref": "legacy"
                        }
                    }
                },
                "$defs": {
                    "entry": {"type":"string"}
                }
            }),
        }]),
        tool_choice: None,
        source_protocol: OPENAI_CHAT_V1,
        extra: Default::default(),
    };

    let (body, _) = GeminiEncoder.encode_request(&req).expect("encode");
    let params = body
        .get("tools")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.get("functionDeclarations"))
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.get("parameters"))
        .cloned()
        .expect("parameters");

    let rendered = params.to_string();
    assert!(!rendered.contains("$schema"));
    assert!(!rendered.contains("additionalProperties"));
    assert!(!rendered.contains("$ref"));
    assert!(!rendered.contains("\"ref\""));
    assert!(!rendered.contains("$defs"));
}

fn responses_request(messages: Vec<InternalMessage>, stream: bool) -> InternalRequest {
    InternalRequest {
        messages,
        model: "gpt-5.4".to_string(),
        stream,
        temperature: None,
        max_tokens: None,
        top_p: None,
        tools: None,
        tool_choice: None,
        source_protocol: OPENAI_RESPONSES_V1,
        extra: Default::default(),
    }
}

#[test]
fn responses_encoder_targets_slash_v1_responses_and_forces_stream() {
    let req = responses_request(
        vec![InternalMessage {
            role: Role::User,
            content: MessageContent::Text("hello".to_string()),
            tool_calls: None,
            tool_call_id: None,
        }],
        false,
    );

    let (body, _) = ResponsesEncoder.encode_request(&req).expect("encode");
    assert_eq!(
        body.get("stream").and_then(|v| v.as_bool()),
        Some(true),
        "responses backends only accept stream:true"
    );
    assert_eq!(
        body.get("store").and_then(|v| v.as_bool()),
        Some(false),
        "gateway never persists server-side state"
    );
    assert_eq!(
        ResponsesEncoder.egress_path("gpt-5.4", false),
        "/v1/responses"
    );
}

#[test]
fn responses_encoder_splits_system_to_instructions_and_user_to_input_text() {
    let req = responses_request(
        vec![
            InternalMessage {
                role: Role::System,
                content: MessageContent::Text("be terse".to_string()),
                tool_calls: None,
                tool_call_id: None,
            },
            InternalMessage {
                role: Role::User,
                content: MessageContent::Text("hi".to_string()),
                tool_calls: None,
                tool_call_id: None,
            },
        ],
        false,
    );

    let (body, _) = ResponsesEncoder.encode_request(&req).expect("encode");
    assert_eq!(
        body.get("instructions").and_then(|v| v.as_str()),
        Some("be terse")
    );
    let input = body.get("input").and_then(|v| v.as_array()).expect("input");
    assert_eq!(input.len(), 1);
    assert_eq!(input[0].get("type").and_then(|v| v.as_str()), Some("message"));
    assert_eq!(input[0].get("role").and_then(|v| v.as_str()), Some("user"));
    let first_block = input[0]
        .get("content")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .expect("first content block");
    assert_eq!(
        first_block.get("type").and_then(|v| v.as_str()),
        Some("input_text")
    );
    assert_eq!(first_block.get("text").and_then(|v| v.as_str()), Some("hi"));
}

#[test]
fn responses_encoder_emits_function_call_and_function_call_output_items() {
    let req = responses_request(
        vec![
            InternalMessage {
                role: Role::Assistant,
                content: MessageContent::Text(String::new()),
                tool_calls: Some(vec![ToolCall {
                    id: "call_abc".to_string(),
                    name: "list_dir".to_string(),
                    arguments: "{\"path\":\".\"}".to_string(),
                }]),
                tool_call_id: None,
            },
            InternalMessage {
                role: Role::Tool,
                content: MessageContent::Text("file1\nfile2".to_string()),
                tool_calls: None,
                tool_call_id: Some("call_abc".to_string()),
            },
        ],
        false,
    );

    let (body, _) = ResponsesEncoder.encode_request(&req).expect("encode");
    let input = body.get("input").and_then(|v| v.as_array()).expect("input");
    assert_eq!(input.len(), 2, "one function_call + one function_call_output");

    assert_eq!(
        input[0].get("type").and_then(|v| v.as_str()),
        Some("function_call")
    );
    assert_eq!(
        input[0].get("call_id").and_then(|v| v.as_str()),
        Some("call_abc")
    );
    assert_eq!(
        input[0].get("name").and_then(|v| v.as_str()),
        Some("list_dir")
    );
    assert_eq!(
        input[0].get("arguments").and_then(|v| v.as_str()),
        Some("{\"path\":\".\"}")
    );

    assert_eq!(
        input[1].get("type").and_then(|v| v.as_str()),
        Some("function_call_output")
    );
    assert_eq!(
        input[1].get("call_id").and_then(|v| v.as_str()),
        Some("call_abc")
    );
    assert_eq!(
        input[1].get("output").and_then(|v| v.as_str()),
        Some("file1\nfile2")
    );
}

#[test]
fn responses_encoder_drops_max_output_tokens_for_codex_compat() {
    let mut req = responses_request(
        vec![InternalMessage {
            role: Role::User,
            content: MessageContent::Text("hi".to_string()),
            tool_calls: None,
            tool_call_id: None,
        }],
        false,
    );
    req.max_tokens = Some(128);

    let (body, _) = ResponsesEncoder.encode_request(&req).expect("encode");
    assert!(
        body.get("max_output_tokens").is_none(),
        "codex backend rejects max_output_tokens; callers needing a cap must use extra"
    );
}

#[test]
fn responses_stream_parser_extracts_text_and_usage() {
    let sse = "event: response.created\n\
data: {\"response\":{\"id\":\"resp_1\",\"model\":\"gpt-5.4\"}}\n\
\n\
event: response.output_text.delta\n\
data: {\"delta\":\"Hel\"}\n\
\n\
event: response.output_text.delta\n\
data: {\"delta\":\"lo\"}\n\
\n\
event: response.completed\n\
data: {\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":7,\"output_tokens\":2}}}\n\
\n";

    let mut parser = ResponsesStreamParser::new();
    let deltas = parser.parse_chunk(sse).expect("parse");

    let mut saw_start = false;
    let mut text_concat = String::new();
    let mut usage_input = 0;
    let mut usage_output = 0;
    let mut done_reason: Option<String> = None;

    for delta in &deltas {
        match delta {
            StreamDelta::MessageStart { id, model } => {
                saw_start = true;
                assert_eq!(id, "resp_1");
                assert_eq!(model, "gpt-5.4");
            }
            StreamDelta::TextDelta(t) => text_concat.push_str(t),
            StreamDelta::Usage(u) => {
                usage_input = u.input_tokens;
                usage_output = u.output_tokens;
            }
            StreamDelta::Done { stop_reason } => done_reason = Some(stop_reason.clone()),
            _ => {}
        }
    }

    assert!(saw_start);
    assert_eq!(text_concat, "Hello");
    assert_eq!(usage_input, 7);
    assert_eq!(usage_output, 2);
    assert_eq!(done_reason.as_deref(), Some("completed"));
}

#[test]
fn responses_stream_parser_extracts_function_call() {
    let sse = "event: response.output_item.added\n\
data: {\"output_index\":0,\"item\":{\"type\":\"function_call\",\"call_id\":\"call_xyz\",\"name\":\"ls\"}}\n\
\n\
event: response.function_call_arguments.delta\n\
data: {\"output_index\":0,\"delta\":\"{\\\"a\\\":1\"}\n\
\n\
event: response.function_call_arguments.delta\n\
data: {\"output_index\":0,\"delta\":\"}\"}\n\
\n";

    let mut parser = ResponsesStreamParser::new();
    let deltas = parser.parse_chunk(sse).expect("parse");

    let mut got_start = false;
    let mut arg_concat = String::new();
    for delta in &deltas {
        match delta {
            StreamDelta::ToolCallStart { id, name, .. } => {
                got_start = true;
                assert_eq!(id, "call_xyz");
                assert_eq!(name, "ls");
            }
            StreamDelta::ToolCallDelta { arguments, .. } => arg_concat.push_str(arguments),
            _ => {}
        }
    }
    assert!(got_start);
    assert_eq!(arg_concat, "{\"a\":1}");
}

#[test]
fn responses_response_parser_extracts_text_tool_calls_and_usage() {
    let body = serde_json::json!({
        "id": "resp_42",
        "model": "gpt-5.4",
        "status": "completed",
        "output": [
            {
                "type": "message",
                "content": [
                    {"type": "output_text", "text": "Hi "},
                    {"type": "output_text", "text": "there"}
                ]
            },
            {
                "type": "function_call",
                "call_id": "call_1",
                "name": "search",
                "arguments": "{\"q\":\"rust\"}"
            }
        ],
        "usage": {"input_tokens": 11, "output_tokens": 3}
    });

    let resp = ResponsesResponseParser
        .parse_response(body)
        .expect("parse");

    assert_eq!(resp.id, "resp_42");
    assert_eq!(resp.model, "gpt-5.4");
    assert_eq!(resp.content, "Hi there");
    assert_eq!(resp.stop_reason.as_deref(), Some("completed"));
    assert_eq!(resp.usage.input_tokens, 11);
    assert_eq!(resp.usage.output_tokens, 3);
    assert_eq!(resp.tool_calls.len(), 1);
    assert_eq!(resp.tool_calls[0].id, "call_1");
    assert_eq!(resp.tool_calls[0].name, "search");
    assert_eq!(resp.tool_calls[0].arguments, "{\"q\":\"rust\"}");
}
