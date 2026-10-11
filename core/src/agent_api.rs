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
use crate::store::{EnvKind, Server};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::process::{Command, Stdio};
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
}

/// Who is asking, from where.
struct Ctx {
    pane: u64,
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
        Handler { engine, approvals, audit: Audit::new(path), sudo_program: "sudo".into() }
    }

    pub fn audit_path(&self) -> &std::path::Path {
        self.audit.path()
    }

    /// Answers one request. Never panics on a bad one: the answer says what was wrong.
    pub fn handle(&self, call: AgentCall) -> Value {
        let agent = call.body.get("agent").and_then(Value::as_str).unwrap_or("").chars().take(60).collect();
        let ctx = Ctx { pane: call.sid, title: call.title, agent };
        let tool = call.body.get("tool").and_then(Value::as_str).unwrap_or("");
        let args = call.body.get("args").cloned().unwrap_or(Value::Null);
        match tool {
            "oso_context" => self.context(&ctx),
            "oso_hosts" => self.hosts(&ctx, &args),
            "oso_run" => self.run(&ctx, &args),
            "oso_sudo" => self.sudo(&ctx, &args),
            "oso_run_with" => self.run_with(&ctx, &args),
            "oso_tunnel" => self.tunnel(&ctx, &args),
            other => no(format!("oso has no tool called {other:?}")),
        }
    }

    fn engine(&self) -> std::sync::MutexGuard<'_, Engine> {
        self.engine.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn log(&self, ctx: &Ctx, action: &str, host: &str, command: &str, decision: &str, extra: impl FnOnce(&mut Entry)) {
        let mut e = Entry { pane: ctx.pane, agent: ctx.agent.clone(), action: action.into(), host: host.into(), command: command.chars().take(2000).collect(), decision: decision.into(), ..Default::default() };
        extra(&mut e);
        self.audit.log(e);
    }

    // ------------------------------------------------------------ reading

    fn context(&self, ctx: &Ctx) -> Value {
        let mut e = self.engine();
        let profile = e.active_profile().to_string();
        let pname = e.profiles().iter().find(|p| p.id == profile).map(|p| p.name.clone()).unwrap_or_else(|| profile.clone());
        let servers: Vec<Server> = e.store.servers.iter().filter(|s| e.store.server_in(s, &profile)).cloned().collect();
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
                        "reached_through": s.jump.and_then(|j| e.store.server(j)).map(|j| j.name.clone()),
                        "tunnels": tunnels,
                    })
                })
                .collect()
        };
        let vars: Vec<Value> = e.vault.list().into_iter().filter(|m| m.agent_visible && m.env_var.is_some() && m.profile.as_deref().is_none_or(|p| p == profile)).filter_map(|m| m.env_var).map(Value::from).collect();
        let more: Vec<Value> = e.vault.list().into_iter().filter(|m| !m.agent_visible && m.env_var.is_some() && m.profile.as_deref().is_none_or(|p| p == profile)).map(|m| Value::from(m.name)).collect();
        let many = servers.len() > MANY_SERVERS;
        let out = ok(json!({
            "profile": { "id": profile, "name": pname },
            "terminal": ctx.title,
            "servers": list,
            "servers_note": if many { "There are many servers: this is only their names. Use oso_hosts with a search to see one." } else { "" },
            "environment_variables_already_set_in_your_terminal": vars,
            "secrets_you_can_use_with_oso_run_with": more,
            "rules": "Never ask the user for a password or a key: use oso_run (a command on a server), oso_sudo (sudo on this computer) or oso_run_with (a local command that needs a secret). The user is asked to approve what is not a plain read. Production servers always ask.",
        }));
        drop(e);
        self.log(ctx, "context", "", "", "allowed_without_asking", |_| {});
        out
    }

    fn hosts(&self, ctx: &Ctx, args: &Value) -> Value {
        let q = args.get("query").and_then(Value::as_str).unwrap_or("").to_lowercase();
        let e = self.engine();
        let profile = e.active_profile().to_string();
        let found: Vec<Value> = e
            .store
            .servers
            .iter()
            .filter(|s| e.store.server_in(s, &profile))
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

    fn target(&self, query: &str) -> Result<Target, String> {
        let mut e = self.engine();
        let profile = e.active_profile().to_string();
        let q = query.trim().to_lowercase();
        let visible: Vec<&Server> = e.store.servers.iter().filter(|s| e.store.server_in(s, &profile)).collect();
        let mut hits: Vec<&Server> = visible.iter().copied().filter(|s| s.name.to_lowercase() == q).collect();
        if hits.is_empty() {
            hits = visible.iter().copied().filter(|s| s.host.to_lowercase() == q).collect();
        }
        if hits.is_empty() {
            hits = visible.iter().copied().filter(|s| s.name.to_lowercase().contains(&q)).collect();
        }
        let s = match hits.as_slice() {
            [one] => (*one).clone(),
            [] => return Err(format!("There is no server called {query:?} in this profile. Use oso_hosts to search.")),
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

    fn run(&self, ctx: &Ctx, args: &Value) -> Value {
        let (Some(host), Some(command)) = (args.get("host").and_then(Value::as_str), args.get("command").and_then(Value::as_str)) else {
            return no("oso_run needs `host` and `command`.");
        };
        if command.trim().is_empty() || command.len() > 8000 || command.contains('\0') {
            return no("The command is empty, too long, or has a NUL in it.");
        }
        let timeout = Duration::from_secs(args.get("timeout_seconds").and_then(Value::as_u64).unwrap_or(DEFAULT_TIMEOUT).clamp(1, MAX_TIMEOUT));
        let t = match self.target(host) {
            Ok(t) => t,
            Err(m) => return no(m),
        };
        let production = t.env == Some(EnvKind::Production);
        match policy::decide(t.policy, t.env, command) {
            Verdict::Block(why) => {
                self.log(ctx, "run", &t.name, command, "blocked", |_| {});
                return no(why);
            }
            Verdict::Auto => self.log(ctx, "run", &t.name, command, "allowed_without_asking", |_| {}),
            Verdict::Ask => {
                let can_remember = !production && policy::is_read_only(command);
                match self.ask(ctx, "run", &t.name, command, production, can_remember) {
                    Decision::Once => self.log(ctx, "run", &t.name, command, "allowed", |_| {}),
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
                        self.log(ctx, "run", &t.name, command, "allowed_and_remembered_for_reads", |_| {});
                    }
                    Decision::AlwaysRead => self.log(ctx, "run", &t.name, command, "allowed", |_| {}),
                    Decision::Terminal => {
                        self.log(ctx, "run", &t.name, command, "denied", |e| e.note = "the person runs it themselves".into());
                        return no("The user chose to run this command themselves, in a terminal. Do not try it again; wait for what they tell you.");
                    }
                    Decision::Deny => {
                        self.log(ctx, "run", &t.name, command, "denied", |_| {});
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
                self.log(ctx, "run", &t.name, command, "failed", |x| x.note = e.chars().take(200).collect());
                no(format!("Could not run it: {e}"))
            }
            Ok(c) => {
                let (stdout, t1) = cap(redact(&c.stdout, &hide));
                let (stderr, t2) = cap(redact(&c.stderr, &hide));
                self.log(ctx, "run", &t.name, command, "finished", |x| {
                    x.exit_code = c.code;
                    x.bytes_out = Some(stdout.len() + stderr.len());
                    x.note = format!("{} ms{}", started.elapsed().as_millis(), if c.timed_out { ", timed out" } else { "" });
                });
                ok(json!({
                    "host": t.name, "exit_code": c.code, "stdout": stdout, "stderr": stderr,
                    "truncated": t1 || t2, "timed_out": c.timed_out,
                    "note": if needs_prompt { "This login needed a typed password, so stdout and stderr come mixed in `stdout`." } else { "" },
                }))
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
            if !e.vault.is_unlocked() {
                return no("oso's vault is locked: ask the person to unlock it, then try again.");
            }
            if e.vault.get(LOCAL, crate::vault::Kind::Sudo).is_none() {
                return no("No sudo password is saved for this computer: the person can add it in oso (Secrets → This computer's sudo password).");
            }
        }
        match self.ask(ctx, "sudo", "this computer", &format!("sudo {command}"), false, false) {
            Decision::Once | Decision::AlwaysRead => self.log(ctx, "sudo", "this computer", command, "allowed", |_| {}),
            Decision::Terminal => {
                self.log(ctx, "sudo", "this computer", command, "denied", |e| e.note = "the person runs it themselves".into());
                return no("The user chose to run this themselves, in a terminal. Do not try it again; wait for what they tell you.");
            }
            Decision::Deny => {
                self.log(ctx, "sudo", "this computer", command, "denied", |_| {});
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
            profile = e.active_profile().to_string();
            let list = e.vault.list();
            let mut out = vec![];
            for n in &names {
                let Some(m) = list.iter().find(|m| m.name.eq_ignore_ascii_case(n) && m.profile.as_deref().is_none_or(|p| p == profile)) else {
                    return no(format!("There is no secret called {n:?} in this profile."));
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
            Decision::Once | Decision::AlwaysRead => self.log(ctx, "run_with", "this computer", &format!("{command} [with: {shown}]"), "allowed", |_| {}),
            Decision::Terminal => return no("The user chose to run this themselves, in a terminal. Do not try it again."),
            Decision::Deny => {
                self.log(ctx, "run_with", "this computer", &format!("{command} [with: {shown}]"), "denied", |_| {});
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
            let profile = e.active_profile().to_string();
            let hits: Vec<(&Server, &crate::store::Forward)> = e.store.servers.iter().filter(|s| e.store.server_in(s, &profile)).flat_map(|s| s.forwards.iter().map(move |f| (s, f))).filter(|(_, f)| f.name.eq_ignore_ascii_case(name) || (f.name.is_empty() && f.bind_port.to_string() == name)).collect();
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
                    self.log(ctx, "tunnel", &server_name, &desc, "denied", |_| {});
                    return no("The user did not allow this.");
                }
            }
            self.log(ctx, "tunnel", &server_name, &desc, "allowed", |_| {});
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
        self.log(ctx, "tunnel", &server_name, &desc, "allowed_without_asking", |_| {});
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
        let c = Ctx { pane: 2, title: "acme-api".into(), agent: "claude-code".into() };
        assert_eq!(c.label(), "Claude in acme-api, pane 2");
        let c = Ctx { pane: 3, title: "Local".into(), agent: "opencode".into() };
        assert_eq!(c.label(), "Opencode in Local, pane 3");
        let c = Ctx { pane: 1, title: "x".into(), agent: String::new() };
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
