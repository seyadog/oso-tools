//! How oso reaches a server: the `ssh` command (with jump hosts), the local shell, and the saved secrets each
//! prompt may be answered with. It has no interface in it, so any front end can use it.

use crate::session::{Autofill, SpawnOpts};
use crate::store::{Auth, Forward, ForwardKind, Server, Store};
use crate::vault::{Kind, Vault};
use std::path::PathBuf;

/// `server_id` of the terminal of this computer (real servers start at 1).
pub const LOCAL: u64 = 0;

fn home_dir() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf())
}

pub fn expand_tilde(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(h) = home_dir() {
            return h.join(rest).to_string_lossy().to_string();
        }
    }
    p.to_string()
}

/// The user's shell, for the local terminal tab.
fn local_shell() -> Vec<String> {
    if cfg!(windows) {
        return vec!["powershell.exe".into(), "-NoLogo".into()];
    }
    vec![std::env::var("SHELL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/bin/sh".into())]
}

fn target(s: &Server) -> String {
    if s.user.is_empty() { s.host.clone() } else { format!("{}@{}", s.user, s.host) }
}

/// Quotes `s` for the shell that runs a `ProxyCommand`: `sh` on Unix, `cmd.exe` on Windows.
/// Plain words are left as they are.
fn sh_quote(s: &str) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || "/._-:=@%,+".contains(c) || (cfg!(windows) && c == '\\');
    if !s.is_empty() && s.chars().all(plain) {
        return s.to_string();
    }
    if cfg!(windows) {
        format!("\"{}\"", s.replace('"', "\\\""))
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// Options shared by direct connections and by the hops of a ProxyCommand.
fn auth_args(s: &Server) -> Vec<String> {
    match s.auth {
        Auth::Agent => vec![],
        Auth::Key => vec!["-i".into(), expand_tilde(&s.key_path), "-o".into(), "IdentitiesOnly=yes".into()],
        Auth::Password => vec![
            "-o".into(),
            "PreferredAuthentications=password,keyboard-interactive".into(),
            "-o".into(),
            "PubkeyAuthentication=no".into(),
        ],
    }
}

/// `ssh` command that tunnels through `chain` (outermost first), as a single shell string.
fn proxy_command(chain: &[&Server]) -> String {
    let Some((hop, before)) = chain.split_last() else { return String::new() };
    let mut parts = vec!["ssh".to_string()];
    parts.extend(auth_args(hop).into_iter().map(|a| sh_quote(&a)));
    parts.extend(["-p".into(), hop.port.to_string()]);
    if !before.is_empty() {
        parts.extend(["-o".into(), sh_quote(&format!("ProxyCommand={}", proxy_command(before)))]);
    }
    parts.extend(["-W".into(), "%h:%p".into(), target(hop)]);
    parts.join(" ")
}

/// Arguments for the system `ssh` client, including any jump hosts.
pub fn ssh_argv(store: &Store, s: &Server) -> Vec<String> {
    let mut a: Vec<String> = vec!["ssh".into(), "-p".into(), s.port.to_string()];
    a.extend(["-o".into(), "ServerAliveInterval=15".into(), "-o".into(), "ConnectTimeout=15".into()]);
    a.extend(auth_args(s));
    let chain = store.jump_chain(s.id);
    if !chain.is_empty() {
        if chain.iter().all(|h| h.auth != Auth::Key) {
            let hops: Vec<String> = chain
                .iter()
                .map(|h| if h.port == 22 { target(h) } else { format!("{}:{}", target(h), h.port) })
                .collect();
            a.extend(["-J".into(), hops.join(",")]);
        } else {
            // -J cannot give each hop its own key, so build the tunnel by hand.
            a.extend(["-o".into(), format!("ProxyCommand={}", proxy_command(&chain))]);
        }
    }
    a.push(target(s));
    a
}

/// The environment a local terminal starts with: the one this program has, minus what only oso may set.
pub fn client_env() -> Vec<(String, String)> {
    const OURS: [&str; 5] = ["OSO", "OSO_PROFILE", "OSO_PANE", "OSO_TOKEN", "OSO_SOCKET"];
    std::env::vars().filter(|(k, _)| !OURS.contains(&k.as_str())).collect()
}

/// What is needed to open a session for `id` (a server, or `LOCAL`).
pub struct Plan {
    pub title: String,
    pub opts: SpawnOpts,
}

/// The saved secrets for reaching `id`: its own and those of every jump host on the way, each tied to the
/// prompt it answers, so that a hop's password is never typed at another host's prompt.
pub fn autofills(store: &Store, vault: &mut Vault, id: u64) -> Vec<Autofill> {
    let mut servers: Vec<Server> = store.jump_chain(id).into_iter().cloned().collect();
    servers.extend(store.server(id).cloned());
    let mut out = vec![];
    for s in servers {
        if s.auth == Auth::Agent || !s.has_secret {
            continue;
        }
        let Some(secret) = vault.get(s.id, Kind::Login) else { continue };
        let when = match s.auth {
            Auth::Key => format!("'{}'", expand_tilde(&s.key_path)).to_lowercase(),
            _ => format!("{}@{}", s.user, s.host).to_lowercase(),
        };
        out.push(Autofill { when, secret, fallback: s.id == id });
    }
    out
}

/// Whether opening `id` needs the vault unlocked: it or a jump host on the way has a saved secret.
pub fn needs_vault(store: &Store, id: u64) -> bool {
    let wants = |s: &Server| s.auth != Auth::Agent && s.has_secret;
    store.server(id).is_some_and(wants) || store.jump_chain(id).into_iter().any(wants)
}

/// What is needed to run `argv` (an ssh or scp command for server `id`): the saved secrets are typed at the
/// prompts they belong to. `login_expected` says the first generic `Password:` is the login.
pub fn opts_for(store: &Store, vault: &mut Vault, id: u64, argv: Vec<String>, rows: u16, cols: u16, use_vault: bool) -> SpawnOpts {
    let login_expected = id != LOCAL && store.server(id).is_some_and(|s| s.auth == Auth::Password);
    let fills = if use_vault && id != LOCAL { autofills(store, vault, id) } else { vec![] };
    SpawnOpts { argv, rows, cols, fills, login_expected, cwd: None, env: None, tap: None, sudo_fill: None, agent: false }
}

/// The command, title and secrets to open `id`. `unlocked` says whether the saved secrets may be used.
pub fn plan(store: &Store, vault: &mut Vault, id: u64, rows: u16, cols: u16, use_vault: bool) -> Option<Plan> {
    let (argv, title) = if id == LOCAL {
        (local_shell(), "Local".to_string())
    } else {
        let server = store.server(id)?.clone();
        (ssh_argv(store, &server), server.name.clone())
    };
    Some(Plan { title, opts: opts_for(store, vault, id, argv, rows, cols, use_vault) })
}

/// `ssh` that only keeps `forward` open (no shell), through the same jump hosts and with the same login as a
/// terminal of this server. `extra` goes before the target (options for tests).
pub fn tunnel_argv(store: &Store, s: &Server, f: &Forward, extra: &[String]) -> Vec<String> {
    let mut a = ssh_argv(store, s);
    let target = a.pop().unwrap_or_default();
    a.extend(["-N".into(), "-o".into(), "ExitOnForwardFailure=yes".into(), "-o".into(), "ControlPath=none".into()]);
    let spec = match f.kind {
        ForwardKind::Local => ["-L".to_string(), format!("127.0.0.1:{}:{}:{}", f.bind_port, f.host, f.port)],
        ForwardKind::Remote => ["-R".to_string(), format!("127.0.0.1:{}:{}:{}", f.bind_port, f.host, f.port)],
        ForwardKind::Dynamic => ["-D".to_string(), format!("127.0.0.1:{}", f.bind_port)],
    };
    a.extend(spec);
    a.extend(extra.iter().cloned());
    a.push(target);
    a
}

/// Which way a file goes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    Up,
    Down,
}

/// `scp` for server `s` with the same login and jump hosts as a terminal. Up: `local` are paths on this computer
/// and `remote` is the folder on the server (empty: its home). Down: `local[0]` is the folder here and `remote` the
/// path on the server. `extra` goes before the files (options for tests).
pub fn scp_argv(store: &Store, s: &Server, dir: Direction, local: &[String], remote: &str, extra: &[String]) -> Vec<String> {
    let mut ssh = ssh_argv(store, s);
    let target = ssh.pop().unwrap_or_default();
    let mut a: Vec<String> = vec!["scp".into(), "-r".into(), "-o".into(), "ControlPath=none".into()];
    let mut it = ssh.into_iter().skip(1);
    while let Some(x) = it.next() {
        // scp spells the port -P
        if x == "-p" {
            a.push("-P".into());
        } else {
            a.push(x);
        }
    }
    a.extend(extra.iter().cloned());
    let remote_spec = |path: &str| format!("{target}:{}", path.trim_start_matches("~/").trim_start_matches('~'));
    match dir {
        Direction::Up => {
            a.push("--".into());
            a.extend(local.iter().cloned());
            a.push(remote_spec(remote));
        }
        Direction::Down => {
            a.push("--".into());
            a.push(remote_spec(remote));
            a.push(local.first().cloned().unwrap_or_else(|| ".".into()));
        }
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(auth: Auth, key: &str, user: &str) -> Server {
        Server {
            id: 1,
            name: "n".into(),
            host: "h.example".into(),
            port: 2222,
            user: user.into(),
            auth,
            key_path: key.into(),
            parent: None,
            jump: None,
            has_secret: false,
            has_sudo: false,
            forwards: vec![],
            profile: None,
        }
    }

    #[test]
    fn argv_for_key_auth() {
        let a = ssh_argv(&Store::default(), &server(Auth::Key, "/k/id", "root"));
        assert_eq!(a.last().unwrap(), "root@h.example");
        assert!(a.windows(2).any(|w| w == ["-i", "/k/id"]));
        assert!(a.windows(2).any(|w| w == ["-p", "2222"]));
    }

    #[test]
    fn argv_without_user_uses_host_only() {
        assert_eq!(ssh_argv(&Store::default(), &server(Auth::Agent, "", "")).last().unwrap(), "h.example");
    }

    fn chain_store(hop_auth: Auth) -> (Store, Server) {
        let mut st = Store::default();
        let mut bastion = server(hop_auth, "/k/bast", "ops");
        bastion.host = "bastion.example".into();
        bastion.port = 22;
        let b = st.add_server(bastion);
        let mut inner = server(Auth::Agent, "", "app");
        inner.jump = Some(b);
        let id = st.add_server(inner);
        let target = st.server(id).unwrap().clone();
        (st, target)
    }

    #[test]
    fn argv_uses_dash_j_for_agent_hops() {
        let (st, t) = chain_store(Auth::Agent);
        let a = ssh_argv(&st, &t);
        assert!(a.windows(2).any(|w| w == ["-J", "ops@bastion.example"]), "{a:?}");
    }

    #[test]
    fn quotes_only_what_needs_quoting() {
        assert_eq!(sh_quote("-W"), "-W");
        assert_eq!(sh_quote("%h:%p"), "%h:%p");
        let q = sh_quote("/k/my key");
        assert!(q == "'/k/my key'" || q == "\"/k/my key\"", "{q}");
    }

    #[test]
    fn proxy_command_quotes_key_paths_with_spaces() {
        let mut st = Store::default();
        let mut bastion = server(Auth::Key, "/k/my key", "ops");
        bastion.host = "bastion.example".into();
        let b = st.add_server(bastion);
        let mut inner = server(Auth::Agent, "", "app");
        inner.jump = Some(b);
        let id = st.add_server(inner);
        let a = ssh_argv(&st, &st.server(id).unwrap().clone());
        let pc = a.iter().find(|x| x.starts_with("ProxyCommand=")).unwrap();
        assert!(pc.contains("-i '/k/my key'") || pc.contains("-i \"/k/my key\""), "{pc}");
    }

    fn forward(kind: ForwardKind) -> Forward {
        Forward { id: 1, kind, bind_port: 5433, host: "db.internal".into(), port: 5432 }
    }

    #[test]
    fn tunnel_keeps_the_forward_and_no_shell() {
        let s = server(Auth::Key, "/k/id", "root");
        let a = tunnel_argv(&Store::default(), &s, &forward(ForwardKind::Local), &[]);
        assert_eq!(a.last().unwrap(), "root@h.example");
        assert!(a.contains(&"-N".to_string()));
        assert!(a.windows(2).any(|w| w == ["-L", "127.0.0.1:5433:db.internal:5432"]), "{a:?}");
        assert!(a.windows(2).any(|w| w == ["-o", "ExitOnForwardFailure=yes"]));
        assert!(a.windows(2).any(|w| w == ["-i", "/k/id"]), "same login as a terminal");
        let r = tunnel_argv(&Store::default(), &s, &forward(ForwardKind::Remote), &[]);
        assert!(r.windows(2).any(|w| w == ["-R", "127.0.0.1:5433:db.internal:5432"]), "{r:?}");
        let d = tunnel_argv(&Store::default(), &s, &forward(ForwardKind::Dynamic), &["-o".into(), "X=1".into()]);
        assert!(d.windows(2).any(|w| w == ["-D", "127.0.0.1:5433"]), "{d:?}");
        assert_eq!(d[d.len() - 3..], ["-o".to_string(), "X=1".to_string(), "root@h.example".to_string()]);
    }

    #[test]
    fn tunnel_goes_through_the_jump_hosts() {
        let (st, t) = chain_store(Auth::Agent);
        let a = tunnel_argv(&st, &t, &forward(ForwardKind::Local), &[]);
        assert!(a.windows(2).any(|w| w == ["-J", "ops@bastion.example"]), "{a:?}");
    }

    #[test]
    fn scp_up_and_down() {
        let s = server(Auth::Key, "/k/id", "root");
        let st = Store::default();
        let up = scp_argv(&st, &s, Direction::Up, &["/tmp/a b.txt".into(), "/tmp/dir".into()], "~/backups", &[]);
        assert_eq!(up[0], "scp");
        assert!(up.contains(&"-r".to_string()));
        assert!(up.windows(2).any(|w| w == ["-P", "2222"]), "scp spells the port -P: {up:?}");
        assert!(!up.contains(&"-p".to_string()));
        assert!(up.windows(2).any(|w| w == ["-i", "/k/id"]));
        let at = up.iter().position(|x| x == "--").unwrap();
        assert_eq!(up[at + 1..], ["/tmp/a b.txt", "/tmp/dir", "root@h.example:backups"]);
        // home when no folder is given
        let home = scp_argv(&st, &s, Direction::Up, &["/tmp/x".into()], "", &[]);
        assert_eq!(home.last().unwrap(), "root@h.example:");
        let down = scp_argv(&st, &s, Direction::Down, &["/home/me/Downloads".into()], "/var/log/syslog", &[]);
        assert_eq!(down[down.len() - 2..], ["root@h.example:/var/log/syslog", "/home/me/Downloads"]);
    }

    #[test]
    fn scp_goes_through_the_jump_hosts_too() {
        let (st, t) = chain_store(Auth::Key);
        let a = scp_argv(&st, &t, Direction::Up, &["/tmp/x".into()], "", &[]);
        assert!(a.iter().any(|x| x.starts_with("ProxyCommand=")), "{a:?}");
    }

    #[test]
    fn argv_uses_proxy_command_when_a_hop_has_a_key() {
        let (st, t) = chain_store(Auth::Key);
        let a = ssh_argv(&st, &t);
        let pc = a.iter().find(|x| x.starts_with("ProxyCommand=")).expect("ProxyCommand");
        assert!(pc.contains("-i /k/bast") && pc.contains("-W %h:%p ops@bastion.example"), "{pc}");
    }
}
