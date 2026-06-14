use std::path::PathBuf;

/// Which agent CLI drives the benchmark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Harness {
    Claude,
    Codex,
    Gemini,
    Opencode,
}

impl Harness {
    pub fn parse(s: &str) -> Option<Harness> {
        match s {
            "claude" => Some(Harness::Claude),
            "codex" => Some(Harness::Codex),
            "gemini" => Some(Harness::Gemini),
            "opencode" => Some(Harness::Opencode),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Harness::Claude => "claude",
            Harness::Codex => "codex",
            Harness::Gemini => "gemini",
            Harness::Opencode => "opencode",
        }
    }
}

/// Everything a harness builder needs to construct its launch command.
pub struct LaunchInputs {
    pub model: Option<String>,
    pub prompt: String,
    /// The shell string that starts the MCP broker (passed to `sh -c`).
    pub mcp_shell: String,
    /// Per-run scratch dir, outside the repo, where config files are written.
    pub session_dir: PathBuf,
}

/// A config file the harness needs on disk before launch.
pub struct ConfigFile {
    pub path: PathBuf,
    pub contents: String,
}

/// The fully-resolved plan for launching one harness.
pub struct LaunchSpec {
    /// Harness CLI + args (prompt included). NO sandbox wrapper.
    pub argv: Vec<String>,
    /// Isolation env to export (e.g. CODEX_HOME). Never secrets.
    pub env: Vec<(String, String)>,
    /// Config files to write into session_dir.
    pub config_files: Vec<ConfigFile>,
    /// Secret env vars that must already be set (preflight).
    pub required_env: Vec<String>,
}

mod claude;
mod codex;

/// Build the launch plan for a harness.
pub fn build(harness: Harness, inputs: &LaunchInputs) -> LaunchSpec {
    match harness {
        Harness::Claude => claude::spec(inputs),
        // Added in later phases:
        Harness::Codex => codex::spec(inputs),
        Harness::Gemini => todo!("gemini builder — Task 14"),
        Harness::Opencode => todo!("opencode builder — Task 16"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_harnesses_and_rejects_unknown() {
        assert_eq!(Harness::parse("claude"), Some(Harness::Claude));
        assert_eq!(Harness::parse("codex"), Some(Harness::Codex));
        assert_eq!(Harness::parse("gemini"), Some(Harness::Gemini));
        assert_eq!(Harness::parse("opencode"), Some(Harness::Opencode));
        assert_eq!(Harness::parse("gpt"), None);
        assert_eq!(Harness::Codex.as_str(), "codex");
    }
}
