use crate::protocol::types::{InternalResponse, ResponseItem};

pub fn populate_response_items(resp: &mut InternalResponse) {
    if resp.response_items.is_some() {
        return;
    }

    let mut items: Vec<ResponseItem> = Vec::new();

    if let Some(reasoning) = resp
        .reasoning_content
        .as_ref()
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
    {
        items.push(ResponseItem::Reasoning {
            text: reasoning.to_string(),
        });
    }

    for tc in &resp.tool_calls {
        items.push(ResponseItem::FunctionCall {
            call_id: tc.id.clone(),
            name: tc.name.clone(),
            arguments: tc.arguments.clone(),
        });
    }

    if !resp.content.trim().is_empty() {
        items.push(ResponseItem::Message {
            text: resp.content.clone(),
        });
    }

    if !items.is_empty() {
        resp.response_items = Some(items);
    }
}
