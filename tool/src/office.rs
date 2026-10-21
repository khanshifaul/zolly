//! The office in front of the binary. One binary serves every office on the
//! machine, so nothing below names a project: the frequency, project name,
//! founder, base branch and remote are read from `.agents/zolly/config.toml`,
//! with the directory name as the fallback. This is the only place that knows
//! how to tell two offices apart.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const CONFIG_PATH: &str = ".agents/zolly/config.toml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Office {
    pub root: PathBuf,
    pub frequency: String,
    pub project: String,
    pub founder: String,
    pub base_branch: String,
    pub remote: String,
}

impl Office {
    pub fn load(root: PathBuf) -> Self {
        let config = read_config(&root.join(CONFIG_PATH));
        let dir = root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let pick = |key: &str, env: &str, default: &str| -> String {
            std::env::var(env)
                .ok()
                .filter(|v| !v.is_empty())
                .or_else(|| config.get(key).cloned().filter(|v| !v.is_empty()))
                .unwrap_or_else(|| default.to_string())
        };
        Self {
            frequency: pick("frequency", "ZOLLY_FREQUENCY", &dir),
            project: pick("project", "ZOLLY_PROJECT", &dir),
            founder: pick("founder", "ZOLLY_FOUNDER", ""),
            base_branch: pick("base_branch", "ZOLLY_BASE_BRANCH", "main"),
            remote: pick("remote", "ZOLLY_REMOTE", "origin"),
            root,
        }
    }

    pub fn config_exists(&self) -> bool {
        self.root.join(CONFIG_PATH).is_file()
    }

    /// herdr agent name. Agent names are unique server-wide, so the frequency
    /// is part of the name; the radio handle stays bare because the bus is
    /// already scoped by frequency.
    pub fn agent(&self, role: &str) -> String {
        format!("{role}-{}", self.frequency)
    }

    /// Radio ledger workspace: the plugin prefixes a named frequency with
    /// `freq.`.
    pub fn workspace(&self) -> String {
        format!("freq.{}", self.frequency)
    }

    /// systemd user unit that keeps this office's listener alive.
    pub fn unit(&self) -> String {
        format!("zolly@{}.service", self.frequency)
    }

    /// The persona file for a seat (`zolly`, `foreman`, `reviewer`).
    pub fn agent_md(&self, seat: &str) -> PathBuf {
        self.root.join(format!(".agents/{seat}/agent.md"))
    }

    pub fn root_str(&self) -> String {
        self.root.to_string_lossy().into_owned()
    }
}

fn read_config(path: &Path) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Ok(content) = std::fs::read_to_string(path) else {
        return map;
    };
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let value = value.trim().trim_matches('"').trim_matches('\'');
            map.insert(key.trim().to_string(), value.to_string());
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_office(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "zolly-office-test-{}-{}",
            name,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".agents/zolly")).unwrap();
        dir
    }

    #[test]
    fn falls_back_to_the_directory_name() {
        let root = temp_office("fallback");
        let office = Office::load(root.clone());
        let dir_name = root.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(office.frequency, dir_name);
        assert_eq!(office.project, dir_name);
        assert_eq!(office.base_branch, "main");
        assert_eq!(office.remote, "origin");
        assert!(!office.config_exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn config_wins_over_the_directory_name() {
        let root = temp_office("dir-name");
        std::fs::write(
            root.join(CONFIG_PATH),
            "frequency = \"other-office\"\nfounder = \"Founder\"\nproject = \"Other\"\n",
        )
        .unwrap();
        let office = Office::load(root.clone());
        assert_eq!(office.frequency, "other-office");
        assert_eq!(office.project, "Other");
        assert_eq!(office.founder, "Founder");
        assert_eq!(office.agent("foreman"), "foreman-other-office");
        assert_eq!(office.workspace(), "freq.other-office");
        assert_eq!(office.unit(), "zolly@other-office.service");
        assert!(office.config_exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
