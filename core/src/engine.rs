//! Everything the window asks for, in one place and with no window in it: the servers and folders, the vault,
//! and the sessions (kept alive by the background server). The Tauri commands are thin wrappers over this, so it
//! is tested without a screen.

use crate::connect::{self, LOCAL};
use crate::daemon::{self, Daemon, Sink, Stream};
use crate::jobs::{self, Job};
use crate::session::SpawnOpts;
use crate::store::{Auth, DEFAULT_PROFILE, Forward, ForwardKind, NodeId, Profile, Server, Store, slug};
use crate::vault::{Kind, SecretMeta, UnlockError, Vault};
use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

pub use crate::daemon::Stream as SessionEvent;

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
}

impl Engine {
    pub fn new(store: Store, vault: Vault, emit: Emit) -> Self {
        let settings = crate::settings::Settings::load_from(store.path().parent().unwrap_or(std::path::Path::new(".")));
        let active_profile = settings.active_profile.filter(|p| store.profiles.iter().any(|x| &x.id == p)).unwrap_or_else(|| DEFAULT_PROFILE.to_string());
        Engine { store, vault, daemon: None, sessions: HashMap::new(), emit, tunnels: HashMap::new(), transfers: HashMap::new(), next_transfer: 1, ssh_extra: vec![], active_profile, view_all: settings.view_all_profiles }
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
        if server_id != LOCAL && self.store.server(server_id).is_none() {
            return Err(OpenError::NoSuchServer);
        }
        if use_vault && server_id != LOCAL && connect::needs_vault(&self.store, server_id) && !self.vault.is_unlocked() {
            return Err(OpenError::VaultLocked);
        }
        let plan = connect::plan(&self.store, &mut self.vault, server_id, rows, cols, use_vault).ok_or(OpenError::NoSuchServer)?;
        let title = plan.title.clone();
        self.spawn(plan.opts, title, server_id).map_err(|e| OpenError::Other(format!("{e:#}")))
    }

    fn spawn(&mut self, opts: SpawnOpts, title: String, server_id: u64) -> Result<SessionInfo> {
        let make = self.sink_for();
        let d = self.daemon.as_ref().context("the background server is not running")?;
        let session = d.spawn_stream(opts, make)?;
        let sid = session.sid();
        let meta = serde_json::json!({ "title": title, "server_id": server_id });
        session.set_meta(&meta);
        self.sessions.insert(sid, session);
        Ok(SessionInfo { sid, title, server_id, meta })
    }

    /// Starts the same connection again in place of a session that ended (Enter on a dead terminal).
    pub fn reconnect(&mut self, sid: u64, rows: u16, cols: u16, use_vault: bool) -> std::result::Result<SessionInfo, OpenError> {
        let meta = self.sessions.get(&sid).map(|_| ()).and(self.meta_of(sid));
        let server_id = meta.as_ref().and_then(|m| m.get("server_id")).and_then(Value::as_u64).ok_or(OpenError::NoSuchServer)?;
        let mut info = self.open(server_id, rows, cols, use_vault)?;
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
        self.vault.relabel_hosts(|id| names.get(&id).cloned()).map_err(|e| format!("{e:#}"))
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
        self.vault.save_secret(meta, value)
    }

    /// Deletes a secret and keeps the flags of the servers that used it in step.
    pub fn secret_delete(&mut self, id: u64) -> Result<()> {
        let touched = self.vault.delete_secret(id)?;
        for server in touched {
            let links = self.vault.links(server);
            self.set_flags(server, |s| {
                s.has_secret = links.login.is_some();
                s.has_sudo = links.sudo.is_some();
            });
        }
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
                v
            })
            .collect();
        serde_json::json!({ "folders": folders, "servers": servers, "profiles": self.store.profiles, "active_profile": self.active_profile, "view_all": self.view_all })
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
            self.store.profiles.push(Profile { id: id.clone(), name: name.to_string() });
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
        if self.store.server(id).is_some_and(|s| s.auth == Auth::Agent && s.has_secret) && self.vault.is_unlocked() {
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
            if f.id == 0 {
                f.id = next;
                next += 1;
            }
        }
        // a tunnel that was removed or changed is switched off
        let gone: Vec<u64> = s.forwards.iter().filter(|old| !forwards.contains(old)).map(|f| f.id).collect();
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
        if use_vault && connect::needs_vault(&self.store, server_id) && !self.vault.is_unlocked() {
            return Err(OpenError::VaultLocked);
        }
        self.tunnel_stop(server_id, forward_id);
        let mut argv = connect::tunnel_argv(&self.store, &server, &fwd, &self.ssh_extra);
        argv.insert(1, "-v".into());
        let opts = connect::opts_for(&self.store, &mut self.vault, server_id, argv, 24, 200, use_vault);
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
        if use_vault && connect::needs_vault(&self.store, server_id) && !self.vault.is_unlocked() {
            return Err(OpenError::VaultLocked);
        }
        let local = if !upload && local.is_empty() { vec![downloads_dir()] } else { local };
        if local.is_empty() || remote.contains('\n') || local.iter().any(|p| p.contains('\n') || p.is_empty()) {
            return Err(OpenError::Other("nothing to copy".into()));
        }
        let dir = if upload { connect::Direction::Up } else { connect::Direction::Down };
        let argv = connect::scp_argv(&self.store, &server, dir, &local, remote, &self.ssh_extra);
        let opts = connect::opts_for(&self.store, &mut self.vault, server_id, argv, 24, 200, use_vault);
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
            forwards: vec![],
            profile: None,
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
}
