use serde_json::Value;

/// Render Claude Code `stream-json` (one JSON object per line) into a readable
/// markdown transcript (spec §11). Unknown/malformed lines are skipped.
pub fn render_transcript(jsonl: &str) -> String {
    jsonl
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(render_event)
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn render_event(event: Value) -> Option<String> {
    let kind = event.get("type")?.as_str()?;
    let content = event.get("message")?.get("content")?.as_array()?;
    let role = match kind {
        "assistant" => "Assistant",
        "user" => "Tool result",
        _ => return None,
    };
    let blocks: Vec<String> = content.iter().filter_map(render_block).collect();
    if blocks.is_empty() {
        return None;
    }
    Some(format!("### {role}\n\n{}", blocks.join("\n\n")))
}

fn render_block(block: &Value) -> Option<String> {
    match block.get("type")?.as_str()? {
        "text" => Some(block.get("text")?.as_str()?.to_string()),
        "tool_use" => {
            let name = block.get("name")?.as_str()?;
            let input = block.get("input").cloned().unwrap_or(Value::Null);
            let pretty = serde_json::to_string_pretty(&input).unwrap_or_else(|_| input.to_string());
            Some(format!("**→ {name}**\n```json\n{pretty}\n```"))
        }
        "tool_result" => {
            let inner = block.get("content")?.as_array()?;
            let text: String = inner
                .iter()
                .filter_map(|c| c.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("\n");
            Some(format!("```\n{text}\n```"))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_assistant_text_and_tool_calls() {
        let jsonl = concat!(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Building a bypass."},{"type":"tool_use","name":"build_road","input":{"road_type":"Highway"}}]}}"#,
            "\n",
            r#"{"type":"user","message":{"content":[{"type":"tool_result","content":[{"type":"text","text":"{\"ok\":true}"}]}]}}"#,
            "\n",
        );
        let md = render_transcript(jsonl);
        assert!(md.contains("Building a bypass."), "assistant text: {md}");
        assert!(md.contains("build_road"), "tool name: {md}");
        assert!(md.contains("Highway"), "tool input: {md}");
        assert!(md.contains("ok"), "tool result: {md}");
    }

    #[test]
    fn skips_malformed_lines() {
        let md = render_transcript("not json\n{}\n");
        assert!(md.is_empty(), "malformed-only input should render nothing, got: {md}");
    }
}
