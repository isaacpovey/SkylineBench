use serde_json::Value;

use crate::benchmark::harness::Harness;

/// One renderable block inside a turn.
pub enum Block {
    Thinking(String),
    Text(String),
    ToolUse { name: String, input: Value },
    /// A tool result's inner text parts (one harness "tool_result" block).
    ToolResult { parts: Vec<String> },
}

/// A normalized transcript event, harness-independent.
pub enum Event {
    SessionStart,
    /// Assistant turn: Thinking / Text / ToolUse blocks.
    Assistant(Vec<Block>),
    /// Tool-result turn: ToolResult blocks.
    Results(Vec<Block>),
    /// Final result text (live "done" line only).
    Done(String),
}

/// Parse one JSONL line (already deserialized) into 0+ normalized events.
pub fn parse_line(harness: Harness, v: &Value) -> Vec<Event> {
    match harness {
        Harness::Claude => parse_claude(v),
        Harness::Codex => parse_codex(v),
        Harness::Gemini => parse_gemini(v),
        Harness::Opencode => parse_opencode(v),
    }
}

fn parse_claude(v: &Value) -> Vec<Event> {
    let kind = match v.get("type").and_then(|t| t.as_str()) {
        Some(k) => k,
        None => return vec![],
    };
    match kind {
        "system" if v.get("subtype").and_then(|s| s.as_str()) == Some("init") => {
            vec![Event::SessionStart]
        }
        "assistant" => {
            let blocks = claude_blocks(v, false);
            if blocks.is_empty() { vec![] } else { vec![Event::Assistant(blocks)] }
        }
        "user" => {
            let blocks = claude_blocks(v, true);
            if blocks.is_empty() { vec![] } else { vec![Event::Results(blocks)] }
        }
        "result" => v
            .get("result")
            .and_then(|r| r.as_str())
            .map(|r| vec![Event::Done(r.to_string())])
            .unwrap_or_default(),
        _ => vec![],
    }
}

/// Collect blocks from a claude message. `results` selects tool_result blocks
/// (user turn) vs thinking/text/tool_use blocks (assistant turn).
fn claude_blocks(v: &Value, results: bool) -> Vec<Block> {
    let content = match v.get("message").and_then(|m| m.get("content")).and_then(|c| c.as_array()) {
        Some(c) => c,
        None => return vec![],
    };
    content
        .iter()
        .filter_map(|b| {
            let t = b.get("type")?.as_str()?;
            match (results, t) {
                (false, "thinking") => Some(Block::Thinking(b.get("thinking")?.as_str()?.to_string())),
                (false, "text") => Some(Block::Text(b.get("text")?.as_str()?.to_string())),
                (false, "tool_use") => Some(Block::ToolUse {
                    name: b.get("name")?.as_str()?.to_string(),
                    input: b.get("input").cloned().unwrap_or(Value::Null),
                }),
                (true, "tool_result") => {
                    let parts = b
                        .get("content")?
                        .as_array()?
                        .iter()
                        .filter_map(|c| c.get("text").and_then(|t| t.as_str()).map(String::from))
                        .collect();
                    Some(Block::ToolResult { parts })
                }
                _ => None,
            }
        })
        .collect()
}

// Stubs for later phases — return no events so other harnesses are inert until
// implemented (Tasks 11, 13, 15).
fn parse_codex(_v: &Value) -> Vec<Event> { vec![] }
fn parse_gemini(_v: &Value) -> Vec<Event> { vec![] }
fn parse_opencode(_v: &Value) -> Vec<Event> { vec![] }

/// Render a captured JSONL transcript into readable markdown.
pub fn render_transcript(harness: Harness, jsonl: &str) -> String {
    jsonl
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .flat_map(|v| parse_line(harness, &v))
        .filter_map(|e| render_md_event(&e))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn render_md_event(event: &Event) -> Option<String> {
    match event {
        Event::Assistant(blocks) => {
            let rendered: Vec<String> = blocks.iter().filter_map(render_md_block).collect();
            (!rendered.is_empty()).then(|| format!("### Assistant\n\n{}", rendered.join("\n\n")))
        }
        Event::Results(blocks) => {
            let rendered: Vec<String> = blocks.iter().filter_map(render_md_block).collect();
            (!rendered.is_empty()).then(|| format!("### Tool result\n\n{}", rendered.join("\n\n")))
        }
        Event::SessionStart | Event::Done(_) => None,
    }
}

fn render_md_block(block: &Block) -> Option<String> {
    match block {
        Block::Thinking(t) => {
            Some(format!("<details><summary>Thinking</summary>\n\n{t}\n\n</details>"))
        }
        Block::Text(t) => Some(t.clone()),
        Block::ToolUse { name, input } => {
            let pretty = serde_json::to_string_pretty(input).unwrap_or_else(|_| input.to_string());
            Some(format!("**→ {name}**\n```json\n{pretty}\n```"))
        }
        Block::ToolResult { parts } => Some(format!("```\n{}\n```", parts.join("\n"))),
    }
}

/// Format one JSONL line (already deserialized) into a compact live console
/// string. Returns None when there is nothing useful to show.
pub fn format_event_live(harness: Harness, v: &Value) -> Option<String> {
    let lines: Vec<String> = parse_line(harness, v).iter().filter_map(render_live_event).collect();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

fn render_live_event(event: &Event) -> Option<String> {
    match event {
        Event::SessionStart => Some("● session started".to_string()),
        Event::Done(r) => Some(format!("● done: {r}")),
        Event::Assistant(blocks) => {
            let lines: Vec<String> = blocks.iter().filter_map(render_live_block).collect();
            (!lines.is_empty()).then(|| lines.join("\n"))
        }
        Event::Results(blocks) => {
            let lines: Vec<String> = blocks
                .iter()
                .filter_map(|b| match b {
                    Block::ToolResult { parts } => render_live_result(parts),
                    _ => None,
                })
                .collect();
            (!lines.is_empty()).then(|| lines.join("\n"))
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    let out: String = s.chars().take(max).collect();
    if s.chars().count() > max { format!("{out}…") } else { out }
}

fn render_live_block(block: &Block) -> Option<String> {
    match block {
        Block::Thinking(t) => {
            let t = t.trim();
            (!t.is_empty()).then(|| {
                let indented = t.lines().map(|l| format!("  {l}")).collect::<Vec<_>>().join("\n");
                format!("  [thinking]\n{indented}")
            })
        }
        Block::Text(t) => {
            let t = t.trim();
            (!t.is_empty()).then(|| format!("  {t}"))
        }
        Block::ToolUse { name, input } => {
            let name = name.trim_start_matches("mcp__skylinebench__");
            Some(format!("  → {name} {}", truncate(&input.to_string(), 120)))
        }
        Block::ToolResult { .. } => None,
    }
}

fn render_live_result(parts: &[String]) -> Option<String> {
    let text = parts.join(" ");
    if let Ok(v) = serde_json::from_str::<Value>(&text) {
        if let Some(p) = v.get("city_status").or_else(|| v.get("benchmark_progress")) {
            let optf = |new: &str, old: &str, prec: usize| {
                p.get(new)
                    .or_else(|| p.get(old))
                    .and_then(|x| x.as_f64())
                    .map_or("?".to_string(), |n| format!("{n:.prec$}"))
            };
            let getu = |new: &str, old: &str| {
                p.get(new).or_else(|| p.get(old)).and_then(|x| x.as_u64()).unwrap_or(0)
            };
            let rejected = v.get("ok").and_then(|x| x.as_bool()) == Some(false);
            let junctions = p
                .get("congested_junctions")
                .and_then(|x| x.as_u64())
                .map_or("?".to_string(), |n| n.to_string());
            return Some(format!(
                "    ↳ congested {}m / {} junctions  flow {}  changes {}  spent {}  {}s left{}",
                optf("congested_road_meters", "congested_meters_current", 0),
                junctions,
                optf("traffic_flow", "flow_current", 1),
                getu("changes_made", "num_changes"),
                p.get("money_spent").and_then(|x| x.as_i64()).unwrap_or(0),
                getu("time_remaining", "seconds_remaining"),
                if rejected { "  (rejected)" } else { "" },
            ));
        }
    }
    Some(format!("    ↳ {}", truncate(text.trim(), 80)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::benchmark::harness::Harness;

    #[test]
    fn renders_assistant_text_and_tool_calls() {
        let jsonl = concat!(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Building a bypass."},{"type":"tool_use","name":"build_road","input":{"road_type":"Highway"}}]}}"#,
            "\n",
            r#"{"type":"user","message":{"content":[{"type":"tool_result","content":[{"type":"text","text":"{\"ok\":true}"}]}]}}"#,
            "\n",
        );
        let md = render_transcript(Harness::Claude, jsonl);
        assert!(md.contains("Building a bypass."), "assistant text: {md}");
        assert!(md.contains("build_road"), "tool name: {md}");
        assert!(md.contains("Highway"), "tool input: {md}");
        assert!(md.contains("ok"), "tool result: {md}");
    }

    #[test]
    fn skips_malformed_lines() {
        let md = render_transcript(Harness::Claude, "not json\n{}\n");
        assert!(md.is_empty(), "malformed-only input should render nothing, got: {md}");
    }

    #[test]
    fn live_formats_assistant_text_and_tool_call() {
        let event: Value = serde_json::from_str(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Adding a bypass."},{"type":"tool_use","name":"mcp__skylinebench__build_road","input":{"road_type":"Highway"}}]}}"#,
        )
        .unwrap();
        let line = format_event_live(Harness::Claude, &event).unwrap();
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
        let line = format_event_live(Harness::Claude, &event).unwrap();
        assert!(line.contains("congested 840m"), "congestion meters: {line}");
        assert!(line.contains("flow 12.3"), "flow diagnostic: {line}");
        assert!(line.contains("changes 3"), "changes: {line}");
        assert!(line.contains("580s left"), "time: {line}");
    }

    #[test]
    fn live_surfaces_city_status_with_junctions() {
        let event: Value = serde_json::from_str(
            r#"{"type":"user","message":{"content":[{"type":"tool_result","content":[{"type":"text","text":"{\"ok\":true,\"city_status\":{\"money_spent\":12000,\"changes_made\":3,\"congested_road_meters\":840.0,\"congested_junctions\":7,\"traffic_flow\":12.3,\"time_remaining\":580}}"}]}]}}"#,
        )
        .unwrap();
        let line = format_event_live(Harness::Claude, &event).unwrap();
        assert!(line.contains("840m"), "congestion meters: {line}");
        assert!(line.contains("7 junctions"), "junction count: {line}");
        assert!(line.contains("580s left"), "time: {line}");
    }

    #[test]
    fn live_renders_question_mark_for_null_current() {
        let event: Value = serde_json::from_str(
            r#"{"type":"user","message":{"content":[{"type":"tool_result","content":[{"type":"text","text":"{\"ok\":true,\"benchmark_progress\":{\"money_spent\":0,\"num_changes\":0,\"congested_meters_current\":null,\"congested_meters_target\":50.0,\"flow_current\":null,\"seconds_remaining\":10800}}"}]}]}}"#,
        )
        .unwrap();
        let line = format_event_live(Harness::Claude, &event).unwrap();
        assert!(line.contains("congested ?m"), "null current renders ?: {line}");
        assert!(line.contains("flow ?"), "null flow renders ?: {line}");
    }

    #[test]
    fn live_skips_unknown_events() {
        let event: Value = serde_json::from_str(r#"{"type":"rate_limit_event"}"#).unwrap();
        assert!(format_event_live(Harness::Claude, &event).is_none());
    }
}
