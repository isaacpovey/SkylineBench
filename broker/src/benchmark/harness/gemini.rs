use super::{ConfigFile, LaunchInputs, LaunchSpec};

pub fn spec(inputs: &LaunchInputs) -> LaunchSpec {
    let gemini_dir = inputs.session_dir.join("gemini");
    let settings_path = gemini_dir.join(".gemini").join("settings.json");
    let contents = serde_json::json!({
        "mcpServers": {
            "skylinebench": {
                "command": "sh",
                "args": ["-c", inputs.mcp_shell],
                "excludeTools": ["web_fetch", "google_web_search"]
            }
        }
    });

    let model_args = inputs.model.iter().flat_map(|m| ["-m".to_string(), m.clone()]);
    let head = ["gemini", "-p", inputs.prompt.as_str()].map(String::from);
    let tail = ["--approval-mode", "yolo", "--output-format", "stream-json"].map(String::from);
    let argv: Vec<String> = head.into_iter().chain(model_args).chain(tail).collect();

    LaunchSpec {
        argv,
        env: vec![
            ("GEMINI_CLI_HOME".to_string(), gemini_dir.display().to_string()),
            ("GEMINI_CLI_TRUST_WORKSPACE".to_string(), "true".to_string()),
        ],
        config_files: vec![ConfigFile {
            path: settings_path,
            contents: serde_json::to_string_pretty(&contents)
                .expect("serde_json::Value is always serializable"),
        }],
        required_env: vec!["GEMINI_API_KEY".to_string()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn inputs() -> LaunchInputs {
        LaunchInputs {
            model: Some("gemini-2.5-pro".to_string()),
            prompt: "improve traffic".to_string(),
            mcp_shell: "/s/skylinebench benchmark --map m".to_string(),
            session_dir: PathBuf::from("/tmp/sess"),
        }
    }

    #[test]
    fn builds_gemini_argv() {
        let spec = spec(&inputs());
        assert_eq!(spec.argv[0], "gemini");
        assert!(spec.argv.windows(2).any(|w| w == ["-p", "improve traffic"]));
        assert!(spec.argv.windows(2).any(|w| w == ["-m", "gemini-2.5-pro"]));
        assert!(spec.argv.windows(2).any(|w| w == ["--approval-mode", "yolo"]));
        assert!(spec.argv.contains(&"stream-json".to_string()));
        assert_eq!(spec.required_env, vec!["GEMINI_API_KEY".to_string()]);
    }

    #[test]
    fn isolates_home_and_writes_settings() {
        let spec = spec(&inputs());
        assert!(spec.env.iter().any(|(k, _)| k == "GEMINI_CLI_HOME"));
        assert!(spec.env.iter().any(|(k, v)| k == "GEMINI_CLI_TRUST_WORKSPACE" && v == "true"));
        let cf = &spec.config_files[0];
        assert!(cf.path.ends_with("settings.json"));
        assert!(cf.contents.contains("mcpServers"));
        assert!(cf.contents.contains("excludeTools"));
    }
}
