//! Everything the window asks for, in one place and with no window in it: the servers and folders, the vault,
//! and the sessions (kept alive by the background server). The Tauri commands are thin wrappers over this, so it
//! is tested without a screen.

use crate::accounts::{self, AccountsView, Runner};
use crate::spaces::{Space, Spaces};
use crate::agent::{self, KeyProvider};
use crate::connect::{self, LOCAL};
use crate::daemon::{self, Daemon, Sink, Stream};
use crate::jobs::{self, Job};
use crate::session::SpawnOpts;
use crate::ssh_import::{self, Preview};
use crate::store::{Auth, DEFAULT_PROFILE, Forward, ForwardKind, NodeId, Profile, Proxy, Server, Store, slug};
use crate::vault::{Kind, SecretMeta, UnlockError, Vault};
use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

pub use crate::daemon::Stream as SessionEvent;

/// A key of `~/.ssh` chosen for the import, with its passphrase when it has one.
#[derive(serde::Deserialize)]
pub struct ImportKey {
    pub path: String,
    pub passphrase: Option<String>,
}

/// What the person chose in the import preview: keys by path, hosts by alias.
#[derive(serde::Deserialize)]
pub struct ImportRequest {
    pub keys: Vec<ImportKey>,
    pub hosts: Vec<String>,
}

#[derive(Serialize, Default, Debug)]
pub struct ImportResult {
    pub keys_added: Vec<String>,
    pub hosts_added: Vec<String>,
    /// What could not be done, in words.
    pub problems: Vec<String>,
    /// The files that were imported, for the question "delete the originals?". oso never deletes them by itself.
    pub originals: Vec<String>,
}

/// A session as the window needs to know it.
#[derive(Serialize, Clone, Debug)]
pub struct SessionInfo {
    pub sid: u64,
    pub title: String,
    pub server_id: u64,
    /// What the window stored with it (screen, position...): returned as it was.
    pub meta: Value,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
pub struct VaultStatus {
    pub exists: bool,
    pub unlocked: bool,
}

/// What an `open` needs from the user before it can go on.
#[derive(Debug, PartialEq)]
pub enum OpenError {
    /// The server (or a hop) has a saved secret and the vault is locked.
    VaultLocked,
    NoSuchServer,
    /// The server is on the Tailscale account `wanted` and `current` is the one in use: ask before connecting.
    Network { wanted: String, current: String },
    Other(String),
}

/// Where the events of every session go (the window).
pub type Emit = Arc<dyn Fn(u64, Stream) + Send + Sync>;

/// Where downloads go when no folder is given.
pub fn downloads_dir() -> String {
    let dirs = directories::UserDirs::new();
    dirs.as_ref()
        .and_then(|d| d.download_dir().map(|p| p.to_path_buf()).or_else(|| Some(d.home_dir().to_path_buf())))
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| ".".into())
}

/// How a tunnel is doing.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct TunnelStatus {
    pub server_id: u64,
    pub forward_id: u64,
    /// `starting`, `up` or `failed`.
    pub state: &'static str,
    pub message: String,
}

/// How a file copy is doing.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct TransferStatus {
    pub id: u64,
    pub server_id: u64,
    pub upload: bool,
    /// What is being copied, for a sentence ("2 files", or the name).
    pub what: String,
    /// `running`, `done` or `failed`.
    pub state: &'static str,
    pub percent: Option<u8>,
    pub message: String,
}

struct Tunnel {
    job: Option<Job>,
    up: bool,
    failed: Option<String>,
}

struct Transfer {
    server_id: u64,
    upload: bool,
    what: String,
    job: Job,
    end: Option<std::result::Result<(), String>>,
}

pub struct Engine {
    pub store: Store,
    pub vault: Vault,
    daemon: Option<Daemon>,
    sessions: HashMap<u64, crate::daemon::RemoteSession>,
    emit: Emit,
    tunnels: HashMap<(u64, u64), Tunnel>,
    transfers: HashMap<u64, Transfer>,
    next_transfer: u64,
    /// Extra options put before the target of every tunnel and copy (the tests use it to reach their own sshd).
    pub ssh_extra: Vec<String>,
    /// The profile the window works in, and whether the sidebar shows every profile's hosts.
    active_profile: String,
    view_all: bool,
    /// How `gh` and `tailscale` are run (the tests give canned answers).
    pub runner: Box<dyn Runner>,
    /// Servers whose network question was answered "connect anyway", once.
    network_ok: std::collections::HashSet<u64>,
    /// Where oso's own ssh-agent listens, once it runs (`start_agent`).
    pub agent_sock: Option<std::path::PathBuf>,
    /// The agent that was there before oso's, which oso's agents hand on to.
    agent_upstream: Option<std::path::PathBuf>,
    /// This engine, as the agents of the Spaces need it (set by `start_agent_at`).
    self_ref: Option<std::sync::Weak<std::sync::Mutex<Engine>>>,
    /// One ssh-agent per Space that has had a terminal: it offers the keys of that Space and no others.
    space_agents: HashMap<u64, agent::Agent>,
    /// The Spaces (saved in `oso-spaces.json`).
    pub spaces: Spaces,
    /// What each agent running in a terminal is doing.
    tracker: crate::agents::Tracker,
    focus: Focus,
}

/// What the person is looking at, as the window tells the engine: names are looked up here, never screen contents.
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct Focus {
    /// The server selected in the Servers panel.
    pub selected: Option<u64>,
    /// The servers that have an ssh session open.
    pub open: Vec<u64>,
    /// The server of the terminal in front (not for a local terminal).
    pub active: Option<u64>,
}

/// Where and how a session is opened, beyond which server it is.
#[derive(Default, Clone, Debug)]
pub struct OpenCtx {
    /// The Space it belongs to.
    pub space: Option<u64>,
    /// The folder a local terminal opens in (default: the first folder of its Space, else the home folder).
    pub cwd: Option<String>,
}

impl Engine {
    pub fn new(store: Store, vault: Vault, emit: Emit) -> Self {
        let settings = crate::settings::Settings::load_from(store.path().parent().unwrap_or(std::path::Path::new(".")));
        let spaces = Spaces::load_from(store.path().parent().unwrap_or(std::path::Path::new(".")));
        let active_profile = settings.active_profile.filter(|p| store.profiles.iter().any(|x| &x.id == p)).unwrap_or_else(|| DEFAULT_PROFILE.to_string());
        Engine { store, vault, daemon: None, sessions: HashMap::new(), emit, tunnels: HashMap::new(), transfers: HashMap::new(), next_transfer: 1, ssh_extra: vec![], active_profile, view_all: settings.view_all_profiles, runner: Box::new(accounts::RealRunner), network_ok: Default::default(), agent_sock: None, agent_upstream: None, self_ref: None, space_agents: HashMap::new(), spaces, tracker: Default::default(), focus: Focus::default() }
    }

    fn config_dir(&self) -> std::path::PathBuf {
        self.store.path().parent().map(|p| p.to_path_buf()).unwrap_or_default()
    }

    /// Connects to the background server, starting it if needed.
    pub fn connect_daemon(&mut self) -> Result<()> {
        match daemon::connect_or_start() {
            Ok(d) => {
                self.daemon = Some(d);
                Ok(())
            }
            Err(daemon::ConnectError::Incompatible(v)) => bail!("A background server of version {v} is running and cannot be used"),
            Err(daemon::ConnectError::Other(e)) => Err(e),
            Err(daemon::ConnectError::None) => bail!("the background server did not start"),
        }
    }

    /// For tests and for running without a background server.
    pub fn use_daemon(&mut self, d: Daemon) {
        self.daemon = Some(d);
    }

    pub fn store_path(&self) -> std::path::PathBuf {
        self.store.path().to_path_buf()
    }

    /// This window answers what AI agents running in the terminals ask of oso.
    pub fn set_agent_handler(&self, f: crate::daemon::AgentHandlerFn) {
        if let Some(d) = &self.daemon {
            d.set_agent_handler(f);
        }
    }

    pub fn daemon_alive(&self) -> bool {
        self.daemon.as_ref().is_some_and(|d| d.alive())
    }

    fn sink_for(&self) -> impl FnOnce(u64) -> Sink + use<> {
        let emit = Arc::clone(&self.emit);
        move |sid| Box::new(move |ev| emit(sid, ev))
    }

    // ---------------------------------------------------------------- sessions

    /// Opens a session for `server_id` (`LOCAL` for this computer). Saved secrets are typed for you when the
    /// vault is unlocked; if the server needs one and it is locked, this says so instead of connecting.
    pub fn open(&mut self, server_id: u64, rows: u16, cols: u16, use_vault: bool) -> std::result::Result<SessionInfo, OpenError> {
        self.open_in(server_id, rows, cols, use_vault, OpenCtx::default())
    }

    /// `open`, for a terminal that belongs to a Space and/or opens in a folder.
    pub fn open_in(&mut self, server_id: u64, rows: u16, cols: u16, use_vault: bool, ctx: OpenCtx) -> std::result::Result<SessionInfo, OpenError> {
        let space = match ctx.space {
            Some(id) => Some(self.spaces.get(id).cloned().ok_or_else(|| OpenError::Other("this Space no longer exists".into()))?),
            None => None,
        };
        if server_id != LOCAL && self.store.server(server_id).is_none() {
            return Err(OpenError::NoSuchServer);
        }
        if server_id != LOCAL {
            if let Some(why) = connect::proxy_problem(&self.store, server_id) {
                return Err(OpenError::Other(why));
            }
            if let Some(why) = connect::key_problem(&self.store, server_id) {
                return Err(OpenError::Other(why));
            }
            if let Some((wanted, current)) = self.network_mismatch(server_id) {
                return Err(OpenError::Network { wanted, current });
            }
        }
        if use_vault && server_id != LOCAL && connect::needs_vault(&self.store, server_id) && !self.vault.is_unlocked() {
            return Err(OpenError::VaultLocked);
        }
        let mut plan = connect::plan(&self.store, &mut self.vault, server_id, rows, cols, use_vault).ok_or(OpenError::NoSuchServer)?;
        if server_id == LOCAL {
            self.prepare_local(&mut plan, use_vault, space.as_ref(), ctx.cwd.as_deref());
        } else {
            self.agent_env(server_id, &mut plan.opts)?;
        }
        let title = plan.title.clone();
        self.spawn(plan.opts, title, server_id, &ctx).map_err(|e| OpenError::Other(format!("{e:#}")))
    }

    /// A terminal of this computer is born inside the active profile (or the one of its Space): its environment says so,
    /// and (when the vault is unlocked) carries the secrets the profile fills in, and the sudo password is typed at sudo's
    /// prompt. In a Space it carries the Space's secrets instead of the profile's, its own ssh-agent, and the Space's
    /// first folder (or `cwd`) as its working folder.
    fn prepare_local(&mut self, plan: &mut connect::Plan, use_vault: bool, space: Option<&Space>, cwd: Option<&str>) {
        let profile = space.and_then(|s| s.profile.clone()).filter(|p| self.store.profiles.iter().any(|x| &x.id == p)).unwrap_or_else(|| self.active_profile.clone());
        let mut env = connect::client_env();
        env.push(("OSO".into(), "1".into()));
        env.push(("OSO_PROFILE".into(), profile.clone()));
        if let Some(sp) = space {
            env.push(("OSO_SPACE".into(), sp.name.clone()));
        }
        let unlocked = use_vault && self.vault.is_unlocked();
        if unlocked {
            let vars = match space {
                Some(sp) => self.vault.env_for_ids(&sp.secrets, true),
                None => self.vault.env_for(&profile, true),
            };
            for (name, value) in vars {
                env.retain(|(k, _)| *k != name);
                env.push((name, value.to_string()));
            }
            plan.opts.sudo_fill = self.vault.get(LOCAL, Kind::Sudo);
        }
        // what the profile asks for (a proxy, an AWS profile, a kubeconfig) goes into this terminal and no other
        for (k, v) in self.profile_env(&profile) {
            env.retain(|(name, _)| *name != k);
            env.push((k, v));
        }
        // git and ssh in this terminal find oso's agent (which hands on to the agent that was there before); a Space
        // gets one that offers only its own keys
        let sock = match space {
            Some(sp) => self.space_agent_sock(sp.id).or_else(|| self.agent_sock.clone()),
            None => self.agent_sock.clone(),
        };
        if let Some(sock) = sock {
            env.retain(|(k, _)| k != "SSH_AUTH_SOCK");
            env.push(("SSH_AUTH_SOCK".into(), sock.to_string_lossy().into_owned()));
        }
        plan.opts.env = Some(env);
        // a terminal of a Space only gets a token from a background server that keeps the Space (an older one would
        // forget it, and the agent would then see more than its Space): without that, no token at all
        plan.opts.agent = self.agent_ok() && (space.is_none() || self.space_ok());
        if let (Some(sp), true) = (space, plan.opts.agent) {
            plan.opts.space = Some(sp.id);
        }
        let folder = cwd.map(crate::spaces::expand_home).or_else(|| space.and_then(Space::first_folder)).filter(|p| p.is_dir());
        if folder.is_some() {
            plan.opts.cwd = folder;
        }
        let name = self.store.profiles.iter().find(|p| p.id == profile).map(|p| p.name.clone()).unwrap_or(profile.clone());
        match space {
            Some(sp) => {
                plan.title = format!("{} · {}", plan.title, sp.name);
                if !plan.opts.agent {
                    plan.title.push_str(" · no agent access (restart oso's background server)");
                }
            }
            None if profile != DEFAULT_PROFILE => plan.title = format!("{} · {name}", plan.title),
            None => {}
        }
        if self.vault.exists() && !unlocked {
            plan.title.push_str(" · vault locked");
        }
    }

    /// A connection that uses a key from the vault finds oso's agent through `SSH_AUTH_SOCK`.
    pub fn agent_env(&self, server_id: u64, opts: &mut SpawnOpts) -> std::result::Result<(), OpenError> {
        if !connect::uses_vault_key(&self.store, server_id) {
            return Ok(());
        }
        let sock = self.agent_sock.as_ref().ok_or_else(|| OpenError::Other("oso's ssh-agent is not running, so a key kept in the vault cannot be used".into()))?;
        let mut env = opts.env.take().unwrap_or_else(connect::client_env);
        env.retain(|(k, _)| k != "SSH_AUTH_SOCK");
        env.push(("SSH_AUTH_SOCK".into(), sock.to_string_lossy().into_owned()));
        opts.env = Some(env);
        Ok(())
    }

    /// What a profile adds to the environment of a program that is not a secret: its proxy and its cloud settings.
    pub fn profile_env(&self, profile: &str) -> Vec<(String, String)> {
        let mut env = vec![];
        let Some(p) = self.store.profiles.iter().find(|p| p.id == profile) else { return env };
        if let Some(px) = p.proxy.and_then(|id| self.store.proxy(id)).filter(|px| px.problem().is_none()) {
            env.extend(connect::proxy_env(px));
        }
        if let Some(a) = p.aws_profile.as_deref().filter(|a| !a.is_empty()) {
            env.push(("AWS_PROFILE".into(), a.to_string()));
        }
        if let Some(k) = p.kubeconfig.as_deref().filter(|k| !k.is_empty()) {
            env.push(("KUBECONFIG".into(), connect::expand_tilde(k)));
        }
        env
    }

    /// The AWS profiles and kubeconfigs that exist, by name (nothing secret is read).
    pub fn cloud_options(&self) -> crate::cloud::CloudOptions {
        let home = directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf()).unwrap_or_default();
        crate::cloud::options(&home, std::env::var("KUBECONFIG").ok().as_deref())
    }

    /// The cloud settings of a profile's local terminals, and whether it is a production one (shown in red).
    pub fn profile_set_cloud(&mut self, profile: &str, aws_profile: Option<String>, kubeconfig: Option<String>, production: bool) -> Result<()> {
        let clean = |v: Option<String>| v.map(|x| x.trim().to_string()).filter(|x| !x.is_empty());
        let aws = clean(aws_profile);
        if aws.as_deref().is_some_and(|a| a.starts_with('-') || a.chars().any(|c| c.is_whitespace() || c.is_control())) {
            bail!("an AWS profile name has no spaces");
        }
        let kube = clean(kubeconfig);
        if kube.as_deref().is_some_and(|k| k.contains('\0') || k.contains('\n')) {
            bail!("that is not a path");
        }
        let p = self.store.profiles.iter_mut().find(|p| p.id == profile).context("this profile no longer exists")?;
        p.aws_profile = aws;
        p.kubeconfig = kube;
        p.production = production;
        self.store.save()
    }

    /// Does the background server in use keep the Space of a session (version 0.4 and later)?
    pub fn space_ok(&self) -> bool {
        let v = self.daemon.as_ref().map(|d| d.server_version.as_str()).unwrap_or("");
        let mut parts = v.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
        (parts.next().unwrap_or(0), parts.next().unwrap_or(0)) >= (0, 4)
    }

    /// Does the background server in use know about tokens (an older one, still running, does not)?
    pub fn agent_ok(&self) -> bool {
        let v = self.daemon.as_ref().map(|d| d.server_version.as_str()).unwrap_or("");
        let mut parts = v.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
        (parts.next().unwrap_or(0), parts.next().unwrap_or(0)) >= (0, 2)
    }

    fn spawn(&mut self, opts: SpawnOpts, title: String, server_id: u64, ctx: &OpenCtx) -> Result<SessionInfo> {
        let make = self.sink_for();
        let d = self.daemon.as_ref().context("the background server is not running")?;
        let session = d.spawn_stream(opts, make)?;
        let sid = session.sid();
        let mut meta = serde_json::json!({ "title": title, "server_id": server_id });
        if let Some(id) = ctx.space {
            meta["space"] = id.into();
        }
        if let Some(c) = &ctx.cwd {
            meta["cwd"] = c.clone().into();
        }
        session.set_meta(&meta);
        self.sessions.insert(sid, session);
        Ok(SessionInfo { sid, title, server_id, meta })
    }

    /// Starts the same connection again in place of a session that ended (Enter on a dead terminal).
    pub fn reconnect(&mut self, sid: u64, rows: u16, cols: u16, use_vault: bool) -> std::result::Result<SessionInfo, OpenError> {
        let meta = self.sessions.get(&sid).map(|_| ()).and(self.meta_of(sid));
        let server_id = meta.as_ref().and_then(|m| m.get("server_id")).and_then(Value::as_u64).ok_or(OpenError::NoSuchServer)?;
        let ctx = OpenCtx { space: meta.as_ref().and_then(|m| m.get("space")).and_then(Value::as_u64), cwd: meta.as_ref().and_then(|m| m.get("cwd")).and_then(Value::as_str).map(str::to_string) };
        let mut info = self.open_in(server_id, rows, cols, use_vault, ctx)?;
        // the new session keeps what the window had stored for the old one (its screen and place)
        if let Some(old) = meta {
            let mut merged = old;
            if let Some(o) = merged.as_object_mut() {
                o.insert("title".into(), info.title.clone().into());
            }
            self.set_meta(info.sid, &merged);
            info.meta = merged;
        }
        self.close(sid);
        Ok(info)
    }

    fn meta_of(&self, sid: u64) -> Option<Value> {
        self.daemon.as_ref()?.list().ok()?.into_iter().find(|i| i.sid == sid).map(|i| i.meta)
    }

    /// Sessions the background server was already holding (the window was closed and opened again): attaches to
    /// each and returns what is needed to draw them where they were.
    pub fn restore(&mut self, rows: u16, cols: u16) -> Result<Vec<SessionInfo>> {
        let list = self.daemon.as_ref().context("the background server is not running")?.list()?;
        let mut out = vec![];
        for info in list {
            if self.sessions.contains_key(&info.sid) {
                continue;
            }
            let make = self.sink_for();
            let d = self.daemon.as_ref().unwrap();
            let session = d.attach_stream(info.sid, rows, cols, make(info.sid));
            self.sessions.insert(info.sid, session);
            let title = info.meta.get("title").and_then(Value::as_str).unwrap_or("session").to_string();
            let server_id = info.meta.get("server_id").and_then(Value::as_u64).unwrap_or(LOCAL);
            out.push(SessionInfo { sid: info.sid, title, server_id, meta: info.meta });
        }
        out.sort_by_key(|s| s.meta.get("order").and_then(Value::as_u64).unwrap_or(u64::MAX));
        Ok(out)
    }

    pub fn write(&self, sid: u64, bytes: &[u8]) {
        if let Some(s) = self.sessions.get(&sid) {
            s.write(bytes);
        }
    }

    pub fn resize(&mut self, sid: u64, rows: u16, cols: u16) {
        if let Some(s) = self.sessions.get_mut(&sid) {
            s.resize(rows, cols);
        }
    }

    /// Ends the session in the background server.
    pub fn close(&mut self, sid: u64) {
        if let Some(mut s) = self.sessions.remove(&sid) {
            s.kill();
        }
    }

    /// Lets go of every session without ending them (the window is closing): they keep running.
    pub fn detach_all(&mut self) {
        self.sessions.clear();
    }

    /// What the window keeps for a session (its screen and place), saved in the background server so that it
    /// comes back where it was.
    pub fn set_meta(&self, sid: u64, meta: &Value) {
        if let Some(s) = self.sessions.get(&sid) {
            s.set_meta(meta);
        }
    }

    /// Types the sudo password saved for the session's server.
    pub fn fill_sudo(&mut self, sid: u64, server_id: u64) -> Result<()> {
        if !self.vault.is_unlocked() {
            bail!("the vault is locked");
        }
        let mut pw = self.vault.get(server_id, Kind::Sudo).ok_or_else(|| anyhow!("no sudo password is saved for this server"))?;
        if let Some(s) = self.sessions.get(&sid) {
            s.write(format!("{pw}\r").as_bytes());
        }
        zeroize::Zeroize::zeroize(&mut pw);
        Ok(())
    }

    // ---------------------------------------------------------------- vault

    pub fn vault_status(&self) -> VaultStatus {
        VaultStatus { exists: self.vault.exists(), unlocked: self.vault.is_unlocked() }
    }

    /// Creates the vault when there is none, else unlocks it. `Err` carries what to tell the user.
    pub fn vault_unlock(&mut self, master: &str) -> std::result::Result<(), String> {
        if !self.vault.exists() {
            return self.vault.create(master).map_err(|e| format!("{e:#}"));
        }
        self.vault.unlock(master).map_err(|e| match e {
            UnlockError::WrongPassword => "Wrong master password".to_string(),
            UnlockError::Other(m) => m,
        })?;
        // an old vault was migrated by number: name its secrets after their servers
        let names: HashMap<u64, String> = self.store.servers.iter().map(|s| (s.id, s.name.clone())).chain([(LOCAL, "this computer".to_string())]).collect();
        self.vault.relabel_hosts(|id| names.get(&id).cloned()).map_err(|e| format!("{e:#}"))?;
        self.refresh_key_files();
        Ok(())
    }

    fn host_label(&self, server_id: u64) -> String {
        if server_id == LOCAL { "this computer".into() } else { self.store.server(server_id).map(|s| s.name.clone()).unwrap_or_else(|| format!("server {server_id}")) }
    }

    pub fn vault_lock(&mut self) {
        self.vault.lock();
    }

    /// A saved secret, to show or copy (the vault must be unlocked).
    pub fn vault_reveal(&mut self, server_id: u64, sudo: bool) -> Result<Option<String>> {
        if !self.vault.is_unlocked() {
            bail!("the vault is locked");
        }
        Ok(self.vault.get(server_id, if sudo { Kind::Sudo } else { Kind::Login }))
    }

    /// Saves (or, with `None`, removes) a secret of a server (`LOCAL`: of this computer) and keeps the server's flags in step.
    pub fn vault_set(&mut self, server_id: u64, sudo: bool, secret: Option<&str>) -> Result<()> {
        if !self.vault.is_unlocked() {
            bail!("the vault is locked");
        }
        if server_id != LOCAL && self.store.server(server_id).is_none() {
            bail!("this server no longer exists");
        }
        let secret = secret.filter(|s| !s.is_empty());
        let label = self.host_label(server_id);
        self.vault.set(server_id, if sudo { Kind::Sudo } else { Kind::Login }, secret, &label)?;
        self.set_flags(server_id, |s| if sudo { s.has_sudo = secret.is_some() } else { s.has_secret = secret.is_some() });
        self.store.save()
    }

    // ---------------------------------------------------------------- secrets

    /// Every secret's description, never its value.
    pub fn secret_list(&self) -> Result<Vec<SecretMeta>> {
        if !self.vault.is_unlocked() {
            bail!("the vault is locked");
        }
        Ok(self.vault.list())
    }

    /// Creates (`meta.id == 0`) or changes a secret; `None` keeps the value of an existing one. (The window offers
    /// "agents may see it" switched on for an API key and off for the rest.)
    pub fn secret_save(&mut self, meta: SecretMeta, value: Option<&str>) -> Result<u64> {
        let normalized = match (meta.kind, value.filter(|v| !v.trim().is_empty())) {
            (crate::vault::SecretKind::SshKey, Some(v)) => Some(check_ssh_key(v)?),
            (crate::vault::SecretKind::Totp, Some(v)) => {
                crate::totp::parse(v)?;
                Some(v.trim().to_string())
            }
            (_, v) => v.map(str::to_string),
        };
        let id = self.vault.save_secret(meta, normalized.as_deref())?;
        self.refresh_key_files();
        Ok(id)
    }

    /// Deletes a secret and keeps the flags of the servers that used it in step.
    pub fn secret_delete(&mut self, id: u64) -> Result<()> {
        let touched = self.vault.delete_secret(id)?;
        self.refresh_key_files();
        for server in touched {
            let links = self.vault.links(server);
            self.set_flags(server, |s| {
                s.has_secret = links.login.is_some();
                s.has_sudo = links.sudo.is_some();
                s.has_totp = links.totp.is_some();
            });
        }
        self.store.save()
    }

    /// The code a one-time-code secret shows now, and the seconds it still lasts. The seed does not leave.
    pub fn totp_code(&mut self, id: u64) -> Result<(String, u64)> {
        if !self.vault.is_unlocked() {
            bail!("the vault is locked");
        }
        let meta = self.vault.meta(id).context("this secret no longer exists")?;
        if meta.kind != crate::vault::SecretKind::Totp {
            bail!("that secret does not make one-time codes");
        }
        let seed = zeroize::Zeroizing::new(self.vault.reveal(id).context("this secret no longer exists")?);
        let spec = crate::totp::parse(&seed)?;
        let now = crate::totp::now();
        Ok((crate::totp::code(&spec, now), crate::totp::seconds_left(&spec, now)))
    }

    /// The server asks for one-time codes after the password; this seed (`None`: none) answers them.
    pub fn server_set_totp(&mut self, server_id: u64, secret: Option<u64>) -> Result<()> {
        if !self.vault.is_unlocked() {
            bail!("the vault is locked");
        }
        if self.store.server(server_id).is_none() {
            bail!("this server no longer exists");
        }
        self.vault.set_totp_link(server_id, secret)?;
        self.set_flags(server_id, |s| s.has_totp = secret.is_some());
        self.store.save()
    }

    /// A secret's value, to show or copy (the window clears the clipboard after 30 seconds).
    pub fn secret_reveal(&mut self, id: u64) -> Result<String> {
        if !self.vault.is_unlocked() {
            bail!("the vault is locked");
        }
        self.vault.reveal(id).ok_or_else(|| anyhow!("this secret no longer exists"))
    }

    pub fn vault_change_master(&mut self, new_master: &str) -> Result<()> {
        if new_master.is_empty() {
            bail!("the master password cannot be empty");
        }
        self.vault.change_master(new_master)
    }

    // ---------------------------------------------------------------- servers

    /// The folders and servers of the active profile (or of every one), for the sidebar.
    pub fn tree(&self) -> Value {
        let p = self.active_profile.as_str();
        let (folders, servers): (Vec<&crate::store::Folder>, Vec<&Server>) = if self.view_all {
            (self.store.folders.iter().collect(), self.store.servers.iter().collect())
        } else {
            let shown = self.store.folders_in(p);
            // a hop of a shown server stays in the list, whatever its profile: the server's form needs to name it
            let mut ids: Vec<u64> = self.store.servers.iter().filter(|s| self.store.server_in(s, p)).map(|s| s.id).collect();
            for id in ids.clone() {
                ids.extend(self.store.jump_chain(id).into_iter().map(|h| h.id));
            }
            (self.store.folders.iter().filter(|f| shown.contains(&f.id)).collect(), self.store.servers.iter().filter(|s| ids.contains(&s.id)).collect())
        };
        // a server also says which profile it ends up in (its own or its folder's)
        let servers: Vec<Value> = servers
            .into_iter()
            .map(|s| {
                let mut v = serde_json::to_value(s).unwrap_or(Value::Null);
                v["profile_eff"] = self.store.server_profile(s).map(Value::from).unwrap_or(Value::Null);
                v["vault_key"] = self.vault.links(s.id).key.map(Value::from).unwrap_or(Value::Null);
                v["vault_totp"] = self.vault.links(s.id).totp.map(Value::from).unwrap_or(Value::Null);
                v
            })
            .collect();
        serde_json::json!({ "folders": folders, "servers": servers, "profiles": self.store.profiles, "proxies": self.store.proxies, "active_profile": self.active_profile, "view_all": self.view_all })
    }

    // ---------------------------------------------------------------- profiles

    pub fn profiles(&self) -> &[Profile] {
        &self.store.profiles
    }

    pub fn active_profile(&self) -> &str {
        &self.active_profile
    }

    pub fn set_active_profile(&mut self, id: &str) -> Result<()> {
        if !self.store.profiles.iter().any(|p| p.id == id) {
            bail!("there is no profile {id:?}");
        }
        self.active_profile = id.to_string();
        crate::settings::save_key_in(&self.config_dir(), "active_profile", id.into());
        Ok(())
    }

    pub fn set_view_all(&mut self, all: bool) {
        self.view_all = all;
        crate::settings::save_key_in(&self.config_dir(), "view_all_profiles", all.into());
    }

    /// Adds a profile (`id` empty) or renames one. Returns its id.
    pub fn profile_save(&mut self, id: &str, name: &str) -> Result<String> {
        let name = name.trim();
        if name.is_empty() {
            bail!("a profile needs a name");
        }
        if id.is_empty() {
            let id = slug(name);
            if id.is_empty() {
                bail!("a profile name needs letters or digits");
            }
            if self.store.profiles.iter().any(|p| p.id == id) {
                bail!("there is already a profile called {name:?}");
            }
            self.store.profiles.push(Profile { id: id.clone(), name: name.to_string(), ..Default::default() });
            self.store.save()?;
            return Ok(id);
        }
        let p = self.store.profiles.iter_mut().find(|p| p.id == id).context("this profile no longer exists")?;
        p.name = name.to_string();
        self.store.save()?;
        Ok(id.to_string())
    }

    /// Deletes a profile; what belonged to it moves to the default one. The vault must be unlocked, so that no
    /// secret of the profile is left without it (and so shown in every profile).
    pub fn profile_delete(&mut self, id: &str) -> Result<()> {
        if id == DEFAULT_PROFILE {
            bail!("the default profile cannot be deleted");
        }
        if !self.store.profiles.iter().any(|p| p.id == id) {
            return Ok(());
        }
        if !self.vault.is_unlocked() && self.vault.exists() {
            bail!("unlock the vault first: its secrets of this profile move to the default one");
        }
        for m in self.vault.list().into_iter().filter(|m| m.profile.as_deref() == Some(id)) {
            self.vault.save_secret(SecretMeta { profile: Some(DEFAULT_PROFILE.into()), ..m }, None)?;
        }
        let mine = Some(id.to_string());
        let default = Some(DEFAULT_PROFILE.to_string());
        for s in self.store.servers.iter_mut().filter(|s| s.profile == mine) {
            s.profile = default.clone();
        }
        for f in self.store.folders.iter_mut().filter(|f| f.profile == mine) {
            f.profile = default.clone();
        }
        self.store.profiles.retain(|p| p.id != id);
        self.store.save()?;
        if self.active_profile == id {
            self.set_active_profile(DEFAULT_PROFILE)?;
        }
        Ok(())
    }

    // ---------------------------------------------------------------- ssh keys and oso's agent

    /// Starts oso's ssh-agent on its usual socket, handing on to the agent that was running before (if any).
    pub fn start_agent(engine: &Arc<std::sync::Mutex<Engine>>) -> Result<agent::Agent> {
        let path = agent::socket_path();
        let upstream = std::env::var_os("SSH_AUTH_SOCK").map(std::path::PathBuf::from).filter(|p| p.exists());
        Self::start_agent_at(engine, path, upstream)
    }

    pub fn start_agent_at(engine: &Arc<std::sync::Mutex<Engine>>, path: std::path::PathBuf, upstream: Option<std::path::PathBuf>) -> Result<agent::Agent> {
        let keys: Arc<dyn KeyProvider> = Arc::clone(engine) as Arc<dyn KeyProvider>;
        let a = agent::start(path.clone(), keys, upstream.clone())?;
        if let Ok(mut e) = engine.lock() {
            e.agent_sock = Some(path);
            e.agent_upstream = upstream;
            e.self_ref = Some(Arc::downgrade(engine));
        }
        Ok(a)
    }

    /// The socket of the ssh-agent of a Space (started the first time it is needed). It offers the keys of the Space and
    /// no other key of the vault, and hands on to the agent that was there before oso's.
    pub fn space_agent_sock(&mut self, space: u64) -> Option<std::path::PathBuf> {
        if let Some(a) = self.space_agents.get(&space) {
            return Some(a.path().to_path_buf());
        }
        let base = self.agent_sock.clone()?;
        let me = self.self_ref.clone()?;
        let path = base.with_file_name(format!("agent-space-{space}.sock"));
        let keys: Arc<dyn KeyProvider> = Arc::new(SpaceKeys { engine: me, space });
        match agent::start(path.clone(), keys, self.agent_upstream.clone()) {
            Ok(a) => {
                self.space_agents.insert(space, a);
                Some(path)
            }
            // never fall back to the agent that offers every key: better the one the person had before oso
            Err(_) => self.agent_upstream.clone(),
        }
    }

    /// The keys of a Space (an id of a key that is not in the vault any more is simply not there).
    pub fn space_identities(&self, space: u64) -> Vec<(Vec<u8>, String)> {
        let ids = self.spaces.get(space).map(|s| s.secrets.clone()).unwrap_or_default();
        self.parsed_keys().iter().filter(|(id, _, _)| ids.contains(id)).filter_map(|(_, k, name)| agent::identity_of(k, name)).collect()
    }

    pub fn space_sign(&self, space: u64, blob: &[u8], data: &[u8], flags: u32) -> Option<Vec<u8>> {
        let ids = self.spaces.get(space).map(|s| s.secrets.clone()).unwrap_or_default();
        let (_, key, _) = self.parsed_keys().into_iter().filter(|(id, _, _)| ids.contains(id)).find(|(_, k, _)| k.public_key().to_bytes().ok().as_deref() == Some(blob))?;
        agent::sign_with(&key, data, flags)
    }

    // ---------------------------------------------------------------- focus

    /// The window says what the person has selected and open (so that an agent can be told "this server").
    pub fn set_focus(&mut self, focus: Focus) {
        self.focus = focus;
    }

    pub fn focus(&self) -> &Focus {
        &self.focus
    }

    // ---------------------------------------------------------------- agents

    pub fn agent_kinds(&self) -> Vec<crate::agents::AgentKind> {
        crate::agents::kinds_in(&self.config_dir())
    }

    pub fn agent_kinds_save(&mut self, kinds: &[crate::agents::AgentKind]) -> std::result::Result<(), String> {
        crate::agents::save_kinds(&self.config_dir(), kinds)
    }

    /// The agents running in the terminals of this window, and what each is doing. `pending`: the terminals that have a
    /// request waiting in oso (the window knows which: it is the one asked).
    pub fn agent_status(&mut self, pending: &[u64]) -> Vec<crate::agents::Status> {
        let kinds = self.agent_kinds();
        let live: std::collections::HashSet<u64> = self.sessions.keys().copied().collect();
        let mut found = HashMap::new();
        for (sid, procs) in crate::procscan::scan_panes() {
            if !live.contains(&sid) {
                continue;
            }
            if let Some(d) = crate::agents::detect(&kinds, &procs, std::path::Path::new("/proc")) {
                found.insert(sid, d);
            }
        }
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        self.tracker.update(now, &found, pending, &live)
    }

    /// An agent's own hook said something in terminal `sid` (the background server vouched for the terminal).
    pub fn agent_event(&mut self, sid: u64, agent: &str, event: &str) -> bool {
        let Some(state) = crate::agents::state_for(agent, event) else { return false };
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        let kinds = self.agent_kinds();
        self.tracker.event(sid, agent, state, now, &kinds);
        true
    }

    /// The person looked at this terminal.
    pub fn agent_ack(&mut self, sid: u64) {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        self.tracker.ack(sid, now);
    }

    // ---------------------------------------------------------------- spaces

    pub fn spaces_list(&self) -> Vec<Space> {
        self.spaces.list().to_vec()
    }

    pub fn space_save(&mut self, space: &Space) -> Result<u64> {
        self.spaces.save_space(space)
    }

    /// Deletes a Space. Terminals that were opened in it stay open, but an agent running there is refused from now on.
    pub fn space_delete(&mut self, id: u64) -> Result<()> {
        self.space_agents.remove(&id);
        self.spaces.delete(id)
    }

    /// The keys of the vault that can sign (an encrypted one cannot), with the secret's name as comment.
    fn parsed_keys(&self) -> Vec<(u64, ssh_key::PrivateKey, String)> {
        self.vault
            .ssh_keys()
            .into_iter()
            .filter_map(|(id, name, text)| ssh_key::PrivateKey::from_openssh(text.as_bytes()).ok().filter(|k| !k.is_encrypted()).map(|k| (id, k, name)))
            .collect()
    }

    pub fn agent_identities(&self) -> Vec<(Vec<u8>, String)> {
        self.parsed_keys().iter().filter_map(|(_, k, name)| agent::identity_of(k, name)).collect()
    }

    pub fn agent_sign(&self, blob: &[u8], data: &[u8], flags: u32) -> Option<Vec<u8>> {
        let (_, key, _) = self.parsed_keys().into_iter().find(|(_, k, _)| k.public_key().to_bytes().ok().as_deref() == Some(blob))?;
        agent::sign_with(&key, data, flags)
    }

    /// The server logs in with this key of the vault (`None`: unlink). The server's authentication becomes
    /// "key from the vault" when a key is given.
    pub fn server_set_key(&mut self, server_id: u64, secret: Option<u64>) -> Result<()> {
        if !self.vault.is_unlocked() {
            bail!("the vault is locked");
        }
        let mut s = self.store.server(server_id).context("this server no longer exists")?.clone();
        self.vault.set_key_link(server_id, secret)?;
        if secret.is_some() {
            s.auth = Auth::VaultKey;
            self.store.update_server(s);
            self.store.save()?;
        }
        self.refresh_key_files();
        Ok(())
    }

    /// Writes the public half of the key of every server that has one to `<config>/keys/`, where `ssh -i` finds it.
    /// Public halves are not secrets.
    fn refresh_key_files(&mut self) {
        self.store.key_files.clear();
        if !self.vault.is_unlocked() {
            return;
        }
        let dir = self.config_dir().join("keys");
        let keys: HashMap<u64, (ssh_key::PrivateKey, String)> = self.parsed_keys().into_iter().map(|(id, k, name)| (id, (k, name))).collect();
        let ids: Vec<u64> = self.store.servers.iter().map(|s| s.id).collect();
        for server in ids {
            let Some(secret) = self.vault.links(server).key else { continue };
            let Some((key, name)) = keys.get(&secret) else { continue };
            let Ok(mut text) = key.public_key().to_openssh() else { continue };
            text.push('\n');
            let file = dir.join(format!("{secret}-{}.pub", slug(name)));
            let written = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&file, text));
            if written.is_ok() {
                self.store.key_files.insert(server, file);
            }
        }
    }

    // ---------------------------------------------------------------- importing from ~/.ssh

    fn ssh_dir(dir: Option<&str>) -> (std::path::PathBuf, std::path::PathBuf) {
        let home = directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf()).unwrap_or_default();
        (dir.map(std::path::PathBuf::from).unwrap_or_else(|| home.join(".ssh")), home)
    }

    /// What `~/.ssh` has that could come into oso (read only).
    pub fn import_preview(&self, dir: Option<&str>) -> Preview {
        let (dir, home) = Self::ssh_dir(dir);
        ssh_import::scan(&dir, &home)
    }

    /// Brings the chosen keys (decrypted with the passphrase given, then kept in the vault) and hosts into oso.
    /// The originals are not touched. A host that uses an imported key logs in with it from the vault.
    pub fn import_apply(&mut self, dir: Option<&str>, req: ImportRequest) -> Result<ImportResult> {
        if !self.vault.is_unlocked() {
            bail!("the vault is locked");
        }
        let (dir, home) = Self::ssh_dir(dir);
        let root = dir.canonicalize().with_context(|| format!("{} is not there", dir.display()))?;
        let preview = ssh_import::scan(&dir, &home);
        let mut out = ImportResult::default();
        // secret ids by canonical path of the file they came from
        let mut by_path: HashMap<std::path::PathBuf, u64> = HashMap::new();
        let known: Vec<(Vec<u8>, u64)> = self.parsed_keys().into_iter().filter_map(|(id, k, _)| k.public_key().to_bytes().ok().map(|b| (b, id))).collect();

        for k in req.keys {
            let Ok(path) = std::path::Path::new(&k.path).canonicalize() else {
                out.problems.push(format!("{} is not there", k.path));
                continue;
            };
            if !path.starts_with(&root) {
                out.problems.push(format!("{} is not inside {}", k.path, root.display()));
                continue;
            }
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if preview.keys.iter().find(|f| std::path::Path::new(&f.path).canonicalize().ok().as_deref() == Some(&path)).is_some_and(|f| !f.unsupported.is_empty()) {
                out.problems.push(format!("{name}: kept as a file (a security key or an old format cannot go into the vault)"));
                continue;
            }
            let text = match std::fs::read_to_string(&path) {
                Ok(t) => t,
                Err(e) => {
                    out.problems.push(format!("{name}: {e}"));
                    continue;
                }
            };
            let mut key = match ssh_key::PrivateKey::from_openssh(text.trim().as_bytes()) {
                Ok(key) => key,
                Err(_) => {
                    out.problems.push(format!("{name}: not a key oso can read"));
                    continue;
                }
            };
            if key.is_encrypted() {
                let pass = zeroize::Zeroizing::new(k.passphrase.unwrap_or_default());
                match key.decrypt(pass.as_bytes()) {
                    Ok(plain) => key = plain,
                    Err(_) => {
                        out.problems.push(format!("{name}: wrong passphrase (or none given)"));
                        continue;
                    }
                }
            }
            let blob = key.public_key().to_bytes().unwrap_or_default();
            if let Some((_, id)) = known.iter().find(|(b, _)| *b == blob) {
                out.problems.push(format!("{name}: already in the vault"));
                by_path.insert(path, *id);
                continue;
            }
            let plain = zeroize::Zeroizing::new(key.to_openssh(ssh_key::LineEnding::LF).map_err(|e| anyhow!("{name}: {e}"))?.to_string());
            let meta = SecretMeta { id: 0, name: name.clone(), kind: crate::vault::SecretKind::SshKey, profile: None, env_var: None, agent_visible: false, notes: format!("imported from {}", path.display()), updated: 0 };
            let id = self.vault.save_secret(meta, Some(plain.as_str()))?;
            by_path.insert(path.clone(), id);
            out.keys_added.push(name);
            out.originals.push(path.to_string_lossy().into_owned());
        }
        self.refresh_key_files();

        // hosts, with the hops they go through
        let config: HashMap<String, ssh_import::HostEntry> = preview.hosts.iter().map(|h| (h.alias.clone(), h.clone())).collect();
        for alias in &req.hosts {
            match config.get(alias) {
                Some(h) => {
                    if let Err(e) = self.import_host(h, &config, &by_path, &mut out, 0) {
                        out.problems.push(format!("{alias}: {e:#}"));
                    }
                }
                None => out.problems.push(format!("{alias}: not in the config")),
            }
        }
        self.store.save()?;
        self.refresh_key_files();
        Ok(out)
    }

    /// The id of a server for this config entry: the one already there, or a new one.
    fn import_host(&mut self, h: &ssh_import::HostEntry, config: &HashMap<String, ssh_import::HostEntry>, keys: &HashMap<std::path::PathBuf, u64>, out: &mut ImportResult, depth: u8) -> Result<u64> {
        if let Some(s) = self.store.servers.iter().find(|s| s.name == h.alias || (s.host == h.host && s.port == h.port && s.user == h.user)) {
            if depth == 0 {
                out.problems.push(format!("{}: already a server here", h.alias));
            }
            return Ok(s.id);
        }
        if depth > 8 {
            bail!("too many hops");
        }
        // hops first: the last one named is the one this server is reached through
        let mut previous: Option<u64> = None;
        if let Some(j) = &h.proxy_jump {
            for (user, host, port) in ssh_import::parse_jump(j) {
                let entry = config.get(&host).cloned().unwrap_or_else(|| ssh_import::HostEntry { alias: host.clone(), host: host.clone(), user: user.clone().unwrap_or_else(|| h.user.clone()), port: port.unwrap_or(22), ..Default::default() });
                let id = self.import_host(&entry, config, keys, out, depth + 1)?;
                if let Some(prev) = previous.filter(|p| self.store.server(id).is_some_and(|s| s.jump.is_none()) && !self.store.would_cycle(id, *p)) {
                    self.store.move_via(id, Some(prev));
                }
                previous = Some(id);
            }
        }
        let key = h.identity_file.as_ref().and_then(|f| std::path::Path::new(f).canonicalize().ok());
        let secret = key.as_ref().and_then(|p| keys.get(p)).copied();
        let (auth, key_path) = match (&secret, &key) {
            (Some(_), _) => (Auth::VaultKey, String::new()),
            (None, Some(p)) if p.is_file() => (Auth::Key, p.to_string_lossy().into_owned()),
            _ => (Auth::Agent, String::new()),
        };
        let server = Server { name: h.alias.clone(), host: h.host.clone(), port: h.port, user: h.user.clone(), auth, key_path, jump: previous, ..Default::default() };
        let id = self.store.add_server(server);
        if let Some(sec) = secret {
            self.vault.set_key_link(id, Some(sec))?;
        }
        out.hosts_added.push(h.alias.clone());
        Ok(id)
    }

    /// Deletes the imported key files (and their `.pub`) the person asked to be deleted. Only `id_*` files inside the
    /// folder that was scanned are ever touched; the window asks first and offers "keep them" as the first choice.
    pub fn import_delete_originals(&mut self, dir: Option<&str>, paths: &[String]) -> Result<Vec<String>> {
        let (dir, _) = Self::ssh_dir(dir);
        let root = dir.canonicalize().context("that folder is not there")?;
        let mut gone = vec![];
        for p in paths {
            let Ok(path) = std::path::Path::new(p).canonicalize() else { continue };
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if !path.starts_with(&root) || path.parent() != Some(root.as_path()) || !name.starts_with("id_") || name.ends_with(".pub") {
                continue;
            }
            if std::fs::remove_file(&path).is_ok() {
                gone.push(name.clone());
                let _ = std::fs::remove_file(path.with_file_name(format!("{name}.pub")));
            }
        }
        Ok(gone)
    }

    // ---------------------------------------------------------------- accounts

    /// The accounts of GitHub and Tailscale and which is active. A tool that is not installed says so (`installed: false`).
    pub fn accounts(&self) -> AccountsView {
        AccountsView { github: accounts::github(&*self.runner), tailscale: accounts::tailscale(&*self.runner) }
    }

    /// Switches the account of a tool (`github` or `tailscale`).
    pub fn account_use(&mut self, tool: &str, id: &str) -> Result<()> {
        match tool {
            "github" => accounts::github_switch(&*self.runner, id),
            "tailscale" => accounts::tailscale_switch(&*self.runner, id),
            _ => Err("unknown tool".into()),
        }
        .map_err(|m| anyhow!("{m}"))
    }

    /// The accounts a profile asks for (`None`: leave the tool alone).
    pub fn profile_set_accounts(&mut self, profile: &str, gh_user: Option<String>, tailscale_account: Option<String>) -> Result<()> {
        let clean = |v: Option<String>| v.map(|x| x.trim().to_string()).filter(|x| !x.is_empty());
        let p = self.store.profiles.iter_mut().find(|p| p.id == profile).context("this profile no longer exists")?;
        p.gh_user = clean(gh_user);
        p.tailscale_account = clean(tailscale_account);
        self.store.save()
    }

    /// Makes `id` the active profile and puts the accounts it asks for in use. A switch that fails is reported in the
    /// answer, and does not stop the others.
    pub fn switch_profile(&mut self, id: &str) -> Result<Vec<String>> {
        self.set_active_profile(id)?;
        let p = self.store.profiles.iter().find(|p| p.id == id).cloned().unwrap_or_default();
        let mut problems = vec![];
        if let Some(user) = p.gh_user.as_deref() {
            let view = accounts::github(&*self.runner);
            if !view.installed {
                problems.push("GitHub: `gh` is not installed".to_string());
            } else if !accounts::is_active(&view, user) {
                let id = view.accounts.iter().find(|a| [&a.id, &a.name].iter().any(|x| x.eq_ignore_ascii_case(user))).map(|a| a.id.clone()).unwrap_or_else(|| user.to_string());
                if let Err(e) = accounts::github_switch(&*self.runner, &id) {
                    problems.push(e);
                }
            }
        }
        if let Some(acct) = p.tailscale_account.as_deref() {
            let view = accounts::tailscale(&*self.runner);
            if !view.installed {
                problems.push("Tailscale: `tailscale` is not installed".to_string());
            } else if !accounts::is_active(&view, acct) {
                if let Err(e) = accounts::tailscale_switch(&*self.runner, acct) {
                    problems.push(e);
                }
            }
        }
        Ok(problems)
    }

    /// For a server on `tailscale:<account>`: `(wanted, in use)` when they differ. Once the question was answered
    /// "connect anyway" (`network_accept`), the next open goes through.
    pub fn network_mismatch(&mut self, server_id: u64) -> Option<(String, String)> {
        let wanted = self.store.server(server_id)?.network.as_deref()?.strip_prefix("tailscale:")?.trim().to_string();
        if wanted.is_empty() || self.network_ok.remove(&server_id) {
            return None;
        }
        let view = accounts::tailscale(&*self.runner);
        if !view.installed || accounts::is_active(&view, &wanted) {
            return None;
        }
        Some((wanted, accounts::active_name(&view).unwrap_or_else(|| "none".into())))
    }

    /// "Connect anyway" for the next open of this server.
    pub fn network_accept(&mut self, server_id: u64) {
        self.network_ok.insert(server_id);
    }

    // ---------------------------------------------------------------- proxies

    /// Adds (`id == 0`) or changes a saved proxy. Returns its id.
    pub fn proxy_save(&mut self, mut p: Proxy) -> Result<u64> {
        p.name = p.name.trim().to_string();
        p.host = p.host.trim().to_string();
        if p.name.is_empty() {
            bail!("a proxy needs a name");
        }
        if let Some(why) = p.problem() {
            bail!("{why}");
        }
        if p.profile.as_ref().is_some_and(|x| !self.store.profiles.iter().any(|y| &y.id == x)) {
            bail!("there is no such profile");
        }
        let id = if p.id == 0 {
            let id = self.store.proxies.iter().map(|x| x.id).max().unwrap_or(0) + 1;
            p.id = id;
            self.store.proxies.push(p);
            id
        } else {
            let slot = self.store.proxies.iter_mut().find(|x| x.id == p.id).context("this proxy no longer exists")?;
            let id = p.id;
            *slot = p;
            id
        };
        self.store.save()?;
        Ok(id)
    }

    /// Deletes a proxy; whatever used it goes direct.
    pub fn proxy_delete(&mut self, id: u64) -> Result<()> {
        self.store.proxies.retain(|p| p.id != id);
        for s in self.store.servers.iter_mut().filter(|s| s.proxy == Some(id)) {
            s.proxy = None;
        }
        for p in self.store.profiles.iter_mut().filter(|p| p.proxy == Some(id)) {
            p.proxy = None;
        }
        self.store.save()
    }

    /// The proxy local terminals of a profile go out through (`None`: none).
    pub fn profile_set_proxy(&mut self, profile: &str, proxy: Option<u64>) -> Result<()> {
        if proxy.is_some_and(|p| self.store.proxy(p).is_none()) {
            bail!("this proxy no longer exists");
        }
        let p = self.store.profiles.iter_mut().find(|p| p.id == profile).context("this profile no longer exists")?;
        p.proxy = proxy;
        self.store.save()
    }

    /// Puts a folder (and what is in it) in a profile, or in none.
    pub fn set_folder_profile(&mut self, id: u64, profile: Option<String>) -> Result<()> {
        if profile.as_ref().is_some_and(|p| !self.store.profiles.iter().any(|x| &x.id == p)) {
            bail!("there is no such profile");
        }
        if let Some(f) = self.store.folders.iter_mut().find(|f| f.id == id) {
            f.profile = profile;
        }
        self.store.save()
    }

    /// Adds a server (`id == 0`) or updates it. The secrets go to the vault, never to the store: `None` leaves
    /// a saved one alone, `Some("")` removes it.
    pub fn save_server(&mut self, mut s: Server, login: Option<&str>, sudo: Option<&str>) -> Result<u64> {
        if s.profile.as_ref().is_some_and(|p| !self.store.profiles.iter().any(|x| &x.id == p)) {
            bail!("there is no such profile");
        }
        if s.proxy.is_some_and(|p| self.store.proxy(p).is_none()) {
            bail!("this proxy no longer exists");
        }
        let touches_vault = login.is_some() || sudo.is_some();
        if touches_vault && !self.vault.is_unlocked() {
            bail!("unlock the vault to save a password");
        }
        if s.jump == Some(s.id) && s.id != 0 || (s.id != 0 && s.jump.is_some_and(|j| self.store.would_cycle(s.id, j))) {
            bail!("a jump host cannot lead back to this server");
        }
        let id = if s.id == 0 {
            s.has_secret = false;
            s.has_sudo = false;
            self.store.add_server(s)
        } else {
            let old = self.store.server(s.id).context("this server no longer exists")?;
            s.has_secret = old.has_secret;
            s.has_sudo = old.has_sudo;
            s.forwards = old.forwards.clone();
            let id = s.id;
            self.store.update_server(s);
            id
        };
        let name = self.host_label(id);
        if let Some(l) = login {
            let secret = (!l.is_empty()).then_some(l);
            self.vault.set(id, Kind::Login, secret, &name)?;
            self.set_flags(id, |s| s.has_secret = secret.is_some());
        }
        if let Some(p) = sudo {
            let secret = (!p.is_empty()).then_some(p);
            self.vault.set(id, Kind::Sudo, secret, &name)?;
            self.set_flags(id, |s| s.has_sudo = secret.is_some());
        }
        // A server that no longer logs in with a secret should not keep one.
        if self.store.server(id).is_some_and(|s| matches!(s.auth, Auth::Agent | Auth::VaultKey) && s.has_secret) && self.vault.is_unlocked() {
            self.vault.set(id, Kind::Login, None, &name)?;
            self.set_flags(id, |s| s.has_secret = false);
        }
        self.store.save()?;
        Ok(id)
    }

    fn set_flags(&mut self, id: u64, f: impl FnOnce(&mut Server)) {
        if let Some(mut s) = self.store.server(id).cloned() {
            f(&mut s);
            self.store.update_server(s);
        }
    }

    pub fn add_folder(&mut self, name: String, parent: Option<u64>) -> Result<u64> {
        let id = self.store.add_folder(name, parent);
        self.store.save()?;
        Ok(id)
    }

    pub fn rename_folder(&mut self, id: u64, name: String) -> Result<()> {
        if let Some(f) = self.store.folders.iter_mut().find(|f| f.id == id) {
            f.name = name;
        }
        self.store.save()
    }

    pub fn set_folder_open(&mut self, id: u64, open: bool) -> Result<()> {
        if let Some(f) = self.store.folders.iter_mut().find(|f| f.id == id) {
            f.expanded = open;
        }
        self.store.save()
    }

    /// Deletes a server or a folder with what is inside; the vault forgets their secrets when it is unlocked.
    pub fn delete_node(&mut self, node: NodeId) -> Result<Vec<u64>> {
        let gone = self.store.delete(node);
        if self.vault.is_unlocked() {
            for id in &gone {
                let _ = self.vault.remove_server(*id);
            }
        }
        self.store.save()?;
        Ok(gone)
    }

    /// Moves a server or folder into a folder (`None`: the top level).
    pub fn move_into(&mut self, node: NodeId, dest: Option<u64>) -> Result<bool> {
        let ok = self.store.move_into(node, dest);
        if ok {
            self.store.save()?;
        }
        Ok(ok)
    }

    // ---------------------------------------------------------------- tunnels

    /// Saves the port forwards of a server (the list as the window has it; new ones have `id == 0`).
    pub fn set_forwards(&mut self, server_id: u64, mut forwards: Vec<Forward>) -> Result<()> {
        let mut s = self.store.server(server_id).context("this server no longer exists")?.clone();
        let mut next = forwards.iter().map(|f| f.id).max().unwrap_or(0) + 1;
        for f in &mut forwards {
            if f.bind_port == 0 {
                bail!("a tunnel needs a port");
            }
            let host = f.host.trim();
            let bad_host = host.is_empty() || host.starts_with('-') || host.chars().any(|c| c.is_whitespace() || c.is_control() || c == ':');
            if f.kind != ForwardKind::Dynamic && (bad_host || f.port == 0) {
                bail!("a tunnel needs the name (or IPv4 address) and the port it leads to");
            }
            f.host = f.host.trim().to_string();
            f.name = f.name.trim().chars().take(60).collect();
            if f.id == 0 {
                f.id = next;
                next += 1;
            }
        }
        // a tunnel that was removed or changed is switched off
        // (the name and the automatic flag do not change what is running)
        let same_tunnel = |a: &Forward, b: &Forward| a.id == b.id && a.kind == b.kind && a.bind_port == b.bind_port && a.host == b.host && a.port == b.port;
        let gone: Vec<u64> = s.forwards.iter().filter(|old| !forwards.iter().any(|n| same_tunnel(n, old))).map(|f| f.id).collect();
        for id in gone {
            self.tunnel_stop(server_id, id);
        }
        s.forwards = forwards;
        self.store.update_server(s);
        self.store.save()
    }

    /// Switches a tunnel on. It stays on while oso is open; `tunnels()` says how it is doing.
    pub fn tunnel_start(&mut self, server_id: u64, forward_id: u64, use_vault: bool) -> std::result::Result<(), OpenError> {
        let server = self.store.server(server_id).ok_or(OpenError::NoSuchServer)?.clone();
        let fwd = server.forwards.iter().find(|f| f.id == forward_id).ok_or(OpenError::NoSuchServer)?.clone();
        if let Some(why) = connect::proxy_problem(&self.store, server_id) {
            return Err(OpenError::Other(why));
        }
        if use_vault && connect::needs_vault(&self.store, server_id) && !self.vault.is_unlocked() {
            return Err(OpenError::VaultLocked);
        }
        self.tunnel_stop(server_id, forward_id);
        let mut argv = connect::tunnel_argv(&self.store, &server, &fwd, &self.ssh_extra);
        argv.insert(1, "-v".into());
        if let Some(why) = connect::key_problem(&self.store, server_id) {
            return Err(OpenError::Other(why));
        }
        let mut opts = connect::opts_for(&self.store, &mut self.vault, server_id, argv, 24, 200, use_vault);
        self.agent_env(server_id, &mut opts)?;
        let tunnel = match Job::spawn(opts) {
            Ok(job) => Tunnel { job: Some(job), up: false, failed: None },
            Err(e) => Tunnel { job: None, up: false, failed: Some(format!("{e:#}")) },
        };
        self.tunnels.insert((server_id, forward_id), tunnel);
        Ok(())
    }

    pub fn tunnel_stop(&mut self, server_id: u64, forward_id: u64) {
        if let Some(mut t) = self.tunnels.remove(&(server_id, forward_id)) {
            if let Some(j) = t.job.as_mut() {
                j.kill();
            }
        }
    }

    /// How every tunnel that was switched on is doing (a failed one stays listed until it is switched off).
    pub fn tunnels(&mut self) -> Vec<TunnelStatus> {
        let mut out = vec![];
        for ((server_id, forward_id), t) in self.tunnels.iter_mut() {
            let kind = self.store.server(*server_id).and_then(|s| s.forwards.iter().find(|f| f.id == *forward_id)).map(|f| f.kind);
            if t.failed.is_none() {
                if let Some(job) = t.job.as_mut() {
                    let text = job.output();
                    if job.poll().is_some() {
                        t.failed = Some(jobs::problem(&text));
                    } else if let Some(why) = jobs::stuck_at(&text).filter(|_| job.age() > Duration::from_secs(4)) {
                        job.kill();
                        t.failed = Some(why.to_string());
                    } else if !t.up {
                        let marker = if kind == Some(ForwardKind::Remote) { "remote forward success" } else { "Local forwarding listening on" };
                        t.up = text.contains(marker);
                    }
                }
            }
            let (state, message) = match (&t.failed, t.up) {
                (Some(m), _) => ("failed", m.clone()),
                (None, true) => ("up", String::new()),
                (None, false) => ("starting", String::new()),
            };
            out.push(TunnelStatus { server_id: *server_id, forward_id: *forward_id, state, message });
        }
        out.sort_by_key(|t| (t.server_id, t.forward_id));
        out
    }

    // ---------------------------------------------------------------- files

    /// Copies files to a server (`upload`, `local` are paths here and `remote` the folder there, empty for the
    /// home folder) or one path from it (`local[0]` is the folder here). Returns an id for `transfers()`.
    pub fn transfer_start(&mut self, server_id: u64, upload: bool, local: Vec<String>, remote: &str, use_vault: bool) -> std::result::Result<u64, OpenError> {
        let server = self.store.server(server_id).ok_or(OpenError::NoSuchServer)?.clone();
        if let Some(why) = connect::proxy_problem(&self.store, server_id) {
            return Err(OpenError::Other(why));
        }
        if use_vault && connect::needs_vault(&self.store, server_id) && !self.vault.is_unlocked() {
            return Err(OpenError::VaultLocked);
        }
        let local = if !upload && local.is_empty() { vec![downloads_dir()] } else { local };
        if local.is_empty() || remote.contains('\n') || local.iter().any(|p| p.contains('\n') || p.is_empty()) {
            return Err(OpenError::Other("nothing to copy".into()));
        }
        let dir = if upload { connect::Direction::Up } else { connect::Direction::Down };
        let argv = connect::scp_argv(&self.store, &server, dir, &local, remote, &self.ssh_extra);
        if let Some(why) = connect::key_problem(&self.store, server_id) {
            return Err(OpenError::Other(why));
        }
        let mut opts = connect::opts_for(&self.store, &mut self.vault, server_id, argv, 24, 200, use_vault);
        self.agent_env(server_id, &mut opts)?;
        let job = Job::spawn(opts).map_err(|e| OpenError::Other(format!("{e:#}")))?;
        let name = |p: &str| std::path::Path::new(p).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| p.to_string());
        let what = if upload {
            if local.len() == 1 { name(&local[0]) } else { format!("{} files", local.len()) }
        } else {
            name(remote)
        };
        let id = self.next_transfer;
        self.next_transfer += 1;
        self.transfers.insert(id, Transfer { server_id, upload, what, job, end: None });
        Ok(id)
    }

    pub fn transfers(&mut self) -> Vec<TransferStatus> {
        let mut out = vec![];
        for (id, t) in self.transfers.iter_mut() {
            if t.end.is_none() {
                let text = t.job.output();
                if let Some(code) = t.job.poll() {
                    t.end = Some(if code == 0 { Ok(()) } else { Err(jobs::problem(&text)) });
                } else if let Some(why) = jobs::stuck_at(&text).filter(|_| t.job.age() > Duration::from_secs(4)) {
                    t.job.kill();
                    t.end = Some(Err(why.to_string()));
                }
            }
            let (state, message, percent) = match &t.end {
                Some(Ok(())) => ("done", String::new(), Some(100)),
                Some(Err(m)) => ("failed", m.clone(), None),
                None => ("running", String::new(), jobs::percent(&t.job.screen())),
            };
            out.push(TransferStatus { id: *id, server_id: t.server_id, upload: t.upload, what: t.what.clone(), state, percent, message });
        }
        out.sort_by_key(|t| t.id);
        out
    }

    /// Forgets a copy that is over (or stops one that is not).
    pub fn transfer_clear(&mut self, id: u64) {
        if let Some(mut t) = self.transfers.remove(&id) {
            t.job.kill();
        }
    }

    /// Puts a server through a jump host (`None`: direct).
    pub fn set_jump(&mut self, id: u64, via: Option<u64>) -> Result<bool> {
        let ok = self.store.move_via(id, via);
        if ok {
            self.store.save()?;
        }
        Ok(ok)
    }
}

/// A private key as it is kept in the vault: OpenSSH format, with no passphrase (the agent has to be able to sign).
fn check_ssh_key(text: &str) -> Result<String> {
    let key = ssh_key::PrivateKey::from_openssh(text.trim().as_bytes()).map_err(|_| anyhow!("that is not a private key in OpenSSH format (it starts with -----BEGIN OPENSSH PRIVATE KEY-----)"))?;
    if key.is_encrypted() {
        bail!("this key has a passphrase: remove it first (ssh-keygen -p -N \"\" -f <file>), or use the import, which asks for it once");
    }
    let mut out = key.to_openssh(ssh_key::LineEnding::LF).map_err(|e| anyhow!("could not read the key: {e}"))?.to_string();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

/// The window's engine is also the key source of oso's agent: the keys exist while the vault is unlocked.
/// The keys of one Space, for its own ssh-agent.
struct SpaceKeys {
    engine: std::sync::Weak<std::sync::Mutex<Engine>>,
    space: u64,
}

impl KeyProvider for SpaceKeys {
    fn identities(&self) -> Vec<(Vec<u8>, String)> {
        self.engine.upgrade().and_then(|e| e.lock().ok().map(|e| e.space_identities(self.space))).unwrap_or_default()
    }

    fn sign(&self, blob: &[u8], data: &[u8], flags: u32) -> Option<Vec<u8>> {
        self.engine.upgrade()?.lock().ok()?.space_sign(self.space, blob, data, flags)
    }
}

impl KeyProvider for std::sync::Mutex<Engine> {
    fn identities(&self) -> Vec<(Vec<u8>, String)> {
        self.lock().map(|e| e.agent_identities()).unwrap_or_default()
    }

    fn sign(&self, blob: &[u8], data: &[u8], flags: u32) -> Option<Vec<u8>> {
        self.lock().ok()?.agent_sign(blob, data, flags)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::SecretKind;
    use crate::daemon::Daemon;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("oso-engine-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    type Log = Arc<Mutex<Vec<(u64, Stream)>>>;

    fn engine(name: &str) -> (Engine, Log) {
        let dir = temp(name);
        let sock = dir.join("d.sock");
        let p = sock.clone();
        std::thread::spawn(move || {
            let _ = daemon::serve(p, false);
        });
        let end = Instant::now() + Duration::from_secs(3);
        while Daemon::connect_at(&sock).is_err() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(20));
        }
        let log: Log = Arc::default();
        let l = Arc::clone(&log);
        let emit: Emit = Arc::new(move |sid, ev| l.lock().unwrap().push((sid, ev)));
        let store = Store::load_from(dir.join("servers.json")).unwrap();
        let mut e = Engine::new(store, Vault::new(dir.join("vault.json")), emit);
        e.use_daemon(Daemon::connect_at(&sock).ok().unwrap());
        (e, log)
    }

    fn text_of(log: &Log, sid: u64) -> String {
        let mut bytes = vec![];
        for (s, ev) in log.lock().unwrap().iter() {
            if *s != sid {
                continue;
            }
            match ev {
                Stream::Snapshot { replay, screen, .. } => {
                    bytes.extend(replay);
                    bytes.extend(screen);
                }
                Stream::Output(b) => bytes.extend(b),
                _ => {}
            }
        }
        String::from_utf8_lossy(&bytes).to_string()
    }

    fn wait_for(log: &Log, sid: u64, want: &str) -> bool {
        let end = Instant::now() + Duration::from_secs(5);
        while Instant::now() < end {
            if text_of(log, sid).contains(want) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    fn server(name: &str) -> Server {
        Server {
            id: 0,
            name: name.into(),
            host: format!("{name}.example"),
            port: 22,
            user: "root".into(),
            auth: Auth::Password,
            key_path: String::new(),
            parent: None,
            jump: None,
            has_secret: false,
            has_sudo: false,
            has_totp: false,
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
    fn a_local_terminal_opens_takes_input_and_streams_its_output() {
        let (mut e, log) = engine("local");
        let s = e.open(LOCAL, 24, 80, true).unwrap();
        assert_eq!(s.title, "Local");
        e.write(s.sid, b"echo oso-$((6*7))\r");
        assert!(wait_for(&log, s.sid, "oso-42"), "{:?}", text_of(&log, s.sid));
        e.close(s.sid);
    }

    #[test]
    fn a_new_window_finds_the_sessions_where_they_were_and_with_their_meta() {
        let (mut e, log) = engine("restore");
        let s = e.open(LOCAL, 24, 80, true).unwrap();
        e.set_meta(s.sid, &serde_json::json!({ "title": "mine", "server_id": 0, "screen": 3, "order": 0 }));
        e.write(s.sid, b"echo still-here\r");
        assert!(wait_for(&log, s.sid, "still-here"));
        e.detach_all(); // the window closes
        let found = e.restore(24, 80).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title, "mine");
        assert_eq!(found[0].meta["screen"], 3);
        assert!(wait_for(&log, s.sid, "still-here"));
        e.close(s.sid);
    }

    #[test]
    fn opening_a_server_with_a_saved_password_needs_the_vault() {
        let (mut e, _log) = engine("vault");
        // no vault yet: saving a password must say so
        assert!(e.save_server(server("web"), Some("pw"), None).is_err());
        assert_eq!(e.vault_status(), VaultStatus { exists: false, unlocked: false });
        e.vault_unlock("master").unwrap();
        let id = e.save_server(server("web"), Some("hunter2"), Some("rootpw")).unwrap();
        let saved = e.store.server(id).unwrap();
        assert!(saved.has_secret && saved.has_sudo);
        // the secret is in neither the store file nor the tree sent to the window
        let file = std::fs::read_to_string(e.store_path()).unwrap();
        assert!(file.contains("web.example") && !file.contains("hunter2") && !file.contains("rootpw"));
        assert!(!serde_json::to_string(&e.tree()).unwrap().contains("hunter2"));
        e.vault_lock();
        assert_eq!(e.open(id, 24, 80, true).err(), Some(OpenError::VaultLocked));
        assert!(e.vault_unlock("wrong").is_err());
        e.vault_unlock("master").unwrap();
        assert_eq!(e.vault.get(id, Kind::Login).as_deref(), Some("hunter2"));
        assert_eq!(e.open(9999, 24, 80, true).err(), Some(OpenError::NoSuchServer));
    }

    #[test]
    fn the_vault_can_be_read_edited_and_changed_like_a_password_manager() {
        let (mut e, _log) = engine("vault-view");
        e.vault_unlock("master").unwrap();
        let id = e.save_server(server("web"), Some("hunter2"), None).unwrap();
        assert_eq!(e.vault_reveal(id, false).unwrap().as_deref(), Some("hunter2"));
        assert_eq!(e.vault_reveal(id, true).unwrap(), None);
        e.vault_set(id, true, Some("rootpw")).unwrap();
        assert_eq!(e.vault_reveal(id, true).unwrap().as_deref(), Some("rootpw"));
        assert!(e.store.server(id).unwrap().has_sudo);
        e.vault_set(id, false, None).unwrap();
        assert!(!e.store.server(id).unwrap().has_secret);
        assert_eq!(e.vault_reveal(id, false).unwrap(), None);
        e.vault_change_master("new-master").unwrap();
        e.vault_lock();
        assert!(e.vault_reveal(id, true).is_err(), "locked: nothing is shown");
        assert!(e.vault_unlock("master").is_err());
        e.vault_unlock("new-master").unwrap();
        assert_eq!(e.vault_reveal(id, true).unwrap().as_deref(), Some("rootpw"));
        assert!(e.vault_change_master("").is_err());
    }

    #[test]
    fn folders_servers_and_jump_hosts_are_kept_and_cannot_loop() {
        let (mut e, _log) = engine("tree");
        let f = e.add_folder("prod".into(), None).unwrap();
        let a = e.save_server(Server { parent: Some(f), auth: Auth::Agent, ..server("a") }, None, None).unwrap();
        let b = e.save_server(Server { auth: Auth::Agent, jump: Some(a), ..server("b") }, None, None).unwrap();
        assert!(e.set_jump(b, None).unwrap());
        assert!(e.set_jump(b, Some(a)).unwrap());
        let mut loopy = e.store.server(a).unwrap().clone();
        loopy.jump = Some(b);
        assert!(e.save_server(loopy, None, None).is_err(), "a -> b -> a is refused");
        let gone = e.delete_node(NodeId::Folder(f)).unwrap();
        assert!(gone.contains(&a));
        assert!(e.store.server(a).is_none());
        // reloading from disk finds what is left
        let again = Store::load_from(e.store_path()).unwrap();
        assert_eq!(again.servers.len(), 1);
    }

    #[test]
    fn an_old_vault_keeps_its_servers_flags_right_and_names_its_secrets() {
        let (mut e, _log) = engine("migrate");
        let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/vault-v1.json");
        std::fs::copy(fixture, e.store_path().parent().unwrap().join("vault.json")).unwrap();
        let ids: Vec<u64> = ["web", "db", "cache"].iter().map(|n| e.save_server(Server { auth: Auth::Password, ..server(n) }, None, None).unwrap()).collect();
        assert_eq!(ids, vec![1, 2, 3]);
        for (id, login, sudo) in [(1, true, true), (2, true, false), (3, false, true)] {
            e.set_flags(id, |s| {
                s.has_secret = login;
                s.has_sudo = sudo;
            });
        }
        e.vault_unlock("fixture-master").unwrap();
        for (id, login, sudo) in [(1, true, true), (2, true, false), (3, false, true)] {
            let s = e.store.server(id).unwrap();
            assert_eq!((s.has_secret, s.has_sudo), (login, sudo));
            assert_eq!(e.vault_reveal(id, false).unwrap().is_some(), login);
            assert_eq!(e.vault_reveal(id, true).unwrap().is_some(), sudo);
        }
        let names: Vec<String> = e.secret_list().unwrap().into_iter().map(|m| m.name).collect();
        assert!(names.contains(&"web (login)".to_string()) && names.contains(&"cache (sudo)".to_string()), "{names:?}");
    }

    #[test]
    fn secrets_have_names_and_types_and_their_list_has_no_values() {
        let (mut e, _log) = engine("secrets");
        assert!(e.secret_list().is_err(), "locked");
        e.vault_unlock("master").unwrap();
        let meta = |name: &str, kind| SecretMeta { id: 0, name: name.into(), kind, profile: None, env_var: None, agent_visible: false, notes: String::new(), updated: 0 };
        let key = e.secret_save(SecretMeta { env_var: Some("ANTHROPIC_API_KEY".into()), agent_visible: true, ..meta("anthropic", SecretKind::ApiKey) }, Some("TRAP-API-VALUE")).unwrap();
        assert!(e.secret_list().unwrap().iter().find(|m| m.id == key).unwrap().agent_visible);
        let tok = e.secret_save(meta("gh", SecretKind::Token), Some("TRAP-TOKEN-VALUE")).unwrap();
        assert!(!e.secret_list().unwrap().iter().find(|m| m.id == tok).unwrap().agent_visible);
        let id = e.save_server(server("web"), Some("TRAP-LOGIN"), Some("TRAP-SUDO")).unwrap();
        let json = serde_json::to_string(&e.secret_list().unwrap()).unwrap();
        assert!(!json.contains("TRAP-"), "{json}");
        assert_eq!(e.secret_reveal(key).unwrap(), "TRAP-API-VALUE");
        // deleting the secret a server logs in with takes the server's flag down
        let login = e.vault.links(id).login.unwrap();
        e.secret_delete(login).unwrap();
        let s = e.store.server(id).unwrap();
        assert!(!s.has_secret && s.has_sudo);
        assert!(e.secret_reveal(login).is_err());
    }

    #[test]
    fn this_computers_sudo_password_is_a_secret_of_server_zero() {
        let (mut e, _log) = engine("local-sudo");
        e.vault_unlock("master").unwrap();
        e.vault_set(LOCAL, true, Some("local-pw")).unwrap();
        assert_eq!(e.vault_reveal(LOCAL, true).unwrap().as_deref(), Some("local-pw"));
        assert!(e.secret_list().unwrap().iter().any(|m| m.name == "this computer (sudo)"));
    }

    fn names(tree: &Value, key: &str) -> Vec<String> {
        tree[key].as_array().unwrap().iter().map(|v| v["name"].as_str().unwrap().to_string()).collect()
    }

    #[test]
    fn the_tree_shows_the_active_profile_and_everything_when_asked() {
        let (mut e, _log) = engine("profiles");
        let work = e.profile_save("", "Work").unwrap();
        assert_eq!(work, "work");
        let home = e.profile_save("", "Home").unwrap();
        let folder = e.add_folder("clients".into(), None).unwrap();
        e.set_folder_profile(folder, Some(work.clone())).unwrap();
        e.save_server(Server { parent: Some(folder), auth: Auth::Agent, ..server("acme") }, None, None).unwrap();
        e.save_server(Server { profile: Some(home.clone()), auth: Auth::Agent, ..server("nas") }, None, None).unwrap();
        e.save_server(Server { auth: Auth::Agent, ..server("everywhere") }, None, None).unwrap();
        e.set_active_profile(&work).unwrap();
        let t = e.tree();
        assert_eq!(names(&t, "servers"), vec!["acme", "everywhere"]);
        assert_eq!(names(&t, "folders"), vec!["clients"]);
        assert_eq!(t["servers"][0]["profile_eff"], "work", "it says where it ended up");
        e.set_active_profile(&home).unwrap();
        let t = e.tree();
        assert_eq!(names(&t, "servers"), vec!["nas", "everywhere"]);
        assert!(names(&t, "folders").is_empty());
        // a hop of a shown server is listed even when it belongs to another profile
        let hop = e.store.servers.iter().find(|s| s.name == "nas").unwrap().id;
        let mut ev = e.store.servers.iter().find(|s| s.name == "everywhere").unwrap().clone();
        ev.jump = Some(hop);
        e.save_server(ev, None, None).unwrap();
        e.set_active_profile(&work).unwrap();
        assert_eq!(names(&e.tree(), "servers"), vec!["acme", "nas", "everywhere"]);
        e.set_active_profile(&home).unwrap();
        e.set_view_all(true);
        assert_eq!(names(&e.tree(), "servers").len(), 3);
        assert!(e.set_active_profile("nope").is_err());
        assert!(e.save_server(Server { profile: Some("nope".into()), ..server("x") }, None, None).is_err());
    }

    #[test]
    fn the_active_profile_is_remembered_next_to_the_other_files() {
        let (mut e, _log) = engine("profile-memory");
        let work = e.profile_save("", "Work").unwrap();
        e.set_active_profile(&work).unwrap();
        e.set_view_all(true);
        let dir = e.store_path().parent().unwrap().to_path_buf();
        let again = Engine::new(Store::load_from(e.store_path()).unwrap(), Vault::new(dir.join("vault.json")), Arc::new(|_, _| {}));
        assert_eq!(again.active_profile(), "work");
        assert!(again.tree()["view_all"].as_bool().unwrap());
    }

    #[test]
    fn deleting_a_profile_moves_what_was_in_it_to_the_default_one() {
        let (mut e, _log) = engine("profile-delete");
        e.vault_unlock("m").unwrap();
        let work = e.profile_save("", "Work").unwrap();
        let sid = e.save_server(Server { profile: Some(work.clone()), auth: Auth::Agent, ..server("a") }, None, None).unwrap();
        let key = e.secret_save(SecretMeta { id: 0, name: "k".into(), kind: SecretKind::ApiKey, profile: Some(work.clone()), env_var: None, agent_visible: false, notes: String::new(), updated: 0 }, Some("v")).unwrap();
        e.set_active_profile(&work).unwrap();
        assert!(e.profile_delete("default").is_err());
        e.vault_lock();
        assert!(e.profile_delete(&work).is_err(), "a locked vault could leave a secret in every profile");
        e.vault_unlock("m").unwrap();
        e.profile_delete(&work).unwrap();
        assert_eq!(e.active_profile(), "default");
        assert_eq!(e.store.server(sid).unwrap().profile.as_deref(), Some("default"));
        assert_eq!(e.vault.meta(key).unwrap().profile.as_deref(), Some("default"));
        assert!(e.profile_save("", "").is_err() && e.profile_save("", "Default").is_err());
    }

    fn api_key(name: &str, var: &str, profile: Option<&str>, visible: bool) -> SecretMeta {
        SecretMeta { id: 0, name: name.into(), kind: SecretKind::ApiKey, profile: profile.map(str::to_string), env_var: Some(var.into()), agent_visible: visible, notes: String::new(), updated: 0 }
    }

    #[test]
    fn a_local_terminal_gets_the_variables_of_its_profile_and_no_others() {
        let (mut e, log) = engine("local-env");
        e.vault_unlock("m").unwrap();
        let work = e.profile_save("", "Work").unwrap();
        e.secret_save(api_key("a", "SHARED_KEY", None, true), Some("shared-v")).unwrap();
        e.secret_save(api_key("b", "WORK_KEY", Some(&work), true), Some("work-v")).unwrap();
        e.secret_save(api_key("c", "DEFAULT_KEY", Some("default"), true), Some("default-v")).unwrap();
        e.secret_save(api_key("d", "HIDDEN_KEY", None, false), Some("hidden-v")).unwrap();
        e.set_active_profile(&work).unwrap();
        let s = e.open(LOCAL, 24, 120, true).unwrap();
        assert_eq!(s.title, "Local · Work");
        e.write(s.sid, b"echo [$OSO/$OSO_PROFILE] s=$SHARED_KEY w=$WORK_KEY d=$DEFAULT_KEY h=$HIDDEN_KEY .\r");
        assert!(wait_for(&log, s.sid, "[1/work] s=shared-v w=work-v d= h= ."), "{:?}", text_of(&log, s.sid));
        e.close(s.sid);
        // and the other way round
        e.set_active_profile("default").unwrap();
        let s = e.open(LOCAL, 24, 120, true).unwrap();
        assert_eq!(s.title, "Local");
        e.write(s.sid, b"echo [$OSO_PROFILE] w=$WORK_KEY d=$DEFAULT_KEY .\r");
        assert!(wait_for(&log, s.sid, "[default] w= d=default-v ."), "{:?}", text_of(&log, s.sid));
        e.close(s.sid);
    }

    #[test]
    fn a_terminal_of_a_space_gets_its_secrets_its_folder_and_its_name_and_no_others() {
        let (mut e, log) = engine("space-env");
        e.vault_unlock("m").unwrap();
        let inside = e.secret_save(api_key("in", "SPACE_KEY", None, true), Some("space-v")).unwrap();
        let hidden = e.secret_save(api_key("hid", "HIDDEN_KEY", None, false), Some("hidden-v")).unwrap();
        e.secret_save(api_key("out", "OTHER_KEY", None, true), Some("other-v")).unwrap();
        let dir = temp("space-cwd");
        let id = e.space_save(&Space { name: "Acme".into(), secrets: vec![inside, hidden], folders: vec![dir.to_string_lossy().into_owned()], ..Default::default() }).unwrap();
        assert!(e.space_ok(), "the server of this version keeps the Space of a session");
        let s = e.open_in(LOCAL, 24, 120, true, OpenCtx { space: Some(id), cwd: None }).unwrap();
        assert_eq!(s.title, "Local · Acme");
        assert_eq!(s.meta["space"], id, "the window learns which Space it belongs to");
        e.write(s.sid, b"echo [$OSO_SPACE] s=$SPACE_KEY o=$OTHER_KEY h=$HIDDEN_KEY p=$PWD .\r");
        let want = format!("[Acme] s=space-v o= h= p={} .", dir.display());
        assert!(wait_for(&log, s.sid, &want), "{:?}", text_of(&log, s.sid));
        e.close(s.sid);
        // another folder, asked for by the window, wins over the Space's first one
        let other = temp("space-cwd2");
        let s = e.open_in(LOCAL, 24, 120, true, OpenCtx { space: Some(id), cwd: Some(other.to_string_lossy().into_owned()) }).unwrap();
        e.write(s.sid, b"echo p=$PWD .\r");
        assert!(wait_for(&log, s.sid, &format!("p={} .", other.display())), "{:?}", text_of(&log, s.sid));
        e.close(s.sid);
        // a Space that is gone opens nothing
        e.space_delete(id).unwrap();
        assert!(matches!(e.open_in(LOCAL, 24, 120, true, OpenCtx { space: Some(id), cwd: None }), Err(OpenError::Other(m)) if m.contains("no longer exists")));
    }

    #[test]
    fn a_terminal_keeps_its_space_and_folder_in_its_meta_so_that_opening_it_again_puts_it_back() {
        let (mut e, _log) = engine("space-meta");
        let dir = temp("space-meta-dir");
        let id = e.space_save(&Space { name: "Acme".into(), ..Default::default() }).unwrap();
        let live = e.open_in(LOCAL, 24, 120, true, OpenCtx { space: Some(id), cwd: Some(dir.to_string_lossy().into_owned()) }).unwrap();
        let meta = e.meta_of(live.sid).unwrap();
        assert_eq!((meta["space"].as_u64(), meta["cwd"].as_str()), (Some(id), dir.to_str()));
        let plain = e.open(LOCAL, 24, 120, true).unwrap();
        assert!(e.meta_of(plain.sid).unwrap().get("space").is_none(), "a plain terminal belongs to no Space");
        e.close(live.sid);
        e.close(plain.sid);
    }

    #[test]
    fn an_agent_started_by_hand_in_a_terminal_is_found_and_its_state_follows_what_is_known_about_it() {
        let (mut e, log) = engine("agent-detect");
        let dir = temp("agent-detect-dir");
        let s = e.open_in(LOCAL, 24, 120, true, OpenCtx { space: None, cwd: Some(dir.to_string_lossy().into_owned()) }).unwrap();
        assert!(e.agent_status(&[]).is_empty(), "a shell is not an agent");
        e.write(s.sid, b"exec -a claude sleep 60\r");
        let end = std::time::Instant::now() + Duration::from_secs(5);
        let mut found = vec![];
        while found.is_empty() && std::time::Instant::now() < end {
            std::thread::sleep(Duration::from_millis(100));
            found = e.agent_status(&[]);
        }
        let a = found.first().unwrap_or_else(|| panic!("claude not found: {:?}", text_of(&log, s.sid)));
        assert_eq!((a.sid, a.kind.as_str(), a.state, a.source), (s.sid, "claude", crate::agents::State::Idle, "process"));
        assert_eq!(std::path::Path::new(&a.cwd).canonicalize().unwrap(), dir.canonicalize().unwrap(), "its folder");
        // what its own hooks say beats what the process says
        assert!(e.agent_event(s.sid, "claude", "UserPromptSubmit"));
        assert_eq!((e.agent_status(&[])[0].state, e.agent_status(&[])[0].source), (crate::agents::State::Working, "hook"));
        assert!(!e.agent_event(s.sid, "claude", "SomethingNew"), "an event oso does not know says nothing");
        // a request waiting beats the hooks
        assert_eq!(e.agent_status(&[s.sid])[0].state, crate::agents::State::NeedsYou);
        assert_eq!(e.agent_status(&[])[0].state, crate::agents::State::Working, "and goes back when answered");
        // when the program ends it is finished, until the person looks
        e.write(s.sid, &[3]);
        let end = std::time::Instant::now() + Duration::from_secs(5);
        while e.agent_status(&[]).first().is_some_and(|a| a.state != crate::agents::State::Finished) && std::time::Instant::now() < end {
            std::thread::sleep(Duration::from_millis(100));
        }
        assert_eq!(e.agent_status(&[])[0].state, crate::agents::State::Finished);
        e.agent_ack(s.sid);
        assert!(e.agent_status(&[]).is_empty());
        e.close(s.sid);
    }

    #[test]
    fn with_the_vault_locked_the_terminal_opens_without_secrets_and_says_so() {
        let (mut e, log) = engine("local-locked");
        e.vault_unlock("m").unwrap();
        e.secret_save(api_key("a", "SOME_KEY", None, true), Some("some-v")).unwrap();
        e.vault_lock();
        let s = e.open(LOCAL, 24, 120, true).unwrap();
        assert_eq!(s.title, "Local · vault locked");
        e.write(s.sid, b"echo [$OSO] k=$SOME_KEY .\r");
        assert!(wait_for(&log, s.sid, "[1] k= ."), "{:?}", text_of(&log, s.sid));
        e.close(s.sid);
    }

    #[test]
    fn a_local_terminal_has_a_token_that_is_not_shown_anywhere_else() {
        let (mut e, log) = engine("local-token");
        assert!(e.agent_ok(), "the server of this version knows tokens");
        let s = e.open(LOCAL, 24, 120, true).unwrap();
        e.write(s.sid, b"echo pane=$OSO_PANE tok=$OSO_TOKEN.\r");
        assert!(wait_for(&log, s.sid, &format!("pane={} tok=", s.sid)), "{:?}", text_of(&log, s.sid));
        std::thread::sleep(Duration::from_millis(300));
        let text = text_of(&log, s.sid);
        // the typed line shows `$OSO_TOKEN`; the answer shows 64 hex digits, then the dot
        let answered = text.split("tok=").skip(1).any(|t| t.len() > 64 && t[..64].chars().all(|c| c.is_ascii_hexdigit()) && t[64..].starts_with('.'));
        assert!(answered, "{text:?}");
        e.close(s.sid);
    }

    fn socks_proxy(name: &str) -> Proxy {
        Proxy { id: 0, name: name.into(), kind: crate::store::ProxyKind::Socks5, host: "proxy.corp".into(), port: 1080, profile: None }
    }

    #[test]
    fn proxies_are_saved_checked_and_forgotten_by_what_used_them() {
        let (mut e, _log) = engine("proxies");
        assert!(e.proxy_save(Proxy { host: "bad host".into(), ..socks_proxy("x") }).is_err());
        assert!(e.proxy_save(socks_proxy(" ")).is_err());
        let id = e.proxy_save(socks_proxy("corp")).unwrap();
        let again = Store::load_from(e.store_path()).unwrap();
        assert_eq!(again.proxies.len(), 1, "saved in proxies.json");
        let s = e.save_server(Server { proxy: Some(id), auth: Auth::Agent, ..server("a") }, None, None).unwrap();
        e.profile_set_proxy("default", Some(id)).unwrap();
        assert!(e.save_server(Server { proxy: Some(99), ..server("b") }, None, None).is_err());
        assert!(e.profile_set_proxy("default", Some(99)).is_err());
        e.proxy_delete(id).unwrap();
        assert_eq!(e.store.server(s).unwrap().proxy, None);
        assert_eq!(e.store.profiles[0].proxy, None);
    }

    #[test]
    fn a_server_behind_a_missing_helper_or_proxy_says_why_instead_of_connecting() {
        let (mut e, _log) = engine("proxy-open");
        let id = e.proxy_save(socks_proxy("corp")).unwrap();
        let s = e.save_server(Server { proxy: Some(id), auth: Auth::Agent, ..server("a") }, None, None).unwrap();
        e.store.proxies.clear(); // the proxy vanished behind our back
        let err = e.open(s, 24, 80, true).err();
        assert!(matches!(&err, Some(OpenError::Other(m)) if m.contains("no longer exists")), "{err:?}");
    }

    #[test]
    fn a_local_terminal_of_a_proxied_profile_uses_the_proxy_and_the_others_do_not() {
        let (mut e, log) = engine("local-proxy");
        let id = e.proxy_save(socks_proxy("corp")).unwrap();
        let work = e.profile_save("", "Work").unwrap();
        e.profile_set_proxy(&work, Some(id)).unwrap();
        e.set_active_profile(&work).unwrap();
        let s = e.open(LOCAL, 24, 120, true).unwrap();
        e.write(s.sid, b"echo p=$ALL_PROXY n=$no_proxy .\r");
        assert!(wait_for(&log, s.sid, "p=socks5h://proxy.corp:1080 n=localhost,127.0.0.1,::1 ."), "{:?}", text_of(&log, s.sid));
        e.close(s.sid);
        e.set_active_profile("default").unwrap();
        let s = e.open(LOCAL, 24, 120, true).unwrap();
        e.write(s.sid, b"echo [$HTTPS_PROXY] .\r");
        assert!(wait_for(&log, s.sid, "[] ."), "{:?}", text_of(&log, s.sid));
        e.close(s.sid);
    }

    #[test]
    fn renaming_a_tunnel_does_not_switch_it_off() {
        let (mut e, _log) = engine("tunnel-name");
        let id = e.save_server(Server { auth: Auth::Agent, ..server("a") }, None, None).unwrap();
        let f = Forward { id: 0, name: "  postgres ".into(), auto: false, kind: ForwardKind::Local, bind_port: 5433, host: "db".into(), port: 5432 };
        e.set_forwards(id, vec![f]).unwrap();
        let saved = e.store.server(id).unwrap().forwards[0].clone();
        assert_eq!(saved.name, "postgres");
        e.tunnels.insert((id, saved.id), Tunnel { job: None, up: true, failed: None });
        e.set_forwards(id, vec![Forward { name: "renamed".into(), auto: true, ..saved.clone() }]).unwrap();
        assert!(e.tunnels.contains_key(&(id, saved.id)), "only name and auto changed");
        e.set_forwards(id, vec![Forward { bind_port: 5434, ..saved }]).unwrap();
        assert!(e.tunnels.is_empty(), "a different port is a different tunnel");
    }

    use crate::accounts::testing::{Fake, TS_LIST, TS_STATUS};

    const GH_TWO: &str = r#"{"hosts":{"github.com":[{"state":"success","active":true,"host":"github.com","login":"me"},{"state":"success","active":false,"host":"github.com","login":"work-user"}]}}"#;

    /// An engine whose `gh` and `tailscale` are canned; returns the log of what was run.
    fn engine_with_tools(name: &str, answers: Vec<(&'static str, Option<(bool, &'static str, &'static str)>)>) -> (Engine, Log, Arc<Mutex<Vec<String>>>) {
        let (mut e, log) = engine(name);
        let calls: Arc<Mutex<Vec<String>>> = Arc::default();
        e.runner = Box::new(Fake { answers, calls: Arc::clone(&calls) });
        (e, log, calls)
    }

    #[test]
    fn switching_profile_switches_the_accounts_it_asks_for_and_only_those() {
        let (mut e, _log, calls) = engine_with_tools(
            "accounts",
            vec![("gh auth status", Some((true, GH_TWO, ""))), ("gh auth switch", Some((true, "", ""))), ("tailscale status", Some((true, TS_STATUS, ""))), ("tailscale switch --list", Some((true, TS_LIST, ""))), ("tailscale switch 9fe2", Some((true, "", "")))],
        );
        let work = e.profile_save("", "Work").unwrap();
        e.profile_set_accounts(&work, Some("work-user".into()), Some("9fe2".into())).unwrap();
        let problems = e.switch_profile(&work).unwrap();
        assert!(problems.is_empty(), "{problems:?}");
        let ran = calls.lock().unwrap().join("\n");
        assert!(ran.contains("gh auth switch --hostname github.com --user work-user"), "{ran}");
        assert!(ran.contains("tailscale switch 9fe2"), "{ran}");
        assert_eq!(e.active_profile(), "work");
        // a profile that asks for nothing touches nothing
        calls.lock().unwrap().clear();
        assert!(e.switch_profile("default").unwrap().is_empty());
        assert!(calls.lock().unwrap().is_empty(), "{:?}", calls.lock().unwrap());
        // an account that is already in use is left alone
        e.profile_set_accounts("default", Some("me".into()), Some("corp.example.com".into())).unwrap();
        calls.lock().unwrap().clear();
        e.switch_profile("default").unwrap();
        assert!(!calls.lock().unwrap().iter().any(|c| c.contains("switch 9") || c.contains("auth switch") || c.starts_with("tailscale switch ") && !c.contains("--list")), "{:?}", calls.lock().unwrap());
    }

    #[test]
    fn a_switch_that_fails_is_reported_and_the_other_still_happens() {
        let (mut e, _log, calls) = engine_with_tools(
            "accounts-fail",
            vec![("gh auth status", Some((true, GH_TWO, ""))), ("gh auth switch", Some((false, "", "no such user\n"))), ("tailscale status", Some((true, TS_STATUS, ""))), ("tailscale switch --list", Some((true, TS_LIST, ""))), ("tailscale switch 9fe2", Some((true, "", "")))],
        );
        let work = e.profile_save("", "Work").unwrap();
        e.profile_set_accounts(&work, Some("work-user".into()), Some("9fe2".into())).unwrap();
        let problems = e.switch_profile(&work).unwrap();
        assert_eq!(problems, vec!["GitHub: no such user".to_string()]);
        assert!(calls.lock().unwrap().iter().any(|c| c == "tailscale switch 9fe2"), "Tailscale was switched anyway");
        assert_eq!(e.active_profile(), "work", "the profile changed anyway");
    }

    #[test]
    fn a_tool_that_is_missing_is_a_note_not_a_failure() {
        let (mut e, _log, _calls) = engine_with_tools("accounts-missing", vec![("gh", None), ("tailscale", None)]);
        let work = e.profile_save("", "Work").unwrap();
        e.profile_set_accounts(&work, Some("me".into()), None).unwrap();
        let problems = e.switch_profile(&work).unwrap();
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("not installed"));
        let v = e.accounts();
        assert!(!v.github.installed && !v.tailscale.installed, "hidden in the window");
    }

    #[test]
    fn a_server_on_another_tailscale_account_asks_before_connecting() {
        let (mut e, _log, calls) = engine_with_tools("network", vec![("tailscale status", Some((true, TS_STATUS, ""))), ("tailscale switch --list", Some((true, TS_LIST, "")))]);
        let on_home = e.save_server(Server { network: Some("tailscale:home.ts.net".into()), auth: Auth::Agent, ..server("nas") }, None, None).unwrap();
        let on_corp = e.save_server(Server { network: Some("tailscale:corp.example.com".into()), auth: Auth::Agent, ..server("api") }, None, None).unwrap();
        let plain = e.save_server(Server { auth: Auth::Agent, ..server("plain") }, None, None).unwrap();
        calls.lock().unwrap().clear();
        assert_eq!(e.open(on_home, 24, 80, true).err(), Some(OpenError::Network { wanted: "home.ts.net".into(), current: "me@corp.example.com".into() }));
        assert!(!calls.lock().unwrap().iter().any(|c| c.starts_with("ssh")), "nothing was launched");
        // "connect anyway" lets exactly one open through
        e.network_accept(on_home);
        assert!(!matches!(e.open(on_home, 24, 80, true).err(), Some(OpenError::Network { .. })));
        assert!(matches!(e.open(on_home, 24, 80, true).err(), Some(OpenError::Network { .. })));
        // the right account, or no network declared: no question
        assert!(!matches!(e.open(on_corp, 24, 80, true).err(), Some(OpenError::Network { .. })));
        assert!(!matches!(e.open(plain, 24, 80, true).err(), Some(OpenError::Network { .. })));
    }

    #[test]
    fn a_local_terminal_gets_the_cloud_settings_of_its_profile() {
        let (mut e, log) = engine("cloud");
        let work = e.profile_save("", "Work").unwrap();
        e.profile_set_cloud(&work, Some("acme-prod".into()), Some("~/.kube/acme.yaml".into()), true).unwrap();
        assert!(e.profile_set_cloud(&work, Some("bad name".into()), None, false).is_err());
        assert!(e.store.profiles.iter().find(|p| p.id == work).unwrap().production, "a refused change changed nothing");
        e.set_active_profile(&work).unwrap();
        let s = e.open(LOCAL, 24, 160, true).unwrap();
        e.write(s.sid, b"echo aws=$AWS_PROFILE kube=$KUBECONFIG .\r");
        let home = directories::BaseDirs::new().unwrap().home_dir().to_string_lossy().into_owned();
        assert!(wait_for(&log, s.sid, &format!("aws=acme-prod kube={home}/.kube/acme.yaml .")), "{:?}", text_of(&log, s.sid));
        e.close(s.sid);
        e.set_active_profile("default").unwrap();
        let s = e.open(LOCAL, 24, 160, true).unwrap();
        e.write(s.sid, b"echo [$AWS_PROFILE] [$KUBECONFIG] .\r");
        assert!(wait_for(&log, s.sid, "[] [] ."), "another profile does not get them: {:?}", text_of(&log, s.sid));
        e.close(s.sid);
        // and they survive a restart
        let again = Store::load_from(e.store_path()).unwrap();
        assert_eq!(again.profiles.iter().find(|p| p.id == work).unwrap().aws_profile.as_deref(), Some("acme-prod"));
    }
}
