//! The office in front of the binary. One binary serves every office on the
//! machine, so nothing below names a project: the frequency, project name,
//! founder, base branch and remote are read from `.agents/zolly/config.toml`,
//! with the directory name as the fallback. This is the only place that knows
//! how to tell two offices apart.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const CONFIG_PATH: &str = ".agents/zolly/config.toml";

/// Frequency is an identity and a path component (`~/.config/zolly/<freq>`),
/// so it cannot be trusted from a cloned repo's tracked config. Match the
/// installer's rule: lowercase, `[a-z0-9_-]`, trimmed, at most 24 chars.
fn sanitize_frequency(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_' {
            out.push(c);
        } else {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    let out = out.chars().take(24).collect::<String>();
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() {
        "zolly".to_string()
    } else {
        out
    }
}

fn valid_branch(raw: &str) -> bool {
    !raw.is_empty()
        && !raw.starts_with('-')
        && !raw.contains("..")
        && raw
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-'))
}

fn valid_remote(raw: &str) -> bool {
    !raw.is_empty()
        && !raw.starts_with('-')
        && raw
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

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
        let base = pick("base_branch", "ZOLLY_BASE_BRANCH", "main");
        let remote = pick("remote", "ZOLLY_REMOTE", "origin");
        Self {
            frequency: sanitize_frequency(&pick("frequency", "ZOLLY_FREQUENCY", &dir)),
            project: pick("project", "ZOLLY_PROJECT", &dir),
            founder: pick("founder", "ZOLLY_FOUNDER", ""),
            base_branch: if valid_branch(&base) {
                base
            } else {
                "main".to_string()
            },
            remote: if valid_remote(&remote) {
                remote
            } else {
                "origin".to_string()
            },
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

    /// `remote/base_branch`, the configured integration ref.
    pub fn integration_ref(&self) -> String {
        format!("{}/{}", self.remote, self.base_branch)
    }

    /// Remote-tracking ref first, then the local base branch for a repo with
    /// no usable remote (a fresh local checkout, a test fixture).
    pub fn integration_refs(&self) -> [String; 2] {
        [self.integration_ref(), self.base_branch.clone()]
    }

    /// The main checkout for this office. A linked worktree's `.git` is a file
    /// pointing at `<main>/.git/worktrees/<name>`; walk back to `<main>`.
    /// Everything else is already the main checkout.
    pub fn main_root(&self) -> PathBuf {
        let root = self.root.canonicalize().unwrap_or_else(|_| self.root.clone());
        let dotgit = root.join(".git");
        if !dotgit.is_file() {
            return root;
        }
        let Ok(text) = std::fs::read_to_string(&dotgit) else {
            return root;
        };
        let Some(raw) = text.trim().strip_prefix("gitdir:") else {
            return root;
        };
        let raw = PathBuf::from(raw.trim());
        let gitdir = if raw.is_absolute() {
            raw
        } else {
            root.join(raw)
        };
        let gitdir = gitdir.canonicalize().unwrap_or(gitdir);
        gitdir
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or(root)
    }

    /// True when the installer's ownership marker names this checkout. A
    /// missing marker is allowed for legacy/uninstalled trees; a present one
    /// that names another root is a cloned office impersonating it.
    pub fn owned(&self) -> bool {
        match dirs::home_dir() {
            Some(home) => self.owned_with_home(&home),
            None => true,
        }
    }

    fn owned_with_home(&self, home: &Path) -> bool {
        let marker = home
            .join(".config/zolly")
            .join(&self.frequency)
            .join("project");
        if !marker.is_file() {
            return true;
        }
        let Ok(recorded) = std::fs::read_to_string(&marker) else {
            return false;
        };
        let recorded = PathBuf::from(recorded.trim());
        let recorded = recorded.canonicalize().unwrap_or(recorded);
        let main = self.main_root();
        recorded == main
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
        let expected: String = dir_name.chars().take(24).collect();
        assert_eq!(office.frequency, expected.trim_end_matches('-'));
        assert_eq!(office.project, dir_name);
        assert_eq!(office.base_branch, "main");
        assert_eq!(office.remote, "origin");
        assert!(!office.config_exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn frequency_cannot_escape_the_credentials_directory() {
        let root = temp_office("unsafe-frequency");
        std::fs::write(
            root.join(CONFIG_PATH),
            "frequency = \"../../.config/zolly/other\"\n",
        )
        .unwrap();
        let office = Office::load(root.clone());
        assert!(!office.frequency.contains('/'));
        assert!(!office.frequency.contains('.'));
        assert_eq!(office.frequency, "config-zolly-other");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn ownership_marker_names_this_checkout() {
        let root = temp_office("ownership");
        let home = std::env::temp_dir().join(format!(
            "zolly-office-home-{}-{}",
            std::process::id(),
            "ownership"
        ));
        let _ = std::fs::remove_dir_all(&home);
        let office = Office::load(root.clone());
        let marker = home
            .join(".config/zolly")
            .join(&office.frequency)
            .join("project");
        std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
        std::fs::write(&marker, root.canonicalize().unwrap().to_string_lossy().as_bytes())
            .unwrap();
        assert!(office.owned_with_home(&home));
        std::fs::write(&marker, "/some/other/checkout\n").unwrap();
        assert!(!office.owned_with_home(&home));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn unsafe_refs_fall_back_to_defaults() {
        let root = temp_office("unsafe-refs");
        std::fs::write(
            root.join(CONFIG_PATH),
            "base_branch = \"--upload-pack=evil\"\nremote = \"-x\"\n",
        )
        .unwrap();
        let office = Office::load(root.clone());
        assert_eq!(office.base_branch, "main");
        assert_eq!(office.remote, "origin");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn config_wins_over_the_directory_name() {
        let root = temp_office("dir-name");
        std::fs::write(
            root.join(CONFIG_PATH),
            "frequency = \"other-office\"\nfounder = \"Founder\"\nproject = \"Other\"\nbase_branch = \"master\"\nremote = \"upstream\"\n",
        )
        .unwrap();
        let office = Office::load(root.clone());
        assert_eq!(office.frequency, "other-office");
        assert_eq!(office.project, "Other");
        assert_eq!(office.founder, "Founder");
        assert_eq!(office.base_branch, "master");
        assert_eq!(office.remote, "upstream");
        assert_eq!(office.integration_ref(), "upstream/master");
        assert_eq!(office.integration_refs(), ["upstream/master".to_string(), "master".to_string()]);
        assert_eq!(office.agent("foreman"), "foreman-other-office");
        assert_eq!(office.workspace(), "freq.other-office");
        assert_eq!(office.unit(), "zolly@other-office.service");
        assert!(office.config_exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
