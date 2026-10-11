//! Reading what a person already has in `~/.ssh`: the private keys (`id_*`) and the hosts of `~/.ssh/config`. This
//! only *reads*; the engine turns a chosen part of it into secrets and servers. Nothing here writes or deletes.

use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct KeyFile {
    /// Absolute path of the private key.
    pub path: String,
    pub name: String,
    /// `ssh-ed25519`, `ssh-rsa`…
    pub algorithm: String,
    /// `SHA256:…`, of the public half (readable without the passphrase).
    pub fingerprint: String,
    pub comment: String,
    /// Asking for a passphrase to be used.
    pub encrypted: bool,
    /// Why oso cannot take it into the vault (a security key, an old format), or empty.
    pub unsupported: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Default)]
pub struct HostEntry {
    pub alias: String,
    pub host: String,
    pub user: String,
    pub port: u16,
    pub identity_file: Option<String>,
    pub proxy_jump: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Default)]
pub struct Preview {
    pub keys: Vec<KeyFile>,
    pub hosts: Vec<HostEntry>,
    /// Things in the config oso does not turn into a server (patterns, `Include`), in words.
    pub skipped: Vec<String>,
}

/// `~/x` and `%d/x` to a path.
pub fn expand(path: &str, home: &Path) -> String {
    let p = path.trim().trim_matches('"');
    if let Some(rest) = p.strip_prefix("~/") {
        return home.join(rest).to_string_lossy().into_owned();
    }
    if let Some(rest) = p.strip_prefix("%d/") {
        return home.join(rest).to_string_lossy().into_owned();
    }
    p.to_string()
}

/// The hosts of an OpenSSH client config. Wildcard `Host` blocks are not hosts, but what they set (`User`, `Port`,
/// `IdentityFile`) is the default of the hosts that say nothing.
pub fn parse_config(text: &str, home: &Path) -> (Vec<HostEntry>, Vec<String>) {
    #[derive(Default, Clone)]
    struct Block {
        aliases: Vec<String>,
        host_name: Option<String>,
        user: Option<String>,
        port: Option<u16>,
        identity: Option<String>,
        proxy_jump: Option<String>,
    }
    let mut blocks: Vec<Block> = vec![];
    let mut skipped = vec![];
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // `Key value` or `Key=value`
        let (key, value) = match line.split_once(|c: char| c.is_whitespace() || c == '=') {
            Some((k, v)) => (k.trim().to_lowercase(), v.trim().trim_start_matches('=').trim().to_string()),
            None => (line.to_lowercase(), String::new()),
        };
        match key.as_str() {
            "host" => blocks.push(Block { aliases: value.split_whitespace().map(|s| s.trim_matches('"').to_string()).collect(), ..Default::default() }),
            "match" => blocks.push(Block::default()), // a block oso does not read: its lines go nowhere
            "include" => skipped.push(format!("Include {value}: not read")),
            _ => {
                let Some(b) = blocks.last_mut() else { continue };
                match key.as_str() {
                    "hostname" => b.host_name = Some(value),
                    "user" => b.user = Some(value),
                    "port" => b.port = value.parse().ok(),
                    "identityfile" if b.identity.is_none() => b.identity = Some(expand(&value, home)),
                    "proxyjump" => b.proxy_jump = Some(value).filter(|v| !v.eq_ignore_ascii_case("none")),
                    _ => {}
                }
            }
        }
    }
    let defaults = blocks.iter().find(|b| b.aliases.iter().any(|a| a == "*")).cloned().unwrap_or_default();
    let mut out: Vec<HostEntry> = vec![];
    for b in &blocks {
        for alias in &b.aliases {
            if alias.contains(['*', '?', '!']) {
                if alias != "*" {
                    skipped.push(format!("Host {alias}: a pattern, not a host"));
                }
                continue;
            }
            if out.iter().any(|h| &h.alias == alias) {
                continue; // OpenSSH uses the first value it finds
            }
            out.push(HostEntry {
                alias: alias.clone(),
                host: b.host_name.clone().unwrap_or_else(|| alias.clone()),
                user: b.user.clone().or(defaults.user.clone()).unwrap_or_default(),
                port: b.port.or(defaults.port).unwrap_or(22),
                identity_file: b.identity.clone().or(defaults.identity.clone()),
                proxy_jump: b.proxy_jump.clone().or(defaults.proxy_jump.clone()),
            });
        }
    }
    (out, skipped)
}

/// What a `ProxyJump` value names: `[user@]host[:port]`, in order, outermost first.
pub fn parse_jump(value: &str) -> Vec<(Option<String>, String, Option<u16>)> {
    value
        .split(',')
        .map(str::trim)
        .filter(|h| !h.is_empty())
        .map(|h| {
            let (user, rest) = match h.split_once('@') {
                Some((u, r)) => (Some(u.to_string()), r),
                None => (None, h),
            };
            match rest.rsplit_once(':') {
                Some((host, port)) if port.parse::<u16>().is_ok() && !host.contains(':') => (user, host.to_string(), port.parse().ok()),
                _ => (user, rest.to_string(), None),
            }
        })
        .collect()
}

/// Every `id_*` private key of `dir` that can be read.
pub fn scan_keys(dir: &Path) -> Vec<KeyFile> {
    let Ok(read) = std::fs::read_dir(dir) else { return vec![] };
    let mut names: Vec<PathBuf> = read.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
    names.sort();
    let mut out = vec![];
    for path in names {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if !name.starts_with("id_") || name.ends_with(".pub") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let path_s = path.to_string_lossy().into_owned();
        match ssh_key::PrivateKey::from_openssh(text.trim().as_bytes()) {
            Ok(k) => {
                let alg = k.algorithm().to_string();
                let unsupported = if alg.starts_with("sk-") { "a security key (FIDO): it stays a file, chosen as a key file".to_string() } else { String::new() };
                out.push(KeyFile {
                    path: path_s,
                    name,
                    algorithm: alg,
                    fingerprint: k.fingerprint(ssh_key::HashAlg::Sha256).to_string(),
                    comment: k.comment().to_string(),
                    encrypted: k.is_encrypted(),
                    unsupported,
                });
            }
            Err(_) if text.contains("PRIVATE KEY") => out.push(KeyFile { path: path_s, name, algorithm: String::new(), fingerprint: String::new(), comment: String::new(), encrypted: false, unsupported: "an old key format (PEM): convert it with `ssh-keygen -p -m RFC4716 -f <file>`".into() }),
            Err(_) => {}
        }
    }
    out
}

/// What there is to import in `dir` (normally `~/.ssh`).
pub fn scan(dir: &Path, home: &Path) -> Preview {
    let (hosts, skipped) = std::fs::read_to_string(dir.join("config")).map(|t| parse_config(&t, home)).unwrap_or_default();
    Preview { keys: scan_keys(dir), hosts, skipped }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"
# my machines
Include ~/.ssh/config.d/*

Host *
    User fallback
    IdentityFile ~/.ssh/id_default

Host web web-alias
    HostName web.example.com
    User deploy
    Port 2222
    IdentityFile ~/.ssh/id_work
    ProxyJump ops@bastion.example.com:2200

Host bastion
    hostname=bastion.example.com
    IdentityFile "~/.ssh/id_bastion"

Host db-*
    User ignored

Host bare
Host nokey
    IdentityFile none-here
    ProxyJump none
Match host x
    User nobody
"#;

    #[test]
    fn hosts_are_read_with_their_defaults_and_patterns_are_not_hosts() {
        let (hosts, skipped) = parse_config(CONFIG, Path::new("/home/me"));
        let names: Vec<&str> = hosts.iter().map(|h| h.alias.as_str()).collect();
        assert_eq!(names, vec!["web", "web-alias", "bastion", "bare", "nokey"]);
        let web = &hosts[0];
        assert_eq!((web.host.as_str(), web.user.as_str(), web.port), ("web.example.com", "deploy", 2222));
        assert_eq!(web.identity_file.as_deref(), Some("/home/me/.ssh/id_work"));
        assert_eq!(web.proxy_jump.as_deref(), Some("ops@bastion.example.com:2200"));
        assert_eq!(hosts[1].host, "web.example.com", "a second alias of the same block");
        let bastion = &hosts[2];
        assert_eq!((bastion.host.as_str(), bastion.user.as_str(), bastion.port), ("bastion.example.com", "fallback", 22), "`Host *` fills what is not said, `=` and quotes work");
        assert_eq!(bastion.identity_file.as_deref(), Some("/home/me/.ssh/id_bastion"));
        let bare = &hosts[3];
        assert_eq!((bare.host.as_str(), bare.identity_file.as_deref()), ("bare", Some("/home/me/.ssh/id_default")));
        assert_eq!(hosts[4].proxy_jump, None, "ProxyJump none");
        assert!(skipped.iter().any(|s| s.contains("Include")) && skipped.iter().any(|s| s.contains("db-*")), "{skipped:?}");
    }

    #[test]
    fn a_proxy_jump_is_split_into_hops() {
        let h = parse_jump("ops@a.example.com:2200, b.example.com ,c@d");
        assert_eq!(h, vec![(Some("ops".into()), "a.example.com".into(), Some(2200)), (None, "b.example.com".into(), None), (Some("c".into()), "d".into(), None)]);
        assert!(parse_jump("").is_empty());
        assert_eq!(parse_jump("::1")[0].1, "::1", "an IPv6 literal is not split at a colon");
    }

    #[test]
    fn paths_expand_the_home() {
        assert_eq!(expand("~/.ssh/id_x", Path::new("/h")), "/h/.ssh/id_x");
        assert_eq!(expand("%d/.ssh/id_x", Path::new("/h")), "/h/.ssh/id_x");
        assert_eq!(expand("/abs/id", Path::new("/h")), "/abs/id");
    }
}
