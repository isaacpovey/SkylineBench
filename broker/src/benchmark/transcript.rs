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

/// Format a single stream-json event into a compact, human-readable line for
/// live console display during a run (spec §11 runner). Returns None for
/// events with nothing useful to show. Distinct from `render_transcript`, which
/// produces the full markdown record after the run.
pub fn format_event_live(event: &Value) -> Option<String> {
    match event.get("type")?.as_str()? {
        "system" if event.get("subtype").and_then(|s| s.as_str()) == Some("init") => {
            Some("● session started".to_string())
        }
        "assistant" => {
            let blocks: Vec<String> = event
                .get("message")?
                .get("content")?
                .as_array()?
                .iter()
                .filter_map(format_block_live)
                .collect();
            (!blocks.is_empty()).then(|| blocks.join("\n"))
        }
        "user" => {
            let blocks: Vec<String> = event
                .get("message")?
                .get("content")?
                .as_array()?
                .iter()
                .filter_map(format_result_live)
                .collect();
            (!blocks.is_empty()).then(|| blocks.join("\n"))
        }
        "result" => event
            .get("result")
            .and_then(|r| r.as_str())
            .map(|r| format!("● done: {r}")),
        _ => None,
    }
}

fn truncate(s: &str, max: usize) -> String {
    let out: String = s.chars().take(max).collect();
    if s.chars().count() > max {
        format!("{out}…")
    } else {
        out
    }
}

fn format_block_live(block: &Value) -> Option<String> {
    match block.get("type")?.as_str()? {
        "text" => {
            let t = block.get("text")?.as_str()?.trim();
            (!t.is_empty()).then(|| format!("  {t}"))
        }
        "tool_use" => {
            let name = block
                .get("name")?
                .as_str()?
                .trim_start_matches("mcp__skylinebench__");
            let input = block.get("input").cloned().unwrap_or(Value::Null);
            Some(format!("  → {name} {}", truncate(&input.to_string(), 120)))
        }
        _ => None,
    }
}

fn format_result_live(block: &Value) -> Option<String> {
    if block.get("type")?.as_str()? != "tool_result" {
        return None;
    }
    let text: String = block
        .get("content")?
        .as_array()?
        .iter()
        .filter_map(|c| c.get("text").and_then(|t| t.as_str()))
        .collect::<Vec<_>>()
        .join(" ");
    if let Ok(v) = serde_json::from_str::<Value>(&text) {
        if let Some(p) = v.get("benchmark_progress") {
            let opt = |k: &str, prec: usize| {
                p.get(k)
                    .and_then(|x| x.as_f64())
                    .map_or("?".to_string(), |n| format!("{n:.prec$}"))
            };
            let rejected = v.get("ok").and_then(|x| x.as_bool()) == Some(false);
            let target = opt("congested_meters_target", 0);
            return Some(format!(
                "    ↳ congested {}m/{target}m  flow {}  changes {}  spent {}  {}s left{}",
                opt("congested_meters_current", 0),
                opt("flow_current", 1),
                p.get("num_changes").and_then(|x| x.as_u64()).unwrap_or(0),
                p.get("money_spent").and_then(|x| x.as_i64()).unwrap_or(0),
                p.get("seconds_remaining").and_then(|x| x.as_u64()).unwrap_or(0),
                if rejected { "  (rejected)" } else { "" },
            ));
        }
    }
    Some(format!("    ↳ {}", truncate(text.trim(), 80)))
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

    #[test]
    fn live_formats_assistant_text_and_tool_call() {
        let event: Value = serde_json::from_str(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Adding a bypass."},{"type":"tool_use","name":"mcp__skylinebench__build_road","input":{"road_type":"Highway"}}]}}"#,
        )
        .unwrap();
        let line = format_event_live(&event).unwrap();
        assert!(line.contains("Adding a bypass."), "text: {line}");
        assert!(line.contains("→ build_road"), "stripped tool name: {line}");
        assert!(line.contains("Highway"), "input: {line}");
    }

    #[test]
    fn live_surfaces_benchmark_progress() {
        let event: Value = serde_json::from_str(
            r#"{"type":"user","message":{"content":[{"type":"tool_result","content":[{"type":"text","text":"{\"ok\":true,\"benchmark_progress\":{\"money_spent\":12000,\"num_changes\":3,\"congested_meters_current\":840.0,\"congested_meters_target\":50.0,\"flow_current\":12.3,\"seconds_remaining\":580}}"}]}]}}"#,
        )
        .unwrap();
        let line = format_event_live(&event).unwrap();
        assert!(line.contains("congested 840m/50m"), "congestion vs target: {line}");
        assert!(line.contains("flow 12.3"), "flow diagnostic: {line}");
        assert!(line.contains("changes 3"), "changes: {line}");
        assert!(line.contains("580s left"), "time: {line}");
    }

    #[test]
    fn live_renders_question_mark_for_null_current() {
        let event: Value = serde_json::from_str(
            r#"{"type":"user","message":{"content":[{"type":"tool_result","content":[{"type":"text","text":"{\"ok\":true,\"benchmark_progress\":{\"money_spent\":0,\"num_changes\":0,\"congested_meters_current\":null,\"congested_meters_target\":50.0,\"flow_current\":null,\"seconds_remaining\":10800}}"}]}]}}"#,
        )
        .unwrap();
        let line = format_event_live(&event).unwrap();
        assert!(line.contains("congested ?m/50m"), "null current renders ?: {line}");
        assert!(line.contains("flow ?"), "null flow renders ?: {line}");
    }

    #[test]
    fn live_skips_unknown_events() {
        let event: Value = serde_json::from_str(r#"{"type":"rate_limit_event"}"#).unwrap();
        assert!(format_event_live(&event).is_none());
    }
}
