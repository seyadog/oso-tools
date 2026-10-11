//! The cloud settings a profile can ask for in its local terminals: which AWS profile (`AWS_PROFILE`) and which
//! kubeconfig (`KUBECONFIG`). oso reads only the *names* of what exists, to offer them in a list; it never reads
//! or keeps a key, a token or a certificate.

use serde::Serialize;
use std::path::{Path, PathBuf};

/// The profile names of `~/.aws/config` (`[default]`, `[profile x]`) and `~/.aws/credentials` (`[x]`).
pub fn parse_aws_profiles(config: &str, credentials: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let mut add = |n: &str| {
        let n = n.trim();
        if !n.is_empty() && !out.iter().any(|x| x == n) {
            out.push(n.to_string());
        }
    };
    for line in config.lines() {
        let l = line.trim();
        let Some(inner) = l.strip_prefix('[').and_then(|r| r.strip_suffix(']')) else { continue };
        let inner = inner.trim();
        if inner == "default" {
            add("default");
        } else if let Some(name) = inner.strip_prefix("profile ") {
            add(name);
        }
        // `[sso-session x]`, `[services x]`… are not profiles
    }
    for line in credentials.lines() {
        if let Some(inner) = line.trim().strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            add(inner);
        }
    }
    out
}

/// The context names of a kubeconfig, without reading anything under them.
pub fn parse_kube_contexts(text: &str) -> Vec<String> {
    let mut out = vec![];
    let mut in_contexts = false;
    for line in text.lines() {
        if !line.starts_with(' ') && !line.starts_with('-') && !line.trim().is_empty() {
            in_contexts = line.trim_start().starts_with("contexts:");
            continue;
        }
        if !in_contexts {
            continue;
        }
        let l = line.trim_start_matches([' ', '-']).trim();
        // the context's own name sits at the same depth as `context:`; a cluster named inside it is deeper
        if let Some(n) = l.strip_prefix("name:") {
            if line.trim_start().starts_with("- name:") || line.starts_with("  name:") || line.starts_with("    name:") && !line.starts_with("      ") {
                let n = n.trim().trim_matches(['"', '\'']);
                if !n.is_empty() {
                    out.push(n.to_string());
                }
            }
        }
    }
    out
}

#[derive(Serialize, Clone, Debug, PartialEq, Default)]
pub struct Kubeconfig {
    pub path: String,
    pub contexts: Vec<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Default)]
pub struct CloudOptions {
    pub aws_profiles: Vec<String>,
    pub kubeconfigs: Vec<Kubeconfig>,
}

/// What exists under `home` (and in `$KUBECONFIG`), by name.
pub fn options(home: &Path, kubeconfig_env: Option<&str>) -> CloudOptions {
    let read = |p: PathBuf| std::fs::read_to_string(p).unwrap_or_default();
    let aws_profiles = parse_aws_profiles(&read(home.join(".aws/config")), &read(home.join(".aws/credentials")));
    let mut paths: Vec<PathBuf> = kubeconfig_env.map(|v| std::env::split_paths(v).collect()).unwrap_or_default();
    paths.push(home.join(".kube/config"));
    if let Ok(dir) = std::fs::read_dir(home.join(".kube")) {
        let mut more: Vec<PathBuf> = dir.flatten().map(|e| e.path()).filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "yaml" || x == "yml")).collect();
        more.sort();
        paths.extend(more);
    }
    let mut kubeconfigs: Vec<Kubeconfig> = vec![];
    for p in paths {
        if kubeconfigs.iter().any(|k| Path::new(&k.path) == p) || !p.is_file() {
            continue;
        }
        let contexts = parse_kube_contexts(&read(p.clone()));
        if !contexts.is_empty() {
            kubeconfigs.push(Kubeconfig { path: p.to_string_lossy().into_owned(), contexts });
        }
    }
    CloudOptions { aws_profiles, kubeconfigs }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AWS_CONFIG: &str = "[default]\nregion = eu-west-1\n\n[profile acme-prod]\nsso_session = acme\nsso_account_id = 111\n\n[sso-session acme]\nsso_start_url = https://x.awsapps.com/start\n\n[profile  spaced ]\nregion=x\n\n[services s3-special]\nx = y\n";
    const AWS_CREDENTIALS: &str = "[default]\naws_access_key_id = AKIATRAPTRAPTRAP\naws_secret_access_key = TRAPSECRETTRAPSECRET\n[legacy]\naws_access_key_id = AKIAOTHERTRAP\n";
    const KUBE: &str = "apiVersion: v1\nclusters:\n- cluster:\n    server: https://k.example\n    certificate-authority-data: TRAPCERTDATA\n  name: prod-cluster\ncontexts:\n- context:\n    cluster: prod-cluster\n    user: me\n  name: prod\n- context:\n    cluster: dev-cluster\n    user: me\n  name: \"dev\"\ncurrent-context: prod\nkind: Config\nusers:\n- name: me\n  user:\n    token: TRAPTOKEN\n";

    #[test]
    fn aws_profiles_come_from_both_files_by_name_only() {
        let p = parse_aws_profiles(AWS_CONFIG, AWS_CREDENTIALS);
        assert_eq!(p, vec!["default", "acme-prod", "spaced", "legacy"], "sessions and services are not profiles; a name appears once");
        assert!(!format!("{p:?}").contains("TRAP"));
    }

    #[test]
    fn kube_contexts_are_the_names_under_contexts_only() {
        let c = parse_kube_contexts(KUBE);
        assert_eq!(c, vec!["prod", "dev"], "not the cluster, not the user");
        assert!(parse_kube_contexts("not yaml at all").is_empty());
    }

    #[test]
    fn the_options_list_what_exists_and_nothing_secret() {
        let home = std::env::temp_dir().join(format!("oso-cloud-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".aws")).unwrap();
        std::fs::create_dir_all(home.join(".kube")).unwrap();
        std::fs::write(home.join(".aws/config"), AWS_CONFIG).unwrap();
        std::fs::write(home.join(".aws/credentials"), AWS_CREDENTIALS).unwrap();
        std::fs::write(home.join(".kube/config"), KUBE).unwrap();
        std::fs::write(home.join(".kube/staging.yaml"), KUBE.replace("prod", "stg")).unwrap();
        std::fs::write(home.join(".kube/notes.txt"), "x").unwrap();
        let o = options(&home, None);
        assert_eq!(o.aws_profiles.len(), 4);
        assert_eq!(o.kubeconfigs.len(), 2);
        assert_eq!(o.kubeconfigs[1].contexts, vec!["stg", "dev"]);
        assert!(!serde_json::to_string(&o).unwrap().contains("TRAP"));
        // a path in KUBECONFIG is listed first, once
        let kc = home.join(".kube/config").to_string_lossy().into_owned();
        let o2 = options(&home, Some(&kc));
        assert_eq!(o2.kubeconfigs.len(), 2);
        std::fs::remove_dir_all(home).ok();
    }
}
