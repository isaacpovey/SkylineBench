use super::{ConfigFile, LaunchInputs, LaunchSpec};

pub fn spec(inputs: &LaunchInputs) -> LaunchSpec {
    let config_path = inputs.session_dir.join("codex").join("config.toml");
    // sh -c "<mcp_shell>" as the stdio MCP server.
    let contents = format!(
        concat!(
            "[mcp_servers.skylinebench]\n",
            "command = \"sh\"\n",
            "args = [\"-c\", {}]\n",
            "required = true\n",
            "default_tools_approval_mode = \"approve\"\n",
            "enabled_tools = [{}]\n",
        ),
        toml_string(&inputs.mcp_shell),
        TOOL_ALLOWLIST
            .iter()
            .map(|t| toml_string(t))
            .collect::<Vec<_>>()
            .join(", "),
    );

    let model_args = inputs
        .model
        .iter()
        .flat_map(|m| ["-m".to_string(), m.clone()]);
    let head = ["codex", "-a", "never", "exec"].map(String::from);
    let tail = [
        "--skip-git-repo-check",
        "-s",
        "workspace-write",
        "--json",
        inputs.prompt.as_str(),
    ]
    .map(String::from);
    let argv: Vec<String> = head.into_iter().chain(model_args).chain(tail).collect();

    LaunchSpec {
        argv,
        env: vec![(
            "CODEX_HOME".to_string(),
            inputs.session_dir.join("codex").display().to_string(),
        )],
        config_files: vec![ConfigFile {
            path: config_path,
            contents,
        }],
        required_env: vec!["CODEX_API_KEY".to_string()],
    }
}

const TOOL_ALLOWLIST: &[&str] = &[
    "build_road",
    "bulldoze",
    "upgrade_road",
    "set_zoning",
    "control_time",
    "get_city_overview",
    "observe_area",
    "get_metrics",
    "list_road_types",
    "list_zone_types",
    "render_map",
    "submit_solution",
    "query_problems",
    "query_segments",
    "apply_plan",
    "trace_route",
    "validate_road",
];

/// Minimal TOML basic-string encoder (escape backslash and quote).
fn toml_string(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn inputs() -> LaunchInputs {
        LaunchInputs {
            model: Some("gpt-5.5".to_string()),
            prompt: "improve traffic".to_string(),
            mcp_shell: "/s/skylinebench benchmark --map m".to_string(),
            session_dir: PathBuf::from("/tmp/sess"),
        }
    }

    #[test]
    fn builds_codex_exec_argv() {
        let spec = spec(&inputs());
        assert_eq!(&spec.argv[0..4], &["codex", "-a", "never", "exec"]);
        assert_eq!(spec.argv.last(), Some(&"improve traffic".to_string()));
        assert!(spec.argv.windows(2).any(|w| w == ["-m", "gpt-5.5"]));
        assert!(spec.argv.contains(&"--json".to_string()));
        assert!(spec.argv.windows(2).any(|w| w == ["-s", "workspace-write"]));
        assert_eq!(spec.required_env, vec!["CODEX_API_KEY".to_string()]);
    }

    #[test]
    fn isolates_codex_home_and_writes_config_toml() {
        let spec = spec(&inputs());
        assert!(spec
            .env
            .iter()
            .any(|(k, v)| k == "CODEX_HOME" && v.ends_with("codex")));
        let cf = &spec.config_files[0];
        assert!(cf.path.ends_with("config.toml"));
        assert!(cf.contents.contains("[mcp_servers.skylinebench]"));
        assert!(cf.contents.contains("benchmark --map m"));
        assert!(cf.contents.contains("required = true"));
        assert!(cf
            .contents
            .contains("default_tools_approval_mode = \"approve\""));
        assert!(cf.contents.contains("\"get_city_overview\""));
        assert!(cf.contents.contains("\"query_problems\""));
        assert!(cf.contents.contains("\"submit_solution\""));
        assert!(cf.contents.contains("\"validate_road\""));
    }
}
