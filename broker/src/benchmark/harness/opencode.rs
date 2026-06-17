use super::{ConfigFile, LaunchInputs, LaunchSpec};

pub fn spec(inputs: &LaunchInputs) -> LaunchSpec {
    let config_path = inputs.session_dir.join("opencode.json");
    let contents = serde_json::json!({
        "$schema": "https://opencode.ai/config.json",
        "permission": "allow",
        "tools": { "webfetch": false, "websearch": false },
        "mcp": {
            "skylinebench": {
                "type": "local",
                "command": ["sh", "-c", inputs.mcp_shell],
                "enabled": true
            }
        }
    });

    let model_args = inputs
        .model
        .iter()
        .flat_map(|m| ["-m".to_string(), m.clone()]);
    let head = ["opencode", "run", inputs.prompt.as_str()].map(String::from);
    let tail = ["--format", "json", "--dangerously-skip-permissions"].map(String::from);
    let argv: Vec<String> = head.into_iter().chain(model_args).chain(tail).collect();

    LaunchSpec {
        argv,
        env: vec![(
            "OPENCODE_CONFIG".to_string(),
            config_path.display().to_string(),
        )],
        config_files: vec![ConfigFile {
            path: config_path,
            contents: serde_json::to_string_pretty(&contents)
                .expect("serde_json::Value is always serializable"),
        }],
        required_env: vec!["OPENROUTER_API_KEY".to_string()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn inputs() -> LaunchInputs {
        LaunchInputs {
            model: Some("openrouter/qwen/qwen-2.5-coder-32b-instruct".to_string()),
            prompt: "improve traffic".to_string(),
            mcp_shell: "/s/skylinebench benchmark --map m".to_string(),
            session_dir: PathBuf::from("/tmp/sess"),
        }
    }

    #[test]
    fn builds_opencode_run_argv() {
        let spec = spec(&inputs());
        assert_eq!(&spec.argv[0..2], &["opencode", "run"]);
        assert!(spec
            .argv
            .windows(2)
            .any(|w| w == ["-m", "openrouter/qwen/qwen-2.5-coder-32b-instruct"]));
        assert!(spec.argv.windows(2).any(|w| w == ["--format", "json"]));
        assert!(spec
            .argv
            .contains(&"--dangerously-skip-permissions".to_string()));
        assert_eq!(spec.required_env, vec!["OPENROUTER_API_KEY".to_string()]);
    }

    #[test]
    fn writes_config_with_mcp_and_permission_allow() {
        let spec = spec(&inputs());
        assert!(spec.env.iter().any(|(k, _)| k == "OPENCODE_CONFIG"));
        let cf = &spec.config_files[0];
        assert!(cf.path.ends_with("opencode.json"));
        assert!(cf.contents.contains("\"type\": \"local\""));
        assert!(cf.contents.contains("\"permission\": \"allow\""));
        assert!(cf.contents.contains("benchmark --map m"));
    }
}
