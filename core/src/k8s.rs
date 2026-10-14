//! Kubernetes clusters: where `kubectl` runs and how it is pointed at the cluster. A cluster is either reached from this
//! computer (a kubeconfig file, optionally a context and namespace) or through a server of oso over ssh (kubectl, or
//! `k3s kubectl`, runs there). Saved in `clusters.json`. No kubeconfig content is kept here: only the path to it.

use crate::policy::AiPolicy;
use crate::store::EnvKind;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Cluster {
    /// 0 when new: the engine gives it one.
    #[serde(default)]
    pub id: u64,
    pub name: String,
    /// Run kubectl on this server of oso (over ssh) instead of on this computer.
    #[serde(default)]
    pub via_server: Option<u64>,
    /// The program: `kubectl` (the default when empty), or e.g. `k3s kubectl`.
    #[serde(default)]
    pub kubectl: String,
    /// Path to a kubeconfig on this computer (only when not through a server; empty = kubectl's default).
    #[serde(default)]
    pub kubeconfig: String,
    #[serde(default)]
    pub context: String,
    #[serde(default)]
    pub namespace: String,
    /// What an AI agent may do on a cluster reached from this computer (through a server, the server's rule applies).
    #[serde(default)]
    pub ai_policy: AiPolicy,
    #[serde(default)]
    pub env_kind: Option<EnvKind>,
    #[serde(default)]
    pub notes: String,
    /// Through a server: the port of the cluster's API on that server (0 = 6443, the one of k3s and kubeadm).
    #[serde(default)]
    pub api_port: u16,
    /// The port the tunnel opens on this computer (0 = one that depends on the cluster, so two clusters never clash).
    #[serde(default)]
    pub local_port: u16,
}

/// Where the cluster's API is on its server.
pub fn api_port(c: &Cluster) -> u16 {
    if c.api_port == 0 { 6443 } else { c.api_port }
}

/// The port of the tunnel on this computer.
pub fn local_port(c: &Cluster) -> u16 {
    if c.local_port != 0 { c.local_port } else { 16443u32.saturating_add(c.id as u32).min(65535) as u16 }
}

/// The address a kubeconfig on this computer has to use while the tunnel is on.
pub fn tunnel_url(c: &Cluster) -> String {
    format!("https://127.0.0.1:{}", local_port(c))
}

/// Ids of the tunnels of clusters are this plus the cluster's id, so they never meet a saved tunnel's.
pub const TUNNEL_BASE: u64 = 1 << 40;

#[derive(Serialize, Deserialize, Default)]
struct File {
    #[serde(default)]
    next_id: u64,
    #[serde(default)]
    clusters: Vec<Cluster>,
}

pub struct Clusters {
    path: PathBuf,
    file: File,
}

impl Clusters {
    pub fn load_from(dir: &Path) -> Clusters {
        let path = dir.join("clusters.json");
        let file = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        Clusters { path, file }
    }

    pub fn list(&self) -> &[Cluster] {
        &self.file.clusters
    }

    pub fn get(&self, id: u64) -> Option<&Cluster> {
        self.file.clusters.iter().find(|c| c.id == id)
    }

    /// By exact name (any case), else by a part of it when only one matches.
    pub fn find(&self, query: &str) -> Result<&Cluster, String> {
        let q = query.trim().to_lowercase();
        if let Some(c) = self.file.clusters.iter().find(|c| c.name.to_lowercase() == q) {
            return Ok(c);
        }
        let hits: Vec<&Cluster> = self.file.clusters.iter().filter(|c| c.name.to_lowercase().contains(&q)).collect();
        match hits.as_slice() {
            [one] => Ok(one),
            [] => Err(format!("There is no cluster called {query:?}. oso_context lists them.")),
            many => Err(format!("{query:?} matches {}: say which one.", many.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(", "))),
        }
    }

    /// Adds (id 0) or replaces a cluster. Gives back its id.
    pub fn save(&mut self, mut c: Cluster) -> Result<u64> {
        c.name = c.name.trim().to_string();
        if c.name.is_empty() {
            bail!("The cluster needs a name.");
        }
        if self.file.clusters.iter().any(|o| o.id != c.id && o.name.eq_ignore_ascii_case(&c.name)) {
            bail!("There is already a cluster called {:?}.", c.name);
        }
        for (what, v) in [("kubectl", &c.kubectl), ("context", &c.context), ("namespace", &c.namespace), ("kubeconfig", &c.kubeconfig)] {
            if v.contains(['\0', '\n', '\r']) {
                bail!("The {what} has a character that is not allowed.");
            }
        }
        if c.via_server.is_some() {
            c.kubeconfig.clear();
        }
        if c.id == 0 {
            self.file.next_id = self.file.next_id.max(self.file.clusters.iter().map(|o| o.id).max().unwrap_or(0)) + 1;
            c.id = self.file.next_id;
            self.file.clusters.push(c.clone());
        } else if let Some(slot) = self.file.clusters.iter_mut().find(|o| o.id == c.id) {
            *slot = c.clone();
        } else {
            bail!("That cluster no longer exists.");
        }
        self.write()?;
        Ok(c.id)
    }

    pub fn delete(&mut self, id: u64) -> Result<()> {
        self.file.clusters.retain(|c| c.id != id);
        self.write()
    }

    fn write(&self) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(&self.file)?)?;
        std::fs::rename(tmp, &self.path)?;
        Ok(())
    }
}

/// One word of a shell command, quoted only when it has to be.
pub fn quote(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_./=:,@%+".contains(c)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// The text a person (or an agent) typed after `kubectl`, as words.
fn program(c: &Cluster) -> String {
    let p = c.kubectl.trim();
    if p.is_empty() { "kubectl".into() } else { p.to_string() }
}

/// `kubectl <args>` as the person or agent asked it: what the read-only rule looks at. It says `kubectl` whatever the
/// program is (`k3s kubectl`, a full path): the rule is about the words after it.
pub fn asked_line(_c: &Cluster, args: &[String]) -> String {
    std::iter::once("kubectl".to_string()).chain(args.iter().map(|a| quote(a))).collect::<Vec<_>>().join(" ")
}

/// The full command with the cluster's flags (`--kubeconfig`, `--context`, `-n`), to run or to type in a terminal.
/// `rest` is already shell text (a person's own words, or words made with `quote`).
pub fn full_line(c: &Cluster, rest: &str) -> String {
    let mut parts = vec![program(c)];
    if c.via_server.is_none() && !c.kubeconfig.trim().is_empty() {
        parts.push(format!("--kubeconfig {}", quote(&expand_home(c.kubeconfig.trim()))));
    }
    if !c.context.trim().is_empty() {
        parts.push(format!("--context {}", quote(c.context.trim())));
    }
    if !c.namespace.trim().is_empty() && !rest.split_whitespace().any(|w| matches!(w, "-n" | "--namespace" | "-A" | "--all-namespaces") || w.starts_with("--namespace=")) {
        parts.push(format!("-n {}", quote(c.namespace.trim())));
    }
    if !rest.trim().is_empty() {
        parts.push(rest.trim().to_string());
    }
    parts.join(" ")
}

/// The same, from separate arguments (an agent's).
pub fn full_line_args(c: &Cluster, args: &[String]) -> String {
    let rest = args.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ");
    let mut line = full_line(c, &rest);
    // the cluster's own flags must come before an agent's `--` (if any): kubectl accepts them anywhere before it
    if args.iter().any(|a| a == "--") {
        line = full_line(c, "") + " " + &rest;
    }
    line
}

/// The arguments that make kubectl run something other than a plain kubectl command (a plugin, or a shell through
/// `--kubeconfig`-style tricks): refused for agents, whatever they say.
pub fn refused_arg(args: &[String]) -> Option<&'static str> {
    let first = args.iter().find(|a| !a.starts_with('-'));
    if first.is_none() {
        return Some("Give a kubectl command (get, describe, logs, ...).");
    }
    if args.iter().any(|a| a == "--kubeconfig" || a.starts_with("--kubeconfig=") || a == "--context" || a.starts_with("--context=") || a == "--server" || a.starts_with("--server=") || a == "-s" || a == "--token" || a.starts_with("--token=") || a == "--as" || a.starts_with("--as=") || a == "--certificate-authority" || a == "--client-key" || a == "--client-certificate" || a.starts_with("--insecure")) {
        return Some("oso chooses the cluster and its login: do not pass --kubeconfig, --context, --server, --token or --as.");
    }
    None
}

fn expand_home(p: &str) -> String {
    crate::spaces::expand_home(p).to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cl() -> Cluster {
        Cluster { name: "k3s".into(), ..Default::default() }
    }

    #[test]
    fn words_are_quoted_only_when_needed() {
        assert_eq!(quote("pods"), "pods");
        assert_eq!(quote("app=web"), "app=web");
        assert_eq!(quote("a b"), "'a b'");
        assert_eq!(quote("it's"), "'it'\\''s'");
        assert_eq!(quote(""), "''");
        assert_eq!(quote("$(reboot)"), "'$(reboot)'");
    }

    #[test]
    fn the_line_carries_the_clusters_flags() {
        let mut c = cl();
        assert_eq!(full_line(&c, "get pods"), "kubectl get pods");
        c.context = "prod".into();
        c.namespace = "web".into();
        c.kubeconfig = "/home/u/k.yaml".into();
        assert_eq!(full_line(&c, "get pods"), "kubectl --kubeconfig /home/u/k.yaml --context prod -n web get pods");
        assert_eq!(full_line(&c, "get pods -A"), "kubectl --kubeconfig /home/u/k.yaml --context prod get pods -A", "an explicit namespace wins");
        c.via_server = Some(3);
        c.kubectl = "k3s kubectl".into();
        assert_eq!(full_line(&c, "get nodes"), "k3s kubectl --context prod -n web get nodes", "no local kubeconfig through a server");
    }

    #[test]
    fn an_agents_arguments_cannot_become_more_than_words() {
        let c = cl();
        let a: Vec<String> = ["get", "pods", "-l", "app=a b;reboot"].iter().map(|s| s.to_string()).collect();
        assert_eq!(full_line_args(&c, &a), "kubectl get pods -l 'app=a b;reboot'");
        // quoted, the `;` is one argument of kubectl and not a second command
        assert!(crate::policy::is_read_only(&asked_line(&c, &a)));
        let chained: Vec<String> = ["get", "pods", ";", "reboot"].iter().map(|s| s.to_string()).collect();
        assert_eq!(full_line_args(&c, &chained), "kubectl get pods ';' reboot");
        let ok: Vec<String> = ["get", "pods", "-n", "web"].iter().map(|s| s.to_string()).collect();
        assert!(crate::policy::is_read_only(&asked_line(&c, &ok)));
        let sec: Vec<String> = ["get", "secrets"].iter().map(|s| s.to_string()).collect();
        assert!(!crate::policy::is_read_only(&asked_line(&c, &sec)));
        let del: Vec<String> = ["delete", "pod", "x"].iter().map(|s| s.to_string()).collect();
        assert!(!crate::policy::is_read_only(&asked_line(&c, &del)));
    }

    #[test]
    fn an_agent_cannot_choose_the_cluster_or_login() {
        for bad in [vec!["get", "pods", "--kubeconfig", "/x"], vec!["get", "pods", "--server=https://evil"], vec!["--token", "t", "get", "pods"], vec!["get", "pods", "--as=admin"], vec!["get", "pods", "--insecure-skip-tls-verify"], vec!["--context=x", "get", "pods"]] {
            let a: Vec<String> = bad.iter().map(|s| s.to_string()).collect();
            assert!(refused_arg(&a).is_some(), "{bad:?}");
        }
        let a: Vec<String> = ["get", "pods", "-A"].iter().map(|s| s.to_string()).collect();
        assert!(refused_arg(&a).is_none());
        assert!(refused_arg(&[]).is_some());
    }

    #[test]
    fn clusters_are_saved_and_found() {
        let dir = std::env::temp_dir().join(format!("oso-k8s-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut cs = Clusters::load_from(&dir);
        let id = cs.save(Cluster { name: " Prod ".into(), ..Default::default() }).unwrap();
        let id2 = cs.save(Cluster { name: "Prod-eu".into(), ..Default::default() }).unwrap();
        assert_ne!(id, id2);
        assert!(cs.save(Cluster { name: "prod".into(), ..Default::default() }).is_err(), "names are unique");
        assert!(cs.save(Cluster { name: "".into(), ..Default::default() }).is_err());
        assert_eq!(cs.find("prod").unwrap().id, id, "an exact name wins over a part");
        assert!(cs.find("eu").is_ok());
        assert!(cs.find("zzz").is_err());
        let again = Clusters::load_from(&dir);
        assert_eq!(again.list().len(), 2);
        let mut edited = again.get(id).unwrap().clone();
        edited.context = "x".into();
        let mut again = again;
        again.save(edited).unwrap();
        assert_eq!(again.get(id).unwrap().context, "x");
        again.delete(id).unwrap();
        assert_eq!(Clusters::load_from(&dir).list().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
