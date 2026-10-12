//! oso's command line (`oso unlock`, `oso run`…) talks to the running window through a private socket. The vault lives
//! in the window's process, so that is where the answers are; with no window there is nothing to talk to.
//!
//! What it never does: print a secret. `oso run` hands the secrets of a Space to a program the person names (as
//! environment variables, like any "run with secrets" tool); when that would give it more than a terminal of oso
//! already has, the window asks first.

use crate::approvals::{ApprovalRequest, Approvals, Decision};
use crate::engine::Engine;
use crate::store::{Auth, Server};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Where the window listens for the command line (next to the agent's socket, or in the config folder for a test profile).
pub fn socket_path() -> PathBuf {
    crate::agent::socket_path().with_file_name("control.sock")
}

pub struct Control {
    path: PathBuf,
    stop: Arc<AtomicBool>,
}

impl Control {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Control {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = std::fs::remove_file(&self.path);
    }
}

pub type Handler = Arc<dyn Fn(Value) -> Value + Send + Sync>;

/// Starts listening (mode 0600). Fails if another oso already answers there.
pub fn start(path: PathBuf, handler: Handler) -> Result<Control> {
    if let Some(dir) = path.parent() {
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    }
    if path.exists() {
        if UnixStream::connect(&path).is_ok() {
            bail!("another oso is already listening on {}", path.display());
        }
        let _ = std::fs::remove_file(&path);
    }
    let listener = UnixListener::bind(&path).with_context(|| format!("could not listen on {}", path.display()))?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    std::thread::spawn(move || {
        while !flag.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((conn, _)) => {
                    let handler = Arc::clone(&handler);
                    std::thread::spawn(move || {
                        let _ = conn.set_nonblocking(false);
                        let _ = conn.set_read_timeout(Some(Duration::from_secs(30)));
                        let mut line = String::new();
                        let Ok(read) = conn.try_clone() else { return };
                        // one request, one answer; a limit on what is read, so that a stray program cannot fill memory
                        if std::io::Read::take(BufReader::new(read), 1 << 20).read_line(&mut line).is_err() {
                            return;
                        }
                        let answer = match serde_json::from_str::<Value>(&line) {
                            Ok(req) => handler(req),
                            Err(_) => json!({ "ok": false, "error": "not JSON" }),
                        };
                        let mut w = conn;
                        let _ = writeln!(w, "{answer}");
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(40)),
                Err(_) => std::thread::sleep(Duration::from_millis(200)),
            }
        }
    });
    Ok(Control { path, stop })
}

/// The command line's side: one request, and the answer (waiting as long as the person takes to approve).
pub fn request(path: &Path, req: &Value) -> Result<Value> {
    let mut s = UnixStream::connect(path).map_err(|_| anyhow::anyhow!("oso is not open: start it, then try again"))?;
    s.set_read_timeout(Some(Duration::from_secs(900)))?;
    writeln!(s, "{req}")?;
    let mut line = String::new();
    BufReader::new(s).read_line(&mut line)?;
    serde_json::from_str(&line).context("oso did not answer")
}

fn fail(msg: impl Into<String>) -> Value {
    json!({ "ok": false, "error": msg.into() })
}

/// What the window does for each command. `ui(event, payload)` tells the window to show something (open a tab).
pub struct Api {
    pub engine: Arc<Mutex<Engine>>,
    pub approvals: Arc<Approvals>,
    pub ui: Arc<dyn Fn(&str, Value) + Send + Sync>,
}

impl Api {
    pub fn handle(&self, req: Value) -> Value {
        let cmd = req.get("cmd").and_then(Value::as_str).unwrap_or("");
        match cmd {
            "status" => {
                let e = self.engine.lock().unwrap_or_else(|x| x.into_inner());
                let v = e.vault_status();
                let space = e.active_space().and_then(|id| e.spaces.get(id)).map(|s| s.name.clone()).unwrap_or_else(|| "General".into());
                json!({ "ok": true, "vault_exists": v.exists, "vault_unlocked": v.unlocked, "space": space })
            }
            "unlock" => {
                let master = zeroize::Zeroizing::new(req.get("master").and_then(Value::as_str).unwrap_or("").to_string());
                if master.is_empty() {
                    return fail("no master password given");
                }
                let r = self.engine.lock().unwrap_or_else(|x| x.into_inner()).vault_unlock(&master);
                (self.ui)("refresh", Value::Null);
                match r {
                    Ok(()) => json!({ "ok": true }),
                    Err(m) => fail(m),
                }
            }
            "lock" => {
                self.engine.lock().unwrap_or_else(|x| x.into_inner()).vault_lock();
                (self.ui)("refresh", Value::Null);
                json!({ "ok": true })
            }
            "spaces" => {
                let e = self.engine.lock().unwrap_or_else(|x| x.into_inner());
                let active = e.active_space().and_then(|id| e.spaces.get(id)).map(|s| s.name.clone()).unwrap_or_else(|| "General".into());
                let mut all = vec![json!({ "name": "General" })];
                all.extend(e.spaces.list().iter().map(|s| json!({ "name": s.name })));
                json!({ "ok": true, "active": active, "spaces": all })
            }
            "space_use" => {
                let name = req.get("space").and_then(Value::as_str).unwrap_or("");
                let mut e = self.engine.lock().unwrap_or_else(|x| x.into_inner());
                let id = if name.eq_ignore_ascii_case("general") {
                    None
                } else {
                    match e.spaces.list().iter().find(|s| s.name.eq_ignore_ascii_case(name)) {
                        Some(s) => Some(s.id),
                        None => return fail(format!("there is no Space {name:?}")),
                    }
                };
                match e.set_active_space(id) {
                    Ok(()) => {
                        let changes = e.account_changes(id);
                        let shown = id.and_then(|i| e.spaces.get(i)).map(|s| s.name.clone()).unwrap_or_else(|| "General".into());
                        drop(e);
                        (self.ui)("refresh", Value::Null);
                        json!({ "ok": true, "space": shown, "changes": changes })
                    }
                    Err(x) => fail(format!("{x:#}")),
                }
            }
            "env_import" => {
                let dir = req.get("dir").and_then(Value::as_str).unwrap_or("");
                let choices: Vec<crate::envscan::EnvChoice> = serde_json::from_value(req.get("choices").cloned().unwrap_or(Value::Null)).unwrap_or_default();
                match self.engine.lock().unwrap_or_else(|x| x.into_inner()).env_import(dir, &choices) {
                    Ok(s) => {
                        (self.ui)("refresh", Value::Null);
                        json!({ "ok": true, "added": s.added, "skipped": s.skipped, "originals": s.originals })
                    }
                    Err(e) => fail(format!("{e:#}")),
                }
            }
            "env_delete" => {
                let dir = req.get("dir").and_then(Value::as_str).unwrap_or("");
                let paths: Vec<String> = req.get("paths").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
                match self.engine.lock().unwrap_or_else(|x| x.into_inner()).env_delete(dir, &paths) {
                    Ok(gone) => json!({ "ok": true, "deleted": gone }),
                    Err(e) => fail(format!("{e:#}")),
                }
            }
            "run_env" => self.run_env(&req),
            "host_add" => self.host_add(&req),
            "open" => {
                let name = req.get("host").and_then(Value::as_str).unwrap_or("").to_lowercase();
                let e = self.engine.lock().unwrap_or_else(|x| x.into_inner());
                let hits: Vec<&Server> = e.store.servers.iter().filter(|s| s.name.to_lowercase() == name).collect();
                let hits = if hits.is_empty() { e.store.servers.iter().filter(|s| s.host.to_lowercase() == name || s.name.to_lowercase().contains(&name)).collect() } else { hits };
                match hits.as_slice() {
                    [s] => {
                        let (id, n) = (s.id, s.name.clone());
                        drop(e);
                        (self.ui)("open", json!({ "server_id": id }));
                        json!({ "ok": true, "host": n })
                    }
                    [] => fail(format!("there is no host {name:?}")),
                    many => fail(format!("{name:?} matches {}: say which", many.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", "))),
                }
            }
            other => fail(format!("unknown command {other:?}")),
        }
    }

    /// The environment for `oso run`: asks first when it would hand over more than a terminal of oso already has.
    fn run_env(&self, req: &Value) -> Value {
        let command: Vec<String> = req.get("command").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
        if command.is_empty() {
            return fail("no command given");
        }
        let wanted = req.get("space").and_then(Value::as_str).map(str::to_string);
        let (env, names, extra, label_space) = {
            let mut e = self.engine.lock().unwrap_or_else(|x| x.into_inner());
            if !e.vault.is_unlocked() {
                return fail("the vault is locked: run `oso unlock` first");
            }
            let active = e.active_space();
            let space = match wanted.as_deref() {
                None => active,
                Some(w) if w.eq_ignore_ascii_case("general") => None,
                Some(w) => match e.spaces.list().iter().find(|s| s.name.eq_ignore_ascii_case(w)) {
                    Some(s) => Some(s.id),
                    None => return fail(format!("there is no Space {w:?}")),
                },
            };
            let mut env: Vec<(String, zeroize::Zeroizing<String>)> = vec![];
            let mut names = vec![];
            // asking for another Space than the one in use hands over more than a terminal of it already has
            let mut extra = space != active;
            let ids = e.secrets_in_reach(space);
            for (name, var, value, visible) in e.vault.env_with_flags(&ids) {
                extra |= !visible;
                names.push(format!("{name} ({var})"));
                env.push((var, value));
            }
            let plain = e.space_env(space);
            let label = space.and_then(|i| e.spaces.get(i)).map(|s| s.name.clone()).unwrap_or_else(|| "General".into());
            let mut all: Vec<(String, String)> = plain;
            all.extend(env.iter().map(|(k, v)| (k.clone(), v.to_string())));
            (all, names, extra, label)
        };
        let shown = command.join(" ");
        if extra && !names.is_empty() {
            let d = self.approvals.ask(
                ApprovalRequest {
                    id: 0,
                    pane: 0,
                    label: "A command you started with `oso run`".into(),
                    agent: "oso run".into(),
                    action: "run_with".into(),
                    host: "this computer".into(),
                    command: format!("{shown}    [with the secrets of {label_space}: {}]", names.join(", ")),
                    production: false,
                    can_remember: false,
                },
                Duration::from_secs(300),
            );
            if !matches!(d, Decision::Once | Decision::AlwaysRead) {
                return fail("not allowed in oso's window");
            }
        }
        json!({ "ok": true, "env": env, "names": names })
    }

    fn host_add(&self, req: &Value) -> Value {
        let text = |k: &str| req.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
        let (Some(name), Some(host)) = (text("name"), text("host")) else { return fail("a host needs a name and an address") };
        let auth = match text("auth").as_deref() {
            None | Some("agent") => Auth::Agent,
            Some("key") => Auth::Key,
            Some("password") => Auth::Password,
            Some(o) => return fail(format!("unknown authentication {o:?}: agent, key or password")),
        };
        let mut e = self.engine.lock().unwrap_or_else(|x| x.into_inner());
        if e.store.servers.iter().any(|s| s.name.eq_ignore_ascii_case(&name)) {
            return fail(format!("there is already a host called {name:?}"));
        }
        let space = match text("space") {
            None => None,
            Some(w) if w.eq_ignore_ascii_case("general") => None,
            Some(w) => match e.spaces.list().iter().find(|s| s.name.eq_ignore_ascii_case(&w)) {
                Some(s) => Some(s.id),
                None => return fail(format!("there is no Space {w:?}")),
            },
        };
        let s = Server {
            name,
            host,
            port: req.get("port").and_then(Value::as_u64).filter(|p| (1..=65535).contains(p)).unwrap_or(22) as u16,
            user: text("user").unwrap_or_default(),
            auth,
            key_path: text("key").unwrap_or_default(),
            notes: text("notes").unwrap_or_default(),
            ..Default::default()
        };
        match e.save_server(s, None, None) {
            Ok(id) => {
                if space.is_some() {
                    let _ = e.server_set_space(id, space);
                }
                drop(e);
                (self.ui)("refresh", Value::Null);
                json!({ "ok": true, "id": id })
            }
            Err(x) => fail(format!("{x:#}")),
        }
    }
}
