use super::{ConfigFile, LaunchInputs, LaunchSpec};

pub fn spec(inputs: &LaunchInputs) -> LaunchSpec {
    let config_path = inputs.session_dir.join("codex").join("config.toml");
    // sh -c "<mcp_shell>" as the stdio MCP server.
    let contents = format!(
        "[mcp_servers.skylinebench]\ncommand = \"sh\"\nargs = [\"-c\", {}]\n",
        toml_string(&inputs.mcp_shell),
    );

    let model_args = inputs.model.iter().flat_map(|m| ["-m".to_string(), m.clone()]);
    let head = ["codex", "exec", inputs.prompt.as_str()].map(String::from);
    let tail = ["-a", "never", "-s", "workspace-write", "--json"].map(String::from);
    let argv: Vec<String> = head.into_iter().chain(model_args).chain(tail).collect();

    LaunchSpec {
        argv,
        env: vec![(
            "CODEX_HOME".to_string(),
            inputs.session_dir.join("codex").display().to_string(),
        )],
        config_files: vec![ConfigFile { path: config_path, contents }],
        required_env: vec!["OPENAI_API_KEY".to_string()],
    }
}

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
        assert_eq!(&spec.argv[0..2], &["codex", "exec"]);
        assert!(spec.argv.contains(&"improve traffic".to_string()));
        assert!(spec.argv.windows(2).any(|w| w == ["-m", "gpt-5.5"]));
        assert!(spec.argv.contains(&"--json".to_string()));
        assert!(spec.argv.windows(2).any(|w| w == ["-a", "never"]));
        assert_eq!(spec.required_env, vec!["OPENAI_API_KEY".to_string()]);
    }

    #[test]
    fn isolates_codex_home_and_writes_config_toml() {
        let spec = spec(&inputs());
        assert!(spec.env.iter().any(|(k, v)| k == "CODEX_HOME" && v.ends_with("codex")));
        let cf = &spec.config_files[0];
        assert!(cf.path.ends_with("config.toml"));
        assert!(cf.contents.contains("[mcp_servers.skylinebench]"));
        assert!(cf.contents.contains("benchmark --map m"));
    }
}
