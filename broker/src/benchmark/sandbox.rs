use std::path::PathBuf;

use crate::benchmark::harness::ConfigFile;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Mac,
    Linux,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Seatbelt,
    Bubblewrap,
    Firejail,
    None,
}

impl Backend {
    pub fn as_str(self) -> &'static str {
        match self {
            Backend::Seatbelt => "seatbelt",
            Backend::Bubblewrap => "bubblewrap",
            Backend::Firejail => "firejail",
            Backend::None => "none",
        }
    }
}

pub struct SandboxInputs {
    pub os: Os,
    pub sandbox_exec_available: bool,
    pub bwrap_available: bool,
    pub firejail_available: bool,
    /// Repo root to deny reads of (the anti-cheat invariant).
    pub repo_root: PathBuf,
    pub session_dir: PathBuf,
}

pub struct SandboxPlan {
    pub backend: Backend,
    /// Wrapper argv prefix to prepend to the harness argv.
    pub wrapper_argv: Vec<String>,
    /// Profile file to write before launch, if the backend needs one.
    pub profile_file: Option<ConfigFile>,
    /// Stderr warning when anti-cheat is OFF (backend == None).
    pub warning: Option<String>,
}

/// Select a sandbox backend that preserves the deny-repo-read invariant on the
/// given host, or None (unsandboxed) with a warning when none is available.
pub fn select(inputs: &SandboxInputs) -> SandboxPlan {
    let root = inputs.repo_root.display().to_string();
    match inputs.os {
        Os::Mac if inputs.sandbox_exec_available => {
            let profile_path = inputs.session_dir.join("deny-repo.sb");
            let contents = format!("(version 1)\n(allow default)\n(deny file-read* (subpath \"{root}\"))\n");
            SandboxPlan {
                backend: Backend::Seatbelt,
                wrapper_argv: vec![
                    "sandbox-exec".to_string(),
                    "-f".to_string(),
                    profile_path.display().to_string(),
                ],
                profile_file: Some(ConfigFile { path: profile_path, contents }),
                warning: None,
            }
        }
        Os::Linux if inputs.bwrap_available => SandboxPlan {
            backend: Backend::Bubblewrap,
            wrapper_argv: vec![
                "bwrap".to_string(),
                "--dev-bind".to_string(),
                "/".to_string(),
                "/".to_string(),
                "--tmpfs".to_string(),
                root,
                "--die-with-parent".to_string(),
            ],
            profile_file: None,
            warning: None,
        },
        Os::Linux if inputs.firejail_available => SandboxPlan {
            backend: Backend::Firejail,
            wrapper_argv: vec![
                "firejail".to_string(),
                "--quiet".to_string(),
                "--noprofile".to_string(),
                format!("--blacklist={root}"),
            ],
            profile_file: None,
            warning: None,
        },
        _ => SandboxPlan {
            backend: Backend::None,
            wrapper_argv: vec![],
            profile_file: None,
            warning: Some(format!(
                "anti-cheat sandbox unavailable on this host (no sandbox-exec/bwrap/firejail) — the agent CAN read {root}; this run is NOT integrity-protected"
            )),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(os: Os) -> SandboxInputs {
        SandboxInputs {
            os,
            sandbox_exec_available: false,
            bwrap_available: false,
            firejail_available: false,
            repo_root: PathBuf::from("/repo"),
            session_dir: PathBuf::from("/sess"),
        }
    }

    #[test]
    fn mac_uses_seatbelt_with_profile() {
        let mut i = base(Os::Mac);
        i.sandbox_exec_available = true;
        let plan = select(&i);
        assert_eq!(plan.backend, Backend::Seatbelt);
        assert_eq!(plan.wrapper_argv[0], "sandbox-exec");
        let profile = plan.profile_file.unwrap();
        assert!(profile
            .contents
            .contains("(deny file-read* (subpath \"/repo\"))"));
    }

    #[test]
    fn linux_prefers_bubblewrap_then_firejail() {
        let mut i = base(Os::Linux);
        i.bwrap_available = true;
        i.firejail_available = true;
        assert_eq!(select(&i).backend, Backend::Bubblewrap);

        i.bwrap_available = false;
        let plan = select(&i);
        assert_eq!(plan.backend, Backend::Firejail);
        assert!(plan.wrapper_argv.contains(&"--blacklist=/repo".to_string()));
    }

    #[test]
    fn no_backend_warns_and_is_unsandboxed() {
        let plan = select(&base(Os::Other));
        assert_eq!(plan.backend, Backend::None);
        assert!(plan.wrapper_argv.is_empty());
        assert!(plan.warning.unwrap().contains("NOT integrity-protected"));
    }
}
