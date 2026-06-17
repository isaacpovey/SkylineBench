use super::{ConfigFile, LaunchInputs, LaunchSpec};

/// The MCP tool allowlist Claude is given (the benchmark tools only).
pub const ALLOWED: &str = "mcp__skylinebench__build_road,mcp__skylinebench__bulldoze,mcp__skylinebench__upgrade_road,mcp__skylinebench__set_zoning,mcp__skylinebench__control_time,mcp__skylinebench__get_city_overview,mcp__skylinebench__observe_area,mcp__skylinebench__get_metrics,mcp__skylinebench__list_road_types,mcp__skylinebench__list_zone_types,mcp__skylinebench__render_map,mcp__skylinebench__submit_solution,mcp__skylinebench__query_problems,mcp__skylinebench__query_segments,mcp__skylinebench__apply_plan,mcp__skylinebench__trace_route,mcp__skylinebench__validate_road";

pub fn spec(inputs: &LaunchInputs) -> LaunchSpec {
    let mcp_config = inputs.session_dir.join("mcp.json");
    let mcp_config_str = mcp_config.display().to_string();
    let contents = serde_json::json!({
        "mcpServers": {
            "skylinebench": { "command": "sh", "args": ["-c", inputs.mcp_shell] }
        }
    });

    let model_args = inputs
        .model
        .iter()
        .flat_map(|m| ["--model".to_string(), m.clone()]);
    let head = ["claude", "-p", inputs.prompt.as_str()].map(String::from);
    let tail = [
        "--mcp-config",
        mcp_config_str.as_str(),
        "--strict-mcp-config",
        "--allowedTools",
        ALLOWED,
        "--disallowedTools",
        "WebFetch,WebSearch",
        "--permission-mode",
        "bypassPermissions",
        "--output-format",
        "stream-json",
        "--verbose",
    ]
    .map(String::from);
    let argv: Vec<String> = head.into_iter().chain(model_args).chain(tail).collect();

    LaunchSpec {
        argv,
        // CLAUDE_CONFIG_DIR is the operator's persistent OAuth dir, set by run.sh.
        env: vec![],
        config_files: vec![ConfigFile {
            path: mcp_config,
            contents: serde_json::to_string_pretty(&contents)
                .expect("serde_json::Value is always serializable"),
        }],
        required_env: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn inputs() -> LaunchInputs {
        LaunchInputs {
            model: Some("claude-opus-4-8".to_string()),
            prompt: "improve traffic".to_string(),
            mcp_shell: "/s/skylinebench benchmark --map m".to_string(),
            session_dir: PathBuf::from("/tmp/sess"),
        }
    }

    #[test]
    fn builds_claude_argv_with_model_and_stream_json() {
        let spec = spec(&inputs());
        assert_eq!(spec.argv[0], "claude");
        assert!(spec.argv.contains(&"-p".to_string()));
        assert!(spec.argv.contains(&"improve traffic".to_string()));
        assert!(spec
            .argv
            .windows(2)
            .any(|w| w == ["--model", "claude-opus-4-8"]));
        assert!(spec.argv.contains(&"stream-json".to_string()));
        assert!(spec.argv.contains(&"bypassPermissions".to_string()));
        assert!(spec
            .argv
            .iter()
            .any(|a| a.contains("mcp__skylinebench__query_problems")));
        assert!(spec
            .argv
            .iter()
            .any(|a| a.contains("mcp__skylinebench__validate_road")));
        assert!(spec.required_env.is_empty());
    }

    #[test]
    fn omits_model_when_absent() {
        let mut i = inputs();
        i.model = None;
        let spec = spec(&i);
        assert!(!spec.argv.contains(&"--model".to_string()));
    }

    #[test]
    fn writes_mcp_json_with_server() {
        let spec = spec(&inputs());
        let cf = &spec.config_files[0];
        assert!(cf.path.ends_with("mcp.json"));
        assert!(cf.contents.contains("mcpServers"));
        assert!(cf.contents.contains("skylinebench"));
        assert!(cf.contents.contains("benchmark --map m"));
    }
}
