use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;
use std::io::BufRead;
use tracing::{info, warn};
use regex::Regex;
use std::sync::OnceLock;

#[derive(Debug, Clone)]
pub struct MaintainersEntry {
    pub subsystem: String,
    pub trees: Vec<String>,
    pub patterns: Vec<String>,
}

#[derive(Debug, PartialEq, Clone)]
pub enum BaselineResolution {
    Commit(String), // Explicit base-commit hash
    LocalRef(String), // e.g. "net-next/master" or "HEAD"
    RemoteTarget { url: String, name: String }, // e.g. url="git://...", name="net-next"
}

impl BaselineResolution {
    pub fn as_str(&self) -> String {
        match self {
            BaselineResolution::Commit(h) => h.clone(),
            BaselineResolution::LocalRef(r) => r.clone(),
            BaselineResolution::RemoteTarget { name, .. } => format!("{}/master", name),
        }
    }
}

#[derive(Debug)]
pub struct BaselineRegistry {
    entries: Vec<MaintainersEntry>,
    remote_map: HashMap<String, String>, // URL -> Local Remote Name
}

impl BaselineRegistry {
    pub fn new(repo_path: &Path) -> Result<Self> {
        let maintainers_path = repo_path.join("MAINTAINERS");
        let entries = if maintainers_path.exists() {
            info!("Loading MAINTAINERS from {:?}", maintainers_path);
            Self::parse_maintainers(&maintainers_path)?
        } else {
            warn!("MAINTAINERS file not found at {:?}, baseline detection will be limited", maintainers_path);
            Vec::new()
        };

        let remote_map = Self::load_git_remotes(repo_path).unwrap_or_default();

        Ok(Self { entries, remote_map })
    }

    fn parse_maintainers(path: &Path) -> Result<Vec<MaintainersEntry>> {
        let file = std::fs::File::open(path)?;
        let reader = std::io::BufReader::new(file);
        
        let mut entries = Vec::new();
        let mut current_subsystem = String::new();
        let mut current_trees = Vec::new();
        let mut current_patterns = Vec::new();

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                if !current_subsystem.is_empty() && (!current_trees.is_empty() || !current_patterns.is_empty()) {
                    entries.push(MaintainersEntry {
                        subsystem: current_subsystem.clone(),
                        trees: current_trees.clone(),
                        patterns: current_patterns.clone(),
                    });
                }
                current_subsystem.clear();
                current_trees.clear();
                current_patterns.clear();
                continue;
            }

            if !line.contains(':') && current_subsystem.is_empty() {
                current_subsystem = line.trim().to_string();
            } else if let Some((tag, value)) = line.split_once(':') {
                let val = value.trim();
                match tag {
                    "T" => {
                        if val.starts_with("git ") {
                            if let Some(url) = val.strip_prefix("git ") {
                                current_trees.push(url.trim().to_string());
                            }
                        }
                    },
                    "F" => {
                        current_patterns.push(val.to_string());
                    },
                    _ => {}
                }
            }
        }
        
        if !current_subsystem.is_empty() && (!current_trees.is_empty() || !current_patterns.is_empty()) {
            entries.push(MaintainersEntry {
                subsystem: current_subsystem,
                trees: current_trees,
                patterns: current_patterns,
            });
        }

        info!("Parsed {} MAINTAINERS entries", entries.len());
        Ok(entries)
    }

    fn load_git_remotes(repo_path: &Path) -> Result<HashMap<String, String>> {
        use std::process::Command;
        
        let output = Command::new("git")
            .current_dir(repo_path)
            .args(["remote", "-v"])
            .output()?;
            
        let mut map = HashMap::new();
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    let name = parts[0];
                    let url = parts[1];
                    map.insert(url.to_string(), name.to_string());
                    if let Some(stripped) = url.strip_suffix(".git") {
                        map.insert(stripped.to_string(), name.to_string());
                    }
                }
            }
        }
        Ok(map)
    }

    pub fn resolve_candidates(&self, files: &[String], subject: &str, body: Option<&str>) -> Vec<BaselineResolution> {
        let mut candidates = Vec::new();

        // 1. Explicit Base Commit
        if let Some(body_text) = body {
            if let Some(commit) = extract_base_commit(body_text) {
                candidates.push(BaselineResolution::Commit(commit));
            }
        }

        // 2. Subsystem Heuristic
        if let Some(subsystem) = self.resolve_subsystem_heuristic(files, subject) {
            candidates.push(subsystem);
        }

        // 3. Linux Next
        // Hardcoded linux-next URL
        let linux_next_url = "https://git.kernel.org/pub/scm/linux/kernel/git/next/linux-next.git";
        candidates.push(self.resolve_url(linux_next_url));

        // 4. Mainline (Local Origin/Master or HEAD)
        // We assume 'origin' is mainline if available, or just HEAD.
        // Or if we can find 'torvalds/linux.git' in remote map.
        // For simplicity: HEAD.
        candidates.push(BaselineResolution::LocalRef("HEAD".to_string()));

        // Deduplicate
        // Simple deduplication based on string representation or enum equality
        // Since we implement PartialEq, dedup works if consecutive. We need unique.
        let mut unique_candidates = Vec::new();
        for c in candidates {
            if !unique_candidates.contains(&c) {
                unique_candidates.push(c);
            }
        }

        unique_candidates
    }

    fn resolve_subsystem_heuristic(&self, files: &[String], subject: &str) -> Option<BaselineResolution> {
        let mut tree_counts: HashMap<String, usize> = HashMap::new();
        
        for file in files {
            for entry in &self.entries {
                let mut matched = false;
                for pattern in &entry.patterns {
                    if pattern.ends_with('/') {
                        if file.starts_with(pattern) {
                            matched = true;
                            break;
                        }
                    } else if pattern == file {
                         matched = true;
                         break;
                    }
                }
                
                if matched {
                    for tree in &entry.trees {
                        *tree_counts.entry(tree.clone()).or_insert(0) += 1;
                    }
                }
            }
        }

        if tree_counts.is_empty() {
            return None;
        }

        let mut candidates: Vec<(&String, &usize)> = tree_counts.iter().collect();
        candidates.sort_by(|a, b| b.1.cmp(a.1));

        let subject_lower = subject.to_lowercase();
        let keywords = ["net", "bpf", "drm", "mm", "sched", "x86", "arm", "arm64", "scsi", "usb"];
        let is_next = subject_lower.contains("next");

        for (url, _) in &candidates {
            for kw in keywords {
                if subject_lower.contains(kw) && url.contains(kw) {
                    if is_next && url.contains("next") {
                         return Some(self.resolve_url(url));
                    }
                    if !is_next && !url.contains("next") {
                        // Prefer non-next if subject doesn't say next? 
                    }
                }
            }
            if is_next && url.contains("next") {
                return Some(self.resolve_url(url));
            }
        }

        Some(self.resolve_url(candidates[0].0))
    }

    fn resolve_url(&self, url: &str) -> BaselineResolution {
        if let Some(remote_name) = self.remote_map.get(url) {
            BaselineResolution::LocalRef(format!("{}/master", remote_name))
        } else {
            let name = self.suggest_remote_name(url);
            BaselineResolution::RemoteTarget {
                url: url.to_string(),
                name,
            }
        }
    }

    fn suggest_remote_name(&self, url: &str) -> String {
        let path = url.trim_end_matches('/');
        let name = path.rsplit('/').next().unwrap_or("unknown");
        let name = name.strip_suffix(".git").unwrap_or(name);
        
        if name == "linux" {
            let parts: Vec<&str> = path.split('/').collect();
            if parts.len() >= 2 {
                return parts[parts.len() - 2].to_string();
            }
        }
        name.to_string()
    }
}

pub fn extract_files_from_diff(diff: &str) -> Vec<String> {
    let mut files = Vec::new();
    for line in diff.lines() {
        if let Some(path) = line.strip_prefix("diff --git a/") {
            if let Some((a, _)) = path.split_once(' ') {
                files.push(a.to_string());
            }
        }
    }
    files
}

pub fn extract_base_commit(body: &str) -> Option<String> {
    static BASE_COMMIT_RE: OnceLock<Regex> = OnceLock::new();
    let re = BASE_COMMIT_RE.get_or_init(|| Regex::new(r"(?m)^base-commit: ([0-9a-f]{40})").unwrap());
    re.captures(body).and_then(|caps| caps.get(1).map(|m| m.as_str().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_registry() -> BaselineRegistry {
        let mut entries = Vec::new();
        entries.push(MaintainersEntry {
            subsystem: "NETWORKING".to_string(),
            trees: vec!["git://net-next.git".to_string()],
            patterns: vec!["net/".to_string()],
        });
        let mut remote_map = HashMap::new();
        remote_map.insert("git://net-next.git".to_string(), "net-next".to_string());
        BaselineRegistry { entries, remote_map }
    }

    #[test]
    fn test_resolve_candidates() {
        let registry = create_registry();
        let files = vec!["net/core.c".to_string()];
        let body = "Some text\nbase-commit: 1234567890123456789012345678901234567890\n";
        
        let candidates = registry.resolve_candidates(&files, "Subject", Some(body));
        
        assert_eq!(candidates.len(), 4); // Base, Subsystem, Next, Head -> Next is RemoteTarget(linux-next), Subsystem is Local(net-next).
        // Wait, Next is hardcoded URL.
        // 1. Commit(123...)
        // 2. LocalRef(net-next/master)
        // 3. RemoteTarget(linux-next)
        // 4. LocalRef(HEAD)
        
        // Actually 4.
        assert_eq!(candidates[0], BaselineResolution::Commit("1234567890123456789012345678901234567890".to_string()));
        
        match &candidates[1] {
            BaselineResolution::LocalRef(r) => assert_eq!(r, "net-next/master"),
            _ => panic!("Expected LocalRef net-next"),
        }
    }
}