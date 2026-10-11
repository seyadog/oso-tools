//! How oso reaches a server: the `ssh` command (with jump hosts), the local shell, and the saved secrets each
//! prompt may be answered with. It has no interface in it, so any front end can use it.

use crate::session::{Autofill, SpawnOpts};
use crate::store::{Auth, Forward, ForwardKind, Proxy, ProxyKind, Server, Store};
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

/// The program that lets `ssh` talk through a proxy. Fedora has `ncat` (package `nmap-ncat`); other systems have the
/// OpenBSD `nc`, which spells its options differently.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProxyTool {
    program: &'static str,
    /// `--proxy host:port --proxy-type socks5` (ncat) or `-X 5 -x host:port` (OpenBSD nc).
    ncat_syntax: bool,
}

pub const NCAT: ProxyTool = ProxyTool { program: "ncat", ncat_syntax: true };

fn on_path(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| std::env::split_paths(&paths).any(|d| d.join(name).is_file()))
}

/// Which proxy helper is installed, looked for once.
pub fn proxy_tool() -> Option<ProxyTool> {
    static FOUND: std::sync::OnceLock<Option<ProxyTool>> = std::sync::OnceLock::new();
    *FOUND.get_or_init(|| {
        if on_path("ncat") {
            return Some(NCAT);
        }
        if !on_path("nc") {
            return None;
        }
        let help = std::process::Command::new("nc").arg("-h").output().ok()?;
        let text = format!("{}{}", String::from_utf8_lossy(&help.stdout), String::from_utf8_lossy(&help.stderr));
        if text.contains("Ncat") {
            Some(ProxyTool { program: "nc", ncat_syntax: true })
        } else if text.contains("-x proxy_address") || text.contains("proxy_protocol") {
            Some(ProxyTool { program: "nc", ncat_syntax: false })
        } else {
            None
        }
    })
}

/// The `ProxyCommand` that opens the connection to `%h:%p` through `p`.
pub fn proxy_command_for(tool: ProxyTool, p: &Proxy) -> String {
    let hp = if p.host.contains(':') { format!("[{}]:{}", p.host, p.port) } else { format!("{}:{}", p.host, p.port) };
    if tool.ncat_syntax {
        let kind = if p.kind == ProxyKind::Socks5 { "socks5" } else { "http" };
        format!("{} --proxy {hp} --proxy-type {kind} %h %p", tool.program)
    } else {
        let kind = if p.kind == ProxyKind::Socks5 { "5" } else { "connect" };
        format!("{} -X {kind} -x {hp} %h %p", tool.program)
    }
}

/// The proxy the connection to `id` leaves through: the one of its outermost hop (the server itself when it is direct).
pub fn outer_proxy<'a>(store: &'a Store, id: u64) -> Option<&'a Proxy> {
    let hop = store.jump_chain(id).first().copied().or_else(|| store.server(id))?;
    hop.proxy.and_then(|p| store.proxy(p))
}

/// Does reaching `id` use a key kept in the vault (so the session needs oso's agent)?
pub fn uses_vault_key(store: &Store, id: u64) -> bool {
    store.server(id).is_some_and(|s| s.auth == Auth::VaultKey) || store.jump_chain(id).iter().any(|h| h.auth == Auth::VaultKey)
}

/// The name of a server on the way (or `id` itself) whose vault key has no public file: the vault has not given it yet.
pub fn key_problem(store: &Store, id: u64) -> Option<String> {
    let mut hops: Vec<&Server> = store.jump_chain(id);
    hops.extend(store.server(id));
    let missing = hops.into_iter().find(|h| h.auth == Auth::VaultKey && !store.key_files.contains_key(&h.id))?;
    Some(format!("{} logs in with a key kept in the vault, and none is chosen for it (or the vault has not been unlocked yet): choose it in the server's form.", missing.name))
}

/// Why `id` cannot be reached through its proxy (a words-for-a-person answer), or `None`.
pub fn proxy_problem(store: &Store, id: u64) -> Option<String> {
    let hop = store.jump_chain(id).first().copied().or_else(|| store.server(id))?;
    let wanted = hop.proxy?;
    let Some(p) = store.proxy(wanted) else { return Some(format!("The proxy of {} no longer exists: pick another one in its form.", hop.name)) };
    if let Some(why) = p.problem() {
        return Some(format!("Proxy {}: {why}.", p.name));
    }
    if proxy_tool().is_none() {
        return Some("To go through a proxy oso needs `ncat` (on Fedora: sudo dnf install nmap-ncat) or the OpenBSD `nc`, and neither is installed.".into());
    }
    None
}

/// The variables that make a local terminal use `p` (this session only: the system's network is never touched).
pub fn proxy_env(p: &Proxy) -> Vec<(String, String)> {
    let url = p.url();
    let mut out = vec![];
    for name in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY"] {
        out.push((name.to_string(), url.clone()));
        out.push((name.to_lowercase(), url.clone()));
    }
    out.push(("NO_PROXY".into(), "localhost,127.0.0.1,::1".into()));
    out.push(("no_proxy".into(), "localhost,127.0.0.1,::1".into()));
    out
}

/// Options shared by direct connections and by the hops of a ProxyCommand.
fn auth_args(store: &Store, s: &Server) -> Vec<String> {
    match s.auth {
        Auth::Agent => vec![],
        // oso's agent has the key; its public half says which one this server wants
        Auth::VaultKey => {
            let mut a = vec![];
            if let Some(f) = store.key_files.get(&s.id) {
                a.extend(["-i".to_string(), f.to_string_lossy().into_owned()]);
            }
            a.extend(["-o".into(), "IdentitiesOnly=yes".into()]);
            a
        }
        Auth::Key => vec!["-i".into(), expand_tilde(&s.key_path), "-o".into(), "IdentitiesOnly=yes".into()],
        Auth::Password => vec![
            "-o".into(),
            "PreferredAuthentications=password,keyboard-interactive".into(),
            "-o".into(),
            "PubkeyAuthentication=no".into(),
        ],
    }
}

/// `ssh` command that tunnels through `chain` (outermost first), as a single shell string. `via` is the proxy
/// command the outermost hop leaves through, if it has one.
fn proxy_command(store: &Store, chain: &[&Server], via: Option<&str>) -> String {
    let Some((hop, before)) = chain.split_last() else { return String::new() };
    let mut parts = vec!["ssh".to_string()];
    parts.extend(auth_args(store, hop).into_iter().map(|a| sh_quote(&a)));
    parts.extend(store.extra_options.iter().map(|a| sh_quote(a)));
    parts.extend(["-p".into(), hop.port.to_string()]);
    if !before.is_empty() {
        parts.extend(["-o".into(), sh_quote(&format!("ProxyCommand={}", proxy_command(store, before, via)))]);
    } else if let Some(v) = via {
        parts.extend(["-o".into(), sh_quote(&format!("ProxyCommand={v}"))]);
    }
    parts.extend(["-W".into(), "%h:%p".into(), target(hop)]);
    parts.join(" ")
}

/// `-L` / `-R` / `-D` and its value for a forward.
fn forward_args(f: &Forward) -> [String; 2] {
    match f.kind {
        ForwardKind::Local => ["-L".to_string(), format!("127.0.0.1:{}:{}:{}", f.bind_port, f.host, f.port)],
        ForwardKind::Remote => ["-R".to_string(), format!("127.0.0.1:{}:{}:{}", f.bind_port, f.host, f.port)],
        ForwardKind::Dynamic => ["-D".to_string(), format!("127.0.0.1:{}", f.bind_port)],
    }
}

/// Arguments for the system `ssh` client for a terminal: jump hosts, the proxy, and the tunnels marked `auto`.
pub fn ssh_argv(store: &Store, s: &Server) -> Vec<String> {
    build_ssh(store, s, proxy_tool().unwrap_or(NCAT), true)
}

/// The same without the automatic tunnels (for a tunnel or a copy, which bring their own).
fn ssh_argv_plain(store: &Store, s: &Server) -> Vec<String> {
    build_ssh(store, s, proxy_tool().unwrap_or(NCAT), false)
}

fn build_ssh(store: &Store, s: &Server, tool: ProxyTool, auto: bool) -> Vec<String> {
    let mut a: Vec<String> = vec!["ssh".into(), "-p".into(), s.port.to_string()];
    a.extend(["-o".into(), "ServerAliveInterval=15".into(), "-o".into(), "ConnectTimeout=15".into()]);
    a.extend(auth_args(store, s));
    a.extend(store.extra_options.iter().cloned());
    let chain = store.jump_chain(s.id);
    let via = outer_proxy(store, s.id).map(|p| proxy_command_for(tool, p));
    if chain.is_empty() {
        if let Some(v) = &via {
            a.extend(["-o".into(), format!("ProxyCommand={v}")]);
        }
    } else {
        if chain.iter().all(|h| h.auth != Auth::Key && h.auth != Auth::VaultKey) && via.is_none() {
            let hops: Vec<String> = chain
                .iter()
                .map(|h| if h.port == 22 { target(h) } else { format!("{}:{}", target(h), h.port) })
                .collect();
            a.extend(["-J".into(), hops.join(",")]);
        } else {
            // -J cannot give each hop its own key (nor leave through a proxy), so build the tunnel by hand.
            a.extend(["-o".into(), format!("ProxyCommand={}", proxy_command(store, &chain, via.as_deref()))]);
        }
    }
    if auto {
        for f in s.forwards.iter().filter(|f| f.auto) {
            a.extend(forward_args(f));
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
        if s.auth == Auth::Agent || s.auth == Auth::VaultKey || !s.has_secret {
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
    let wants = |s: &Server| s.auth == Auth::VaultKey || (s.auth != Auth::Agent && s.has_secret);
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
    let mut a = ssh_argv_plain(store, s);
    let target = a.pop().unwrap_or_default();
    a.extend(["-N".into(), "-o".into(), "ExitOnForwardFailure=yes".into(), "-o".into(), "ControlPath=none".into()]);
    a.extend(forward_args(f));
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
    let mut ssh = ssh_argv_plain(store, s);
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
            notes: String::new(),
            env_kind: None,
            proxy: None,
            network: None,
            ai_policy: Default::default(),
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
        Forward { id: 1, name: String::new(), auto: false, kind, bind_port: 5433, host: "db.internal".into(), port: 5432 }
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

    fn socks(host: &str, port: u16) -> Proxy {
        Proxy { id: 0, name: "corp".into(), kind: ProxyKind::Socks5, host: host.into(), port, profile: None }
    }

    /// A store with the saved proxy `p` (its id is returned).
    fn with_proxy(st: &mut Store, p: Proxy) -> u64 {
        let id = st.proxies.iter().map(|x| x.id).max().unwrap_or(0) + 1;
        st.proxies.push(Proxy { id, ..p });
        id
    }

    #[test]
    fn a_direct_server_through_a_proxy_uses_ncat() {
        let mut st = Store::default();
        let px = with_proxy(&mut st, socks("proxy.corp", 1080));
        let mut s = server(Auth::Agent, "", "root");
        s.proxy = Some(px);
        let id = st.add_server(s);
        let t = st.server(id).unwrap().clone();
        let a = build_ssh(&st, &t, NCAT, true);
        assert!(a.windows(2).any(|w| w == ["-o", "ProxyCommand=ncat --proxy proxy.corp:1080 --proxy-type socks5 %h %p"]), "{a:?}");
        let bsd = ProxyTool { program: "nc", ncat_syntax: false };
        let a = build_ssh(&st, &t, bsd, true);
        assert!(a.windows(2).any(|w| w == ["-o", "ProxyCommand=nc -X 5 -x proxy.corp:1080 %h %p"]), "{a:?}");
        // an HTTP proxy
        st.proxies[0].kind = ProxyKind::Http;
        let a = build_ssh(&st, &t, NCAT, true);
        assert!(a.iter().any(|x| x.ends_with("--proxy-type http %h %p")), "{a:?}");
        let a = build_ssh(&st, &t, bsd, true);
        assert!(a.iter().any(|x| x.ends_with("-X connect -x proxy.corp:1080 %h %p")), "{a:?}");
    }

    #[test]
    fn only_the_outermost_hop_leaves_through_the_proxy_and_each_hop_keeps_its_key() {
        let mut st = Store::default();
        let px = with_proxy(&mut st, socks("proxy.corp", 1080));
        let mut outer = server(Auth::Key, "/k/outer", "ops");
        outer.host = "bastion.example".into();
        outer.port = 22;
        outer.proxy = Some(px);
        let o = st.add_server(outer);
        let mut mid = server(Auth::Key, "/k/mid", "mid");
        mid.host = "mid.example".into();
        mid.port = 22;
        mid.jump = Some(o);
        let m = st.add_server(mid);
        let mut inner = server(Auth::Key, "/k/inner", "app");
        inner.jump = Some(m);
        inner.proxy = Some(px); // not the first hop: ignored
        let id = st.add_server(inner);
        let t = st.server(id).unwrap().clone();
        let a = build_ssh(&st, &t, NCAT, true);
        assert!(a.windows(2).any(|w| w == ["-i", "/k/inner"]));
        let pc = a.iter().find(|x| x.starts_with("ProxyCommand=")).expect("ProxyCommand");
        assert_eq!(pc.matches("ncat").count(), 1, "one proxy, at the start of the chain: {pc}");
        let at = |needle: &str| pc.find(needle).unwrap_or_else(|| panic!("{needle} in {pc}"));
        // the inner command goes through mid, which goes through the outer one, which goes out through the proxy
        assert!(at("/k/mid") < at("/k/outer") && at("/k/outer") < at("ncat") && at("ncat") < at("ops@bastion.example"), "{pc}");
        assert!(pc.contains("mid@mid.example") && pc.contains("-W %h:%p"));
    }

    #[test]
    fn a_proxy_replaces_dash_j_for_agent_hops_and_a_hop_without_one_keeps_it() {
        let mut st = Store::default();
        let px = with_proxy(&mut st, socks("proxy.corp", 1080));
        let mut b = server(Auth::Agent, "", "ops");
        b.host = "bastion.example".into();
        b.port = 22;
        let b = st.add_server(b);
        let mut inner = server(Auth::Agent, "", "app");
        inner.jump = Some(b);
        let id = st.add_server(inner);
        let t = st.server(id).unwrap().clone();
        assert!(build_ssh(&st, &t, NCAT, true).windows(2).any(|w| w == ["-J", "ops@bastion.example"]), "no proxy: -J as before");
        st.servers.iter_mut().find(|s| s.id == b).unwrap().proxy = Some(px);
        let a = build_ssh(&st, &st.server(id).unwrap().clone(), NCAT, true);
        assert!(!a.contains(&"-J".to_string()), "{a:?}");
        let pc = a.iter().find(|x| x.starts_with("ProxyCommand=")).unwrap();
        assert!(pc.contains("ncat") && pc.contains("-W %h:%p ops@bastion.example"), "{pc}");
    }

    #[test]
    fn tunnels_marked_auto_ride_on_every_terminal_but_not_on_tunnel_or_copy_commands() {
        let mut s = server(Auth::Agent, "", "root");
        s.forwards = vec![
            Forward { id: 1, name: "db".into(), auto: true, kind: ForwardKind::Local, bind_port: 5433, host: "db.internal".into(), port: 5432 },
            Forward { id: 2, name: "later".into(), auto: false, kind: ForwardKind::Local, bind_port: 8080, host: "web".into(), port: 80 },
            Forward { id: 3, name: "socks".into(), auto: true, kind: ForwardKind::Dynamic, bind_port: 1081, host: String::new(), port: 0 },
        ];
        let a = ssh_argv(&Store::default(), &s);
        assert!(a.windows(2).any(|w| w == ["-L", "127.0.0.1:5433:db.internal:5432"]), "{a:?}");
        assert!(a.windows(2).any(|w| w == ["-D", "127.0.0.1:1081"]));
        assert!(!a.iter().any(|x| x.contains("8080")), "a tunnel that is not auto stays out");
        assert_eq!(a.last().unwrap(), "root@h.example", "the target is still last");
        let t = tunnel_argv(&Store::default(), &s, &s.forwards[1], &[]);
        assert!(!t.iter().any(|x| x.contains("5433") || x.contains("1081")), "a tunnel brings only its own forward: {t:?}");
        let c = scp_argv(&Store::default(), &s, Direction::Up, &["/tmp/x".into()], "", &[]);
        assert!(!c.iter().any(|x| x.contains("5433")), "{c:?}");
    }

    #[test]
    fn what_is_wrong_with_a_proxy_is_said_in_words() {
        let mut st = Store::default();
        let mut s = server(Auth::Agent, "", "root");
        s.proxy = Some(7);
        let id = st.add_server(s);
        assert!(proxy_problem(&st, id).unwrap().contains("no longer exists"));
        let bad = with_proxy(&mut st, socks("pro xy", 1080));
        st.servers[0].proxy = Some(bad);
        assert!(proxy_problem(&st, id).unwrap().contains("host"));
        assert!(socks("-oProxyCommand=evil", 1).problem().is_some() && socks("a;b", 1).problem().is_some() && socks("h", 0).problem().is_some());
        assert!(socks("proxy.corp", 1080).problem().is_none() && socks("::1", 1080).problem().is_none());
        // no proxy asked: nothing to say
        st.servers[0].proxy = None;
        assert_eq!(proxy_problem(&st, id), None);
    }

    #[test]
    fn a_local_terminal_in_a_proxied_profile_gets_the_proxy_variables() {
        let env = proxy_env(&socks("proxy.corp", 1080));
        let get = |k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        for name in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"] {
            assert_eq!(get(name), Some("socks5h://proxy.corp:1080"), "{name}");
        }
        assert_eq!(get("NO_PROXY"), Some("localhost,127.0.0.1,::1"));
        assert_eq!(get("no_proxy"), get("NO_PROXY"));
        let http = Proxy { kind: ProxyKind::Http, ..socks("p", 3128) };
        assert_eq!(proxy_env(&http)[0].1, "http://p:3128");
        assert_eq!(socks("::1", 1).url(), "socks5h://[::1]:1");
    }
}
