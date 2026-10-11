//! Everything the window asks for, in one place and with no window in it: the servers and folders, the vault,
//! and the sessions (kept alive by the background server). The Tauri commands are thin wrappers over this, so it
//! is tested without a screen.

use crate::connect::{self, LOCAL};
use crate::daemon::{self, Daemon, Sink, Stream};
use crate::session::SpawnOpts;
use crate::store::{Auth, NodeId, Server, Store};
use crate::vault::{Kind, UnlockError, Vault};
use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

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

pub struct Engine {
    pub store: Store,
    pub vault: Vault,
    daemon: Option<Daemon>,
    sessions: HashMap<u64, crate::daemon::RemoteSession>,
    emit: Emit,
}

impl Engine {
    pub fn new(store: Store, vault: Vault, emit: Emit) -> Self {
        Engine { store, vault, daemon: None, sessions: HashMap::new(), emit }
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
        })
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

    /// Saves (or, with `None`, removes) a secret of a server and keeps the server's flags in step.
    pub fn vault_set(&mut self, server_id: u64, sudo: bool, secret: Option<&str>) -> Result<()> {
        if !self.vault.is_unlocked() {
            bail!("the vault is locked");
        }
        if self.store.server(server_id).is_none() {
            bail!("this server no longer exists");
        }
        let secret = secret.filter(|s| !s.is_empty());
        self.vault.set(server_id, if sudo { Kind::Sudo } else { Kind::Login }, secret)?;
        self.set_flags(server_id, |s| if sudo { s.has_sudo = secret.is_some() } else { s.has_secret = secret.is_some() });
        self.store.save()
    }

    pub fn vault_change_master(&mut self, new_master: &str) -> Result<()> {
        if new_master.is_empty() {
            bail!("the master password cannot be empty");
        }
        self.vault.change_master(new_master)
    }

    // ---------------------------------------------------------------- servers

    /// The folders and servers, for the sidebar.
    pub fn tree(&self) -> Value {
        serde_json::json!({ "folders": self.store.folders, "servers": self.store.servers })
    }

    /// Adds a server (`id == 0`) or updates it. The secrets go to the vault, never to the store: `None` leaves
    /// a saved one alone, `Some("")` removes it.
    pub fn save_server(&mut self, mut s: Server, login: Option<&str>, sudo: Option<&str>) -> Result<u64> {
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
            let id = s.id;
            self.store.update_server(s);
            id
        };
        if let Some(l) = login {
            let secret = (!l.is_empty()).then_some(l);
            self.vault.set(id, Kind::Login, secret)?;
            self.set_flags(id, |s| s.has_secret = secret.is_some());
        }
        if let Some(p) = sudo {
            let secret = (!p.is_empty()).then_some(p);
            self.vault.set(id, Kind::Sudo, secret)?;
            self.set_flags(id, |s| s.has_sudo = secret.is_some());
        }
        // A server that no longer logs in with a secret should not keep one.
        if self.store.server(id).is_some_and(|s| s.auth == Auth::Agent && s.has_secret) && self.vault.is_unlocked() {
            self.vault.set(id, Kind::Login, None)?;
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
}
