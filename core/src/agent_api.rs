//! What an AI agent can ask of oso, and how each request is answered: who may do what (`policy`), the person's
//! say (`approvals`), the record (`audit`), and the running of the commands. The agent's side is `mcp`.
//!
//! Rules that hold for every tool:
//! * no tool ever returns a secret: what comes out of a program is blanked of every secret the vault holds;
//! * a request comes from inside an oso terminal (the background server checked its token) and names which one;
//! * anything that is not a plain read asks the person, naming the pane, the host and the exact command.

use crate::approvals::{ApprovalRequest, Approvals, Decision};
use crate::audit::{Audit, Entry};
use crate::connect::{self, LOCAL};
use crate::daemon::AgentCall;
use crate::engine::Engine;
use crate::jobs::Job;
use crate::policy::{self, AiPolicy, Verdict};
use crate::session::SpawnOpts;
use crate::spaces::Space;
use crate::store::{EnvKind, Server};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

/// How much of a program's output an agent gets (each of stdout and stderr).
const MAX_OUTPUT: usize = 64 * 1024;
/// How long the person has to answer.
const APPROVAL_WAIT: Duration = Duration::from_secs(300);
const DEFAULT_TIMEOUT: u64 = 60;
const MAX_TIMEOUT: u64 = 300;
/// Past this many servers `oso_context` gives names only and says to search.
const MANY_SERVERS: usize = 25;

pub struct Handler {
    engine: Arc<Mutex<Engine>>,
    approvals: Arc<Approvals>,
    audit: Audit,
    /// The program that runs a command as root (the tests give another one).
    pub sudo_program: String,
    /// Numbers the requests (the time it started, in ms, then one more each), so that the lines of one request in
    /// the audit log can be put together.
    next_id: AtomicU64,
}

/// Who is asking, from where.
struct Ctx {
    id: u64,
    pane: u64,
    /// The Space of the terminal the agent runs in, as the background server stored it.
    space: Option<u64>,
    title: String,
    agent: String,
}

impl Ctx {
    fn label(&self) -> String {
        let who = match self.agent.split(|c: char| !c.is_alphanumeric()).next().unwrap_or("").to_lowercase().as_str() {
            "claude" => "Claude".to_string(),
            "" => "An AI agent".to_string(),
            other => {
                let mut c = other.chars();
                c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
            }
        };
        format!("{who} in {}, pane {}", self.title, self.pane)
    }
}

fn ok(v: Value) -> Value {
    json!({ "ok": true, "result": v })
}

fn no(msg: impl Into<String>) -> Value {
    json!({ "ok": false, "error": msg.into() })
}

fn policy_word(p: AiPolicy) -> &'static str {
    match p {
        AiPolicy::ReadOnlyAuto => "read_only_auto",
        AiPolicy::Ask => "ask",
        AiPolicy::Blocked => "blocked",
    }
}

/// Why a request had to ask, as the start of the sentence that says how it ended.
fn why_asked(t: &Target) -> &'static str {
    if t.env == Some(EnvKind::Production) {
        "Production always asks, whatever the server's policy says"
    } else if t.policy == AiPolicy::ReadOnlyAuto {
        "The server lets only plain reads through, and this is not one"
    } else {
        "The server's policy is \u{201c}ask\u{201d}"
    }
}

fn env_word(e: Option<EnvKind>) -> &'static str {
    match e {
        Some(EnvKind::Production) => "production",
        Some(EnvKind::Staging) => "staging",
        Some(EnvKind::Dev) => "dev",
        None => "unspecified",
    }
}

/// Blanks every secret of the vault out of `text`.
pub fn redact(text: &str, hide: &[Zeroizing<String>]) -> String {
    let mut out = text.to_string();
    // longest first, so that a value inside another one does not leave the rest showing
    let mut order: Vec<&Zeroizing<String>> = hide.iter().collect();
    order.sort_by_key(|v| std::cmp::Reverse(v.len()));
    for v in order {
        if !v.is_empty() && out.contains(v.as_str()) {
            out = out.replace(v.as_str(), "[hidden by oso]");
        }
    }
    out
}

/// Removes the colours and cursor moves a terminal adds.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('[') => {
                for n in it.by_ref() {
                    if ('@'..='~').contains(&n) {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(n) = it.next() {
                    if n == '\u{7}' || (n == '\u{1b}' && it.peek() == Some(&'\\')) {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn cap(mut s: String) -> (String, bool) {
    if s.len() <= MAX_OUTPUT {
        return (s, false);
    }
    let mut cut = MAX_OUTPUT;
    while !s.is_char_boundary(cut) {
        cut -= 1;
    }
    s.truncate(cut);
    (s, true)
}

/// What a finished program left.
struct Captured {
    stdout: String,
    stderr: String,
    code: Option<i32>,
    timed_out: bool,
}

/// Runs `cmd` with the pipes read as it goes (a program that prints a lot must not fill them and stop), up to
/// `timeout`. Whatever is past the cap is read and dropped.
fn run_capture(mut cmd: Command, stdin: Option<Vec<u8>>, timeout: Duration) -> std::io::Result<Captured> {
    cmd.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    if let (Some(data), Some(mut si)) = (stdin, child.stdin.take()) {
        let _ = si.write_all(&data);
        // closing it tells the program there is no more
    }
    let read = |mut r: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut kept = Vec::new();
            let mut buf = [0u8; 8192];
            while let Ok(n) = r.read(&mut buf) {
                if n == 0 {
                    break;
                }
                if kept.len() < MAX_OUTPUT + 4096 {
                    kept.extend_from_slice(&buf[..n]);
                    kept.truncate(MAX_OUTPUT + 4096);
                }
            }
            String::from_utf8_lossy(&kept).into_owned()
        })
    };
    let out = read(Box::new(child.stdout.take().expect("piped")));
    let err = read(Box::new(child.stderr.take().expect("piped")));
    let end = Instant::now() + timeout;
    let (code, timed_out) = loop {
        match child.try_wait()? {
            Some(st) => break (st.code(), false),
            None if Instant::now() >= end => {
                let _ = child.kill();
                let _ = child.wait();
                break (None, true);
            }
            None => std::thread::sleep(Duration::from_millis(25)),
        }
    };
    Ok(Captured { stdout: out.join().unwrap_or_default(), stderr: err.join().unwrap_or_default(), code, timed_out })
}

/// What an agent may see and use: everything the active profile shows, or, when its terminal was opened in a Space, only
/// what that Space holds. The terminal's Space comes from the background server (it is not something the agent says).
enum Scope {
    Profile(String),
    Space(Box<Space>),
}

impl Scope {
    fn of(e: &Engine, ctx: &Ctx) -> Result<Scope, String> {
        match ctx.space {
            Some(id) => e.spaces.get(id).cloned().map(|s| Scope::Space(Box::new(s))).ok_or_else(|| "This terminal's Space no longer exists, so oso cannot tell what you may use: ask the user to open a new terminal in a Space.".to_string()),
            None => Ok(Scope::Profile(e.active_profile().to_string())),
        }
    }

    /// The servers in scope.
    fn servers(&self, e: &Engine) -> Vec<Server> {
        match self {
            Scope::Profile(p) => e.store.servers.iter().filter(|s| e.store.server_in(s, p)).cloned().collect(),
            Scope::Space(sp) => {
                let ids = sp.server_ids(&e.store);
                e.store.servers.iter().filter(|s| ids.contains(&s.id)).cloned().collect()
            }
        }
    }

    fn has_server(&self, e: &Engine, id: u64) -> bool {
        match self {
            Scope::Profile(p) => e.store.server(id).is_some_and(|s| e.store.server_in(s, p)),
            Scope::Space(sp) => sp.covers_server(&e.store, id),
        }
    }

    /// Is this secret one the agent may know about? A profile's secrets are the ones of that profile (or of none); a Space's are the ones it lists.
    fn has_secret(&self, id: u64, profile_of_secret: Option<&str>) -> bool {
        match self {
            Scope::Profile(p) => profile_of_secret.is_none_or(|sp| sp == p),
            Scope::Space(sp) => sp.has_secret(id),
        }
    }

    /// How to say "outside": where a name that is not found is looked for.
    fn place(&self) -> &'static str {
        match self {
            Scope::Profile(_) => "profile",
            Scope::Space(_) => "Space",
        }
    }
}

/// What an `oso_run` needs, worked out while the engine was locked.
struct Target {
    id: u64,
    name: String,
    policy: AiPolicy,
    env: Option<EnvKind>,
}

impl Handler {
    pub fn new(engine: Arc<Mutex<Engine>>, approvals: Arc<Approvals>) -> Handler {
        let path = engine.lock().map(|e| e.store_path().with_file_name("audit.jsonl")).unwrap_or_default();
        let start = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(1).max(1);
        Handler { engine, approvals, audit: Audit::new(path), sudo_program: "sudo".into(), next_id: AtomicU64::new(start) }
    }

    pub fn audit_path(&self) -> &std::path::Path {
        self.audit.path()
    }

    /// Answers one request. Never panics on a bad one: the answer says what was wrong.
    pub fn handle(&self, call: AgentCall) -> Value {
        let agent = call.body.get("agent").and_then(Value::as_str).unwrap_or("").chars().take(60).collect();
        let ctx = Ctx { id: self.next_id.fetch_add(1, Ordering::Relaxed), pane: call.sid, space: call.space, title: call.title, agent };
        let tool = call.body.get("tool").and_then(Value::as_str).unwrap_or("");
        let args = call.body.get("args").cloned().unwrap_or(Value::Null);
        match tool {
            "oso_context" => self.context(&ctx),
            "oso_hosts" => self.hosts(&ctx, &args),
            "oso_run" => self.run(&ctx, &args),
            "oso_sudo" => self.sudo(&ctx, &args),
            "oso_run_with" => self.run_with(&ctx, &args),
            "oso_tunnel" => self.tunnel(&ctx, &args),
            "oso_focus" => self.focus(&ctx),
            // not an MCP tool: what an agent's own hooks report (`oso hook`), for the Agents panel
            "oso_event" => {
                let (agent, event) = (args.get("agent").and_then(Value::as_str).unwrap_or(""), args.get("event").and_then(Value::as_str).unwrap_or(""));
                ok(json!({ "known": self.engine().agent_event(ctx.pane, agent, event) }))
            }
            other => no(format!("oso has no tool called {other:?}")),
        }
    }

    fn engine(&self) -> std::sync::MutexGuard<'_, Engine> {
        self.engine.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn log(&self, ctx: &Ctx, action: &str, host: &str, command: &str, decision: &str, extra: impl FnOnce(&mut Entry)) {
        let mut e = Entry { id: ctx.id, pane: ctx.pane, agent: ctx.agent.clone(), action: action.into(), host: host.into(), command: command.chars().take(8000).collect(), decision: decision.into(), ..Default::default() };
        extra(&mut e);
        // the log is shown in the window: whatever the agent put in the command or got back is blanked of the vault's secrets
        let hide = self.engine().vault.values_to_hide();
        for field in [&mut e.command, &mut e.note, &mut e.preview, &mut e.reason, &mut e.output] {
            *field = redact(field, &hide);
        }
        self.audit.log(e);
    }

    /// `log` for a request that had a server policy applied to it.
    #[allow(clippy::too_many_arguments)]
    fn log_why(&self, ctx: &Ctx, action: &str, t: &Target, command: &str, decision: &str, reason: &str, extra: impl FnOnce(&mut Entry)) {
        self.log(ctx, action, &t.name, command, decision, |e| {
            e.policy = policy_word(t.policy).into();
            e.reason = reason.into();
            extra(e);
        });
    }

    // ------------------------------------------------------------ reading

    fn context(&self, ctx: &Ctx) -> Value {
        let mut e = self.engine();
        let scope = match Scope::of(&e, ctx) {
            Ok(s) => s,
            Err(m) => return no(m),
        };
        let profile = match &scope {
            Scope::Profile(p) => p.clone(),
            Scope::Space(sp) => sp.profile.clone().filter(|p| e.profiles().iter().any(|x| &x.id == p)).unwrap_or_else(|| e.active_profile().to_string()),
        };
        let pname = e.profiles().iter().find(|p| p.id == profile).map(|p| p.name.clone()).unwrap_or_else(|| profile.clone());
        let servers: Vec<Server> = scope.servers(&e);
        let status = e.tunnels();
        let list: Vec<Value> = if servers.len() > MANY_SERVERS {
            servers.iter().map(|s| json!({ "name": s.name, "environment": env_word(s.env_kind) })).collect()
        } else {
            servers
                .iter()
                .map(|s| {
                    let tunnels: Vec<Value> = s.forwards.iter().map(|f| json!({ "name": if f.name.is_empty() { format!("{}", f.bind_port) } else { f.name.clone() }, "state": status.iter().find(|t| t.server_id == s.id && t.forward_id == f.id).map(|t| t.state).unwrap_or("off") })).collect();
                    json!({
                        "name": s.name, "host": s.host, "user": s.user, "port": s.port,
                        "environment": env_word(s.env_kind), "notes": s.notes,
                        // a hop outside the Space is not named: the agent has no business knowing it exists
                        "reached_through": s.jump.filter(|j| scope.has_server(&e, *j)).and_then(|j| e.store.server(j)).map(|j| j.name.clone()),
                        "tunnels": tunnels,
                    })
                })
                .collect()
        };
        let secrets = e.vault.list();
        let in_scope = |m: &crate::vault::SecretMeta| scope.has_secret(m.id, m.profile.as_deref());
        let vars: Vec<Value> = secrets.iter().filter(|m| m.agent_visible && m.env_var.is_some() && in_scope(m)).filter_map(|m| m.env_var.clone()).map(Value::from).collect();
        let more: Vec<Value> = secrets.iter().filter(|m| !m.agent_visible && m.env_var.is_some() && in_scope(m)).map(|m| Value::from(m.name.clone())).collect();
        let many = servers.len() > MANY_SERVERS;
        let space = match &scope {
            Scope::Space(sp) => json!({ "name": sp.name, "notes": sp.notes, "folders": sp.folders }),
            Scope::Profile(_) => Value::Null,
        };
        let focus = self.focus_view(&e, &scope);
        let out = ok(json!({
            "profile": { "id": profile, "name": pname },
            "space": space,
            "terminal": ctx.title,
            "servers": list,
            "servers_note": if many { "There are many servers: this is only their names. Use oso_hosts with a search to see one." } else { "" },
            "environment_variables_already_set_in_your_terminal": vars,
            "secrets_you_can_use_with_oso_run_with": more,
            "focus": focus,
            "rules": "Never ask the user for a password or a key: use oso_run (a command on a server), oso_sudo (sudo on this computer) or oso_run_with (a local command that needs a secret). The user is asked to approve what is not a plain read. Production servers always ask. When the user says \"this server\", call oso_focus.",
        }));
        drop(e);
        self.log(ctx, "context", "", "", "allowed_without_asking", |_| {});
        out
    }

    /// Which server the person has selected and which ssh sessions they have open: names only, and only servers in scope.
    fn focus_view(&self, e: &Engine, scope: &Scope) -> Value {
        let f = e.focus();
        let name = |id: Option<u64>| id.filter(|i| scope.has_server(e, *i)).and_then(|i| e.store.server(i)).map(|s| s.name.clone());
        let mut open: Vec<String> = f.open.iter().filter(|i| scope.has_server(e, **i)).filter_map(|i| e.store.server(*i)).map(|s| s.name.clone()).collect();
        open.sort();
        open.dedup();
        json!({ "selected_server": name(f.selected), "server_of_the_terminal_in_front": name(f.active), "open_ssh_sessions": open })
    }

    fn focus(&self, ctx: &Ctx) -> Value {
        let e = self.engine();
        let scope = match Scope::of(&e, ctx) {
            Ok(s) => s,
            Err(m) => return no(m),
        };
        let mut v = self.focus_view(&e, &scope);
        let profile = match &scope {
            Scope::Profile(p) => p.clone(),
            Scope::Space(sp) => sp.profile.clone().unwrap_or_else(|| e.active_profile().to_string()),
        };
        v["profile"] = json!(e.profiles().iter().find(|p| p.id == profile).map(|p| p.name.clone()).unwrap_or(profile));
        v["space"] = match &scope {
            Scope::Space(sp) => json!(sp.name),
            Scope::Profile(_) => Value::Null,
        };
        drop(e);
        self.log(ctx, "focus", "", "", "allowed_without_asking", |_| {});
        ok(v)
    }

    fn hosts(&self, ctx: &Ctx, args: &Value) -> Value {
        let q = args.get("query").and_then(Value::as_str).unwrap_or("").to_lowercase();
        let e = self.engine();
        let scope = match Scope::of(&e, ctx) {
            Ok(s) => s,
            Err(m) => return no(m),
        };
        let found: Vec<Value> = scope
            .servers(&e)
            .iter()
            .filter(|s| q.is_empty() || format!("{} {} {} {}", s.name, s.host, s.user, s.notes).to_lowercase().contains(&q))
            .take(30)
            .map(|s| json!({ "name": s.name, "host": s.host, "user": s.user, "port": s.port, "environment": env_word(s.env_kind), "notes": s.notes }))
            .collect();
        drop(e);
        self.log(ctx, "hosts", "", &q, "allowed_without_asking", |_| {});
        ok(json!({ "hosts": found, "shown_at_most": 30 }))
    }

    // ------------------------------------------------------------ asking

    fn ask(&self, ctx: &Ctx, action: &str, host: &str, command: &str, production: bool, can_remember: bool) -> Decision {
        self.approvals.ask(
            ApprovalRequest { id: 0, pane: ctx.pane, label: ctx.label(), agent: ctx.agent.clone(), action: action.into(), host: host.into(), command: command.into(), production, can_remember },
            APPROVAL_WAIT,
        )
    }

    // ------------------------------------------------------------ a command on a server

    fn target(&self, ctx: &Ctx, query: &str) -> Result<Target, String> {
        let mut e = self.engine();
        let scope = Scope::of(&e, ctx)?;
        let q = query.trim().to_lowercase();
        let in_scope = scope.servers(&e);
        let visible: Vec<&Server> = in_scope.iter().collect();
        let mut hits: Vec<&Server> = visible.iter().copied().filter(|s| s.name.to_lowercase() == q).collect();
        if hits.is_empty() {
            hits = visible.iter().copied().filter(|s| s.host.to_lowercase() == q).collect();
        }
        if hits.is_empty() {
            hits = visible.iter().copied().filter(|s| s.name.to_lowercase().contains(&q)).collect();
        }
        let s = match hits.as_slice() {
            [one] => (*one).clone(),
            [] => return Err(format!("There is no server called {query:?} in this {}. Use oso_hosts to search.", scope.place())),
            many => return Err(format!("{query:?} matches {}: say which one.", many.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", "))),
        };
        if let Some(why) = connect::proxy_problem(&e.store, s.id).or_else(|| connect::key_problem(&e.store, s.id)) {
            return Err(why);
        }
        if connect::needs_vault(&e.store, s.id) && !e.vault.is_unlocked() {
            return Err("oso's vault is locked: ask the person to unlock it (the lock at the bottom left of oso), then try again.".into());
        }
        if let Some((wanted, current)) = e.network_mismatch(s.id) {
            return Err(format!("{} is on the Tailscale account {wanted}, and {current} is in use. Ask the person to switch it in oso (Accounts).", s.name));
        }
        Ok(Target { id: s.id, name: s.name.clone(), policy: s.ai_policy, env: s.env_kind })
    }

    /// What to tell an agent whose command uses `sudo` on a server: oso runs it with no terminal, so a sudo that asks
    /// for a password cannot be answered, and the agent must not be left guessing why the sudo part came out empty.
    fn sudo_note(&self, server_id: u64, name: &str) -> Option<String> {
        let mut e = self.engine();
        if !e.vault.is_unlocked() {
            return Some(format!("This command uses sudo. oso's vault is locked, so it cannot tell whether a sudo password is saved for {name}: ask the user to unlock it in oso."));
        }
        if e.vault.get(server_id, crate::vault::Kind::Sudo).is_none() {
            return Some(format!("This command uses sudo, and there is no sudo password saved for {name}. Without one, sudo only works if the server lets this user run it without a password; if the sudo part failed or printed nothing, that is why. Ask the user to add the sudo password for {name} in oso (edit the server), or to run it themselves in a terminal. Do not ask them to type the password to you."));
        }
        Some(format!("This command uses sudo. oso does not type a sudo password into a command run this way: it works only if {name} lets this user run sudo without a password. If it failed for that reason, ask the user to run it in a terminal of oso, where oso types the saved password."))
    }

    fn run(&self, ctx: &Ctx, args: &Value) -> Value {
        let (Some(host), Some(command)) = (args.get("host").and_then(Value::as_str), args.get("command").and_then(Value::as_str)) else {
            return no("oso_run needs `host` and `command`.");
        };
        if command.trim().is_empty() || command.len() > 8000 || command.contains('\0') {
            return no("The command is empty, too long, or has a NUL in it.");
        }
        let timeout = Duration::from_secs(args.get("timeout_seconds").and_then(Value::as_u64).unwrap_or(DEFAULT_TIMEOUT).clamp(1, MAX_TIMEOUT));
        let t = match self.target(ctx, host) {
            Ok(t) => t,
            Err(m) => return no(m),
        };
        let production = t.env == Some(EnvKind::Production);
        let sudo_note = policy::uses_sudo(command).then(|| self.sudo_note(t.id, &t.name)).flatten();
        match policy::decide(t.policy, t.env, command) {
            Verdict::Block(why) => {
                self.log_why(ctx, "run", &t, command, "blocked", "The server's policy is \u{201c}blocked\u{201d}: AI agents cannot use it.", |_| {});
                return no(why);
            }
            Verdict::Auto => self.log_why(ctx, "run", &t, command, "allowed_without_asking", "Automatic: the server's policy is \u{201c}read-only auto\u{201d} and this command only reads.", |_| {}),
            Verdict::Ask => {
                let can_remember = !production && policy::is_read_only(command);
                let asked = why_asked(&t);
                match self.ask(ctx, "run", &t.name, command, production, can_remember) {
                    Decision::Once => self.log_why(ctx, "run", &t, command, "allowed", &format!("{asked}: you allowed it once."), |_| {}),
                    Decision::AlwaysRead if can_remember => {
                        // the window cannot widen what is allowed: the rule is checked here too
                        {
                            let mut e = self.engine();
                            if let Some(mut s) = e.store.server(t.id).cloned() {
                                s.ai_policy = AiPolicy::ReadOnlyAuto;
                                e.store.update_server(s);
                                let _ = e.store.save();
                            }
                        }
                        self.log_why(ctx, "run", &t, command, "allowed_and_remembered_for_reads", &format!("{asked}: you allowed it and chose \u{201c}always for read-only commands\u{201d} on this server."), |_| {});
                    }
                    Decision::AlwaysRead => self.log_why(ctx, "run", &t, command, "allowed", &format!("{asked}: you allowed it once."), |_| {}),
                    Decision::Terminal => {
                        self.log_why(ctx, "run", &t, command, "denied", &format!("{asked}: you chose to run it yourself in a terminal."), |e| e.note = "the person runs it themselves".into());
                        return no("The user chose to run this command themselves, in a terminal. Do not try it again; wait for what they tell you.");
                    }
                    Decision::Deny => {
                        self.log_why(ctx, "run", &t, command, "denied", &format!("{asked}: you denied it (or nobody answered in time)."), |_| {});
                        return no("The user did not allow this command.");
                    }
                }
            }
        }
        // the connection is built only now, after the answer: secrets are not held while waiting
        let (mut opts, extra, hide) = {
            let mut guard = self.engine();
            let e = &mut *guard;
            let Some(server) = e.store.server(t.id).cloned() else { return no("That server no longer exists.") };
            let argv = connect::ssh_argv(&e.store, &server);
            let mut o = connect::opts_for(&e.store, &mut e.vault, t.id, argv, 24, 200, true);
            if let Err(m) = e.agent_env(t.id, &mut o) {
                return no(format!("{m:?}"));
            }
            (o, e.ssh_extra.clone(), e.vault.values_to_hide())
        };
        // [ssh options…, extra…, --, target, command]
        let target = opts.argv.pop().unwrap_or_default();
        let needs_prompt = !opts.fills.is_empty();
        opts.argv.extend(extra);
        if !needs_prompt {
            opts.argv.extend(["-T".into(), "-o".into(), "BatchMode=yes".into()]);
        }
        opts.argv.extend(["--".into(), target, command.to_string()]);
        let started = Instant::now();
        let result = if needs_prompt { run_in_pty(opts, timeout) } else { run_plain(opts, timeout) };
        match result {
            Err(e) => {
                self.log_why(ctx, "run", &t, command, "failed", "", |x| x.note = e.chars().take(200).collect());
                no(format!("Could not run it: {e}"))
            }
            Ok(c) => {
                let (stdout, t1) = cap(redact(&c.stdout, &hide));
                let (stderr, t2) = cap(redact(&c.stderr, &hide));
                self.log_why(ctx, "run", &t, command, "finished", "", |x| {
                    x.exit_code = c.code;
                    x.bytes_out = Some(stdout.len() + stderr.len());
                    x.preview = crate::audit::preview_of(&stdout, &stderr);
                    x.output = crate::audit::output_of(&stdout, &stderr);
                    x.note = format!("{} ms{}", started.elapsed().as_millis(), if c.timed_out { ", timed out" } else { "" });
                });
                let mut answer = json!({
                    "host": t.name, "exit_code": c.code, "stdout": stdout, "stderr": stderr,
                    "truncated": t1 || t2, "timed_out": c.timed_out,
                    "note": if needs_prompt { "This login needed a typed password, so stdout and stderr come mixed in `stdout`." } else { "" },
                });
                if let Some(why) = sudo_note {
                    answer["sudo_note"] = json!(why);
                }
                ok(answer)
            }
        }
    }

    // ------------------------------------------------------------ on this computer

    fn sudo(&self, ctx: &Ctx, args: &Value) -> Value {
        let Some(command) = args.get("command").and_then(Value::as_str).filter(|c| !c.trim().is_empty() && c.len() <= 8000 && !c.contains('\0')) else {
            return no("oso_sudo needs a `command`.");
        };
        let timeout = Duration::from_secs(args.get("timeout_seconds").and_then(Value::as_u64).unwrap_or(DEFAULT_TIMEOUT).clamp(1, MAX_TIMEOUT));
        {
            let mut e = self.engine();
            let scope = match Scope::of(&e, ctx) {
                Ok(s) => s,
                Err(m) => return no(m),
            };
            if !e.vault.is_unlocked() {
                return no("oso's vault is locked: ask the person to unlock it, then try again.");
            }
            let Some(secret) = e.vault.links(LOCAL).sudo.filter(|_| e.vault.get(LOCAL, crate::vault::Kind::Sudo).is_some()) else {
                return no("No sudo password is saved for this computer: the person can add it in oso (Secrets → This computer's sudo password).");
            };
            // in a Space, the password of this computer is one more secret that has to be in it
            if let Scope::Space(sp) = &scope {
                if !sp.has_secret(secret) {
                    return no("This Space does not include the sudo password of this computer: the person can add it to the Space in oso.");
                }
            }
        }
        match self.ask(ctx, "sudo", "this computer", &format!("sudo {command}"), false, false) {
            Decision::Once | Decision::AlwaysRead => self.log(ctx, "sudo", "this computer", command, "allowed", |e| e.reason = "sudo always asks: you allowed it once.".into()),
            Decision::Terminal => {
                self.log(ctx, "sudo", "this computer", command, "denied", |e| {
                    e.note = "the person runs it themselves".into();
                    e.reason = "sudo always asks: you chose to run it yourself in a terminal.".into();
                });
                return no("The user chose to run this themselves, in a terminal. Do not try it again; wait for what they tell you.");
            }
            Decision::Deny => {
                self.log(ctx, "sudo", "this computer", command, "denied", |e| e.reason = "sudo always asks: you denied it (or nobody answered in time).".into());
                return no("The user did not allow this command.");
            }
        }
        let (mut pw, hide) = {
            let mut e = self.engine();
            let Some(p) = e.vault.get(LOCAL, crate::vault::Kind::Sudo) else { return no("The sudo password is gone: the vault changed.") };
            (Zeroizing::new(p), e.vault.values_to_hide())
        };
        let mut cmd = Command::new(&self.sudo_program);
        // -S: the password comes on stdin, once; -k: a cached ticket is not trusted; no prompt text to confuse the output
        cmd.args(["-S", "-k", "-p", "", "--", "sh", "-c", command]);
        let input = format!("{}\n", pw.as_str()).into_bytes();
        pw.clear();
        let result = run_capture(cmd, Some(input), timeout);
        self.finish_local(ctx, "sudo", "this computer", command, result, &hide)
    }

    fn run_with(&self, ctx: &Ctx, args: &Value) -> Value {
        let Some(command) = args.get("command").and_then(Value::as_str).filter(|c| !c.trim().is_empty() && c.len() <= 8000 && !c.contains('\0')) else {
            return no("oso_run_with needs a `command` and the `secrets` it needs.");
        };
        let names: Vec<String> = args.get("secrets").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
        if names.is_empty() {
            return no("Say which secrets the command needs, by name (see oso_context).");
        }
        let timeout = Duration::from_secs(args.get("timeout_seconds").and_then(Value::as_u64).unwrap_or(DEFAULT_TIMEOUT).clamp(1, MAX_TIMEOUT));
        let profile;
        let wanted: Vec<(String, String)> = {
            let e = self.engine();
            if !e.vault.is_unlocked() {
                return no("oso's vault is locked: ask the person to unlock it, then try again.");
            }
            let scope = match Scope::of(&e, ctx) {
                Ok(s) => s,
                Err(m) => return no(m),
            };
            profile = e.active_profile().to_string();
            let list = e.vault.list();
            let mut out = vec![];
            for n in &names {
                // a secret outside the scope is answered like one that does not exist
                let Some(m) = list.iter().find(|m| m.name.eq_ignore_ascii_case(n) && scope.has_secret(m.id, m.profile.as_deref())) else {
                    return no(format!("There is no secret called {n:?} in this {}.", scope.place()));
                };
                let Some(var) = m.env_var.clone() else {
                    return no(format!("The secret {n:?} has no environment variable, so it cannot be handed to a command: the person can set one in Secrets."));
                };
                out.push((m.name.clone(), var));
            }
            out
        };
        let shown = wanted.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(", ");
        match self.ask(ctx, "run_with", "this computer", &format!("{command}    [with the secrets: {shown}]"), false, false) {
            Decision::Once | Decision::AlwaysRead => self.log(ctx, "run_with", "this computer", &format!("{command} [with: {shown}]"), "allowed", |e| e.reason = "Handing secrets to a command always asks: you allowed it once.".into()),
            Decision::Terminal => return no("The user chose to run this themselves, in a terminal. Do not try it again."),
            Decision::Deny => {
                self.log(ctx, "run_with", "this computer", &format!("{command} [with: {shown}]"), "denied", |e| e.reason = "Handing secrets to a command always asks: you denied it (or nobody answered in time).".into());
                return no("The user did not allow this command.");
            }
        }
        let (env, hide) = {
            let mut e = self.engine();
            let mut env = connect::client_env();
            let Some(_) = e.vault.is_unlocked().then_some(()) else { return no("The vault was locked meanwhile.") };
            for (name, var) in &wanted {
                let Some(id) = e.vault.list().into_iter().find(|m| &m.name == name).map(|m| m.id) else { continue };
                if let Some(v) = e.vault.reveal(id) {
                    env.retain(|(k, _)| k != var);
                    env.push((var.clone(), v));
                }
            }
            (env, e.vault.values_to_hide())
        };
        let mut cmd = Command::new("sh");
        cmd.args(["-c", command]).env_clear().envs(env);
        if let Some(h) = directories::BaseDirs::new() {
            cmd.current_dir(h.home_dir());
        }
        let result = run_capture(cmd, None, timeout);
        self.finish_local(ctx, "run_with", "this computer", &format!("{command} [with: {shown}]"), result, &hide)
    }

    fn finish_local(&self, ctx: &Ctx, action: &str, host: &str, command: &str, result: std::io::Result<Captured>, hide: &[Zeroizing<String>]) -> Value {
        match result {
            Err(e) => {
                self.log(ctx, action, host, command, "failed", |x| x.note = e.to_string());
                no(format!("Could not run it: {e}"))
            }
            Ok(c) => {
                let (stdout, t1) = cap(redact(&c.stdout, hide));
                let (stderr, t2) = cap(redact(&c.stderr, hide));
                self.log(ctx, action, host, command, "finished", |x| {
                    x.exit_code = c.code;
                    x.bytes_out = Some(stdout.len() + stderr.len());
                    x.preview = crate::audit::preview_of(&stdout, &stderr);
                    x.output = crate::audit::output_of(&stdout, &stderr);
                });
                ok(json!({ "exit_code": c.code, "stdout": stdout, "stderr": stderr, "truncated": t1 || t2, "timed_out": c.timed_out }))
            }
        }
    }

    // ------------------------------------------------------------ tunnels

    fn tunnel(&self, ctx: &Ctx, args: &Value) -> Value {
        let (Some(name), Some(action)) = (args.get("name").and_then(Value::as_str), args.get("action").and_then(Value::as_str)) else {
            return no("oso_tunnel needs `name` and `action` (start or stop).");
        };
        if action != "start" && action != "stop" {
            return no("`action` is start or stop.");
        }
        let (server_id, forward_id, server_name, production, desc) = {
            let e = self.engine();
            let scope = match Scope::of(&e, ctx) {
                Ok(s) => s,
                Err(m) => return no(m),
            };
            let in_scope = scope.servers(&e);
            let hits: Vec<(&Server, &crate::store::Forward)> = in_scope.iter().flat_map(|s| s.forwards.iter().map(move |f| (s, f))).filter(|(_, f)| f.name.eq_ignore_ascii_case(name) || (f.name.is_empty() && f.bind_port.to_string() == name)).collect();
            match hits.as_slice() {
                [(s, f)] => (s.id, f.id, s.name.clone(), s.env_kind == Some(EnvKind::Production), format!("{} {} (port {})", action, if f.name.is_empty() { "a tunnel".to_string() } else { f.name.clone() }, f.bind_port)),
                [] => return no(format!("There is no tunnel called {name:?}; oso_context lists them.")),
                _ => return no(format!("More than one tunnel is called {name:?}.")),
            }
        };
        if action == "start" {
            match self.ask(ctx, "tunnel", &server_name, &desc, production, false) {
                Decision::Once | Decision::AlwaysRead => {}
                _ => {
                    self.log(ctx, "tunnel", &server_name, &desc, "denied", |e| e.reason = "Starting a tunnel always asks: you denied it (or nobody answered in time).".into());
                    return no("The user did not allow this.");
                }
            }
            self.log(ctx, "tunnel", &server_name, &desc, "allowed", |e| e.reason = "Starting a tunnel always asks: you allowed it.".into());
            if let Err(e) = self.engine().tunnel_start(server_id, forward_id, true) {
                return no(format!("Could not start it: {e:?}"));
            }
            // it takes a moment to be up or to fail
            let end = Instant::now() + Duration::from_secs(8);
            loop {
                let st = self.engine().tunnels().into_iter().find(|t| t.server_id == server_id && t.forward_id == forward_id);
                match st {
                    Some(t) if t.state != "starting" => return ok(json!({ "tunnel": name, "state": t.state, "message": t.message })),
                    _ if Instant::now() > end => return ok(json!({ "tunnel": name, "state": "starting", "message": "still connecting" })),
                    _ => std::thread::sleep(Duration::from_millis(150)),
                }
            }
        }
        self.engine().tunnel_stop(server_id, forward_id);
        self.log(ctx, "tunnel", &server_name, &desc, "allowed_without_asking", |e| e.reason = "Automatic: stopping a tunnel does not ask.".into());
        ok(json!({ "tunnel": name, "state": "off" }))
    }
}

/// `ssh` with its pipes (no password to type).
fn run_plain(opts: SpawnOpts, timeout: Duration) -> Result<Captured, String> {
    let mut cmd = Command::new(&opts.argv[0]);
    cmd.args(&opts.argv[1..]);
    if let Some(env) = opts.env {
        cmd.env_clear().envs(env);
    }
    run_capture(cmd, None, timeout).map_err(|e| e.to_string())
}

/// `ssh` in a terminal nobody sees, so that the saved password is typed at its prompt. Everything it prints is mixed.
fn run_in_pty(opts: SpawnOpts, timeout: Duration) -> Result<Captured, String> {
    let mut job = Job::spawn(opts).map_err(|e| format!("{e:#}"))?;
    let end = Instant::now() + timeout;
    let code = loop {
        if let Some(c) = job.poll() {
            break Some(c as i32);
        }
        if Instant::now() >= end {
            job.kill();
            break None;
        }
        std::thread::sleep(Duration::from_millis(40));
    };
    // let the last bytes arrive
    std::thread::sleep(Duration::from_millis(120));
    let text = strip_ansi(&job.output());
    // the question that was answered is not output
    let clean: Vec<&str> = text.lines().filter(|l| {
        let t = l.trim().to_lowercase();
        !(t.ends_with("password:") || t.contains("passphrase for") && t.ends_with(':'))
    }).collect();
    Ok(Captured { stdout: clean.join("\n"), stderr: String::new(), code, timed_out: code.is_none() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_blanked_out_longest_first() {
        let hide = vec![Zeroizing::new("hunter22".to_string()), Zeroizing::new("hunter22-and-more".to_string())];
        assert_eq!(redact("pw=hunter22-and-more and hunter22!", &hide), "pw=[hidden by oso] and [hidden by oso]!");
        assert_eq!(redact("nothing here", &hide), "nothing here");
    }

    #[test]
    fn terminal_decoration_is_removed() {
        assert_eq!(strip_ansi("\u{1b}[31mred\u{1b}[0m \u{1b}]0;title\u{7}text"), "red text");
    }

    #[test]
    fn long_output_is_cut_on_a_character() {
        let (s, cut) = cap("é".repeat(MAX_OUTPUT));
        assert!(cut && s.len() <= MAX_OUTPUT && s.chars().all(|c| c == 'é'));
        assert!(!cap("short".into()).1);
    }

    #[test]
    fn the_label_says_who_and_where() {
        let c = Ctx { id: 1, pane: 2, space: None, title: "acme-api".into(), agent: "claude-code".into() };
        assert_eq!(c.label(), "Claude in acme-api, pane 2");
        let c = Ctx { id: 1, pane: 3, space: None, title: "Local".into(), agent: "opencode".into() };
        assert_eq!(c.label(), "Opencode in Local, pane 3");
        let c = Ctx { id: 1, pane: 1, space: None, title: "x".into(), agent: String::new() };
        assert_eq!(c.label(), "An AI agent in x, pane 1");
    }

    #[test]
    fn a_program_that_prints_a_lot_and_one_that_hangs_are_both_handled() {
        let mut big = Command::new("sh");
        big.args(["-c", "head -c 3000000 /dev/zero | tr '\\0' 'a'; echo done >&2"]);
        let c = run_capture(big, None, Duration::from_secs(20)).unwrap();
        assert!(c.stdout.len() <= MAX_OUTPUT + 4096 && c.stderr.contains("done") && c.code == Some(0));
        let mut slow = Command::new("sh");
        slow.args(["-c", "sleep 30"]);
        let t = Instant::now();
        let c = run_capture(slow, None, Duration::from_millis(300)).unwrap();
        assert!(c.timed_out && t.elapsed() < Duration::from_secs(5));
        let mut cat = Command::new("sh");
        cat.args(["-c", "read x; echo got:$x"]);
        assert!(run_capture(cat, Some(b"hello\n".to_vec()), Duration::from_secs(5)).unwrap().stdout.contains("got:hello"));
    }
}
