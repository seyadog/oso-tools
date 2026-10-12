//! What an AI agent can do through oso, end to end: a terminal of oso with its token, the background server that
//! checks it, the window's engine that answers (with the person's approval played by the test), a real `sshd`
//! to run commands on, and trap values in the vault that must never come out.

mod common;

use common::*;
use oso_core::agent_api::Handler;
use oso_core::approvals::{ApprovalRequest, Approvals, Decision};
use oso_core::daemon::{self, AgentCall, Daemon};
use oso_core::engine::{Emit, Engine, Focus};
use oso_core::mcp;
use oso_core::policy::AiPolicy;
use oso_core::session::SpawnOpts;
use oso_core::spaces::Space;
use oso_core::store::{Auth, EnvKind, Server, Store};
use oso_core::vault::{SecretKind, SecretMeta, Vault};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const TRAP_API: &str = "TRAP-API-KEY-7f3a9c1d";
const TRAP_TOKEN: &str = "TRAP-TOKEN-b82e44aa";
const TRAP_SUDO: &str = "TRAP-SUDO-PW-5519dd";
const TRAP_LOGIN: &str = "TRAP-LOGIN-PW-0c77ee";

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("oso-mcp-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn user() -> String {
    String::from_utf8(std::process::Command::new("id").arg("-un").output().unwrap().stdout).unwrap().trim().to_string()
}

/// The person at the window: answers every request with `decide`, and remembers what was shown.
fn person(approvals: &Arc<Approvals>, decide: impl Fn(&ApprovalRequest) -> Decision + Send + Sync + 'static) -> Arc<Mutex<Vec<ApprovalRequest>>> {
    let seen: Arc<Mutex<Vec<ApprovalRequest>>> = Arc::default();
    let (s, a) = (Arc::clone(&seen), Arc::clone(approvals));
    approvals.on_request(Arc::new(move |r| {
        let d = decide(&r);
        s.lock().unwrap().push(r.clone());
        let a = Arc::clone(&a);
        std::thread::spawn(move || {
            a.answer(r.id, d);
        });
    }));
    seen
}

struct Rig {
    engine: Arc<Mutex<Engine>>,
    handler: Handler,
    approvals: Arc<Approvals>,
    dir: PathBuf,
}

fn rig(name: &str) -> Rig {
    let dir = temp(name);
    let emit: Emit = Arc::new(|_, _| {});
    let store = Store::load_from(dir.join("servers.json")).unwrap();
    let mut e = Engine::new(store, Vault::new(dir.join("vault.json")), emit);
    e.vault_unlock("master").unwrap();
    let meta = |name: &str, kind, var: Option<&str>, visible| SecretMeta { id: 0, name: name.into(), kind, profile: None, env_var: var.map(str::to_string), agent_visible: visible, notes: String::new(), updated: 0 };
    e.secret_save(meta("anthropic", SecretKind::ApiKey, Some("ANTHROPIC_API_KEY"), true), Some(TRAP_API)).unwrap();
    e.secret_save(meta("deploy token", SecretKind::Token, Some("DEPLOY_TOKEN"), false), Some(TRAP_TOKEN)).unwrap();
    e.vault_set(0, true, Some(TRAP_SUDO)).unwrap();
    let engine = Arc::new(Mutex::new(e));
    let approvals = Approvals::new();
    let handler = Handler::new(Arc::clone(&engine), Arc::clone(&approvals));
    Rig { engine, handler, approvals, dir }
}

/// A server on the throwaway sshd, logged into with its key file.
fn add_server(r: &Rig, sshd: &Sshd, name: &str, env: Option<EnvKind>, policy: AiPolicy, notes: &str) -> u64 {
    let mut e = r.engine.lock().unwrap();
    e.ssh_extra = ["-o", "UserKnownHostsFile=/dev/null", "-o", "StrictHostKeyChecking=no", "-o", "LogLevel=ERROR"].iter().map(|s| s.to_string()).collect();
    e.save_server(
        Server { name: name.into(), host: "127.0.0.1".into(), port: sshd.port, user: user(), auth: Auth::Key, key_path: sshd.dir.join("client").to_string_lossy().into(), env_kind: env, ai_policy: policy, notes: notes.into(), ..Default::default() },
        None,
        None,
    )
    .unwrap()
}

fn call(r: &Rig, tool: &str, args: Value) -> Value {
    r.handler.handle(AgentCall { sid: 2, space: None, title: "acme-api".into(), body: json!({ "tool": tool, "args": args, "agent": "claude-code" }) })
}

fn audit(r: &Rig) -> Vec<Value> {
    std::fs::read_to_string(r.handler.audit_path()).unwrap_or_default().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

macro_rules! need_sshd {
    ($tag:expr) => {
        match start_sshd($tag) {
            Some(s) => s,
            None => return eprintln!("skipped: no sshd here"),
        }
    };
}

fn stdout_of(v: &Value) -> String {
    v["result"]["stdout"].as_str().unwrap_or("").to_string()
}

#[test]
fn the_context_describes_the_servers_with_their_notes_and_the_names_of_secrets_but_never_a_value() {
    let sshd = need_sshd!("ctx");
    let r = rig("ctx");
    add_server(&r, &sshd, "acme-api", Some(EnvKind::Production), AiPolicy::Ask, "nginx + API node, logs in /var/log/acme");
    add_server(&r, &sshd, "dev-box", Some(EnvKind::Dev), AiPolicy::Ask, "");
    let c = call(&r, "oso_context", Value::Null);
    assert_eq!(c["ok"], true, "{c}");
    let text = c.to_string();
    for trap in [TRAP_API, TRAP_TOKEN, TRAP_SUDO] {
        assert!(!text.contains(trap), "{trap} leaked: {text}");
    }
    assert_eq!(c["result"]["profile"]["id"], "default");
    let acme = c["result"]["servers"].as_array().unwrap().iter().find(|s| s["name"] == "acme-api").unwrap();
    assert_eq!((acme["environment"].as_str(), acme["notes"].as_str()), (Some("production"), Some("nginx + API node, logs in /var/log/acme")));
    assert_eq!(c["result"]["environment_variables_already_set_in_your_terminal"], json!(["ANTHROPIC_API_KEY"]));
    assert_eq!(c["result"]["secrets_you_can_use_with_oso_run_with"], json!(["deploy token"]), "the one that is not in the environment is offered by name");
    // searching
    let h = call(&r, "oso_hosts", json!({ "query": "API node" }));
    assert_eq!(h["result"]["hosts"].as_array().unwrap().len(), 1);
    assert!(!h.to_string().contains("TRAP"));
    assert_eq!(call(&r, "oso_hosts", json!({}))["result"]["hosts"].as_array().unwrap().len(), 2);
    assert!(call(&r, "oso_nope", Value::Null)["error"].as_str().unwrap().contains("no tool"));
}

#[test]
fn a_command_on_a_server_asks_naming_the_pane_and_runs_only_when_allowed() {
    let sshd = need_sshd!("run");
    let r = rig("run");
    add_server(&r, &sshd, "acme-api", None, AiPolicy::Ask, "");
    let seen = person(&r.approvals, |_| Decision::Once);
    let out = call(&r, "oso_run", json!({ "host": "acme-api", "command": "echo hello; echo oops >&2; exit 3" }));
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!((stdout_of(&out).trim(), out["result"]["exit_code"].as_i64()), ("hello", Some(3)));
    assert!(out["result"]["stderr"].as_str().unwrap().contains("oops"), "stderr comes apart from stdout");
    let q = seen.lock().unwrap()[0].clone();
    assert_eq!((q.label.as_str(), q.host.as_str(), q.command.as_str(), q.action.as_str()), ("Claude in acme-api, pane 2", "acme-api", "echo hello; echo oops >&2; exit 3", "run"));
    assert!(!q.production && !q.can_remember, "a command that is not a plain read cannot be remembered");

    // the person says no: nothing runs
    let marker = r.dir.join("marker");
    let no = rig_like(&r);
    person(&no.approvals, |_| Decision::Deny);
    let denied = call(&no, "oso_run", json!({ "host": "acme-api", "command": format!("touch {}", marker.display()) }));
    assert_eq!(denied["ok"], false);
    assert!(denied["error"].as_str().unwrap().contains("did not allow"));
    std::thread::sleep(Duration::from_millis(300));
    assert!(!marker.exists(), "a denied command was not run");
    // "I will do it myself" is a no too, with its own words
    let me = rig_like(&r);
    person(&me.approvals, |_| Decision::Terminal);
    assert!(call(&me, "oso_run", json!({ "host": "acme-api", "command": "ls" }))["error"].as_str().unwrap().contains("themselves"));
    // no window at all: no
    let alone = rig_like(&r);
    assert_eq!(call(&alone, "oso_run", json!({ "host": "acme-api", "command": "ls" }))["ok"], false);

    // the record: each decision, with the pane and the command, and nothing secret
    let log = audit(&r);
    assert!(log.iter().any(|l| l["decision"] == "allowed" && l["host"] == "acme-api" && l["pane"] == 2 && l["command"] == "echo hello; echo oops >&2; exit 3"));
    assert!(log.iter().any(|l| l["decision"] == "finished" && l["exit_code"] == 3));
    assert!(!std::fs::read_to_string(r.handler.audit_path()).unwrap().contains("TRAP"));
    let denied_log = audit(&no);
    assert!(denied_log.iter().any(|l| l["decision"] == "denied"));
}

/// Another handler over the same engine (to give it its own approvals).
fn rig_like(r: &Rig) -> Rig {
    let approvals = Approvals::new();
    Rig { engine: Arc::clone(&r.engine), handler: Handler::new(Arc::clone(&r.engine), Arc::clone(&approvals)), approvals, dir: r.dir.join("other") }
}

#[test]
fn the_policy_of_a_server_decides_what_goes_through_without_asking() {
    let sshd = need_sshd!("policy");
    let r = rig("policy");
    add_server(&r, &sshd, "reader", None, AiPolicy::ReadOnlyAuto, "");
    add_server(&r, &sshd, "prod", Some(EnvKind::Production), AiPolicy::ReadOnlyAuto, "");
    add_server(&r, &sshd, "closed", None, AiPolicy::Blocked, "");
    // nobody is at the window: whatever has to ask is refused
    let read = call(&r, "oso_run", json!({ "host": "reader", "command": "echo fine | cat" }));
    assert_eq!((read["ok"].as_bool(), stdout_of(&read).trim()), (Some(true), "fine"), "a read goes through without asking: {read}");
    assert_eq!(call(&r, "oso_run", json!({ "host": "reader", "command": "echo x > /tmp/oso-never" }))["ok"], false, "a write has to ask");
    let prod = call(&r, "oso_run", json!({ "host": "prod", "command": "echo fine" }));
    assert_eq!(prod["ok"], false, "production asks even for a read: {prod}");
    let blocked = call(&r, "oso_run", json!({ "host": "closed", "command": "echo fine" }));
    assert!(blocked["error"].as_str().unwrap().contains("does not let AI agents"), "{blocked}");
    let log = audit(&r);
    assert!(log.iter().any(|l| l["host"] == "reader" && l["decision"] == "allowed_without_asking"));
    assert!(log.iter().any(|l| l["host"] == "closed" && l["decision"] == "blocked"));

    // with the person present, production shows in red and cannot be remembered
    let seen = person(&r.approvals, |_| Decision::Once);
    assert_eq!(call(&r, "oso_run", json!({ "host": "prod", "command": "echo fine" }))["ok"], true);
    let q = seen.lock().unwrap()[0].clone();
    assert!(q.production && !q.can_remember);
}

#[test]
fn a_server_nobody_configured_asks_even_for_df_h_and_the_log_says_which_rule_applied() {
    let sshd = need_sshd!("whichrule");
    let r = rig("whichrule");
    // a server added the ordinary way gets the default policy, and the default is to ask
    let id = add_server(&r, &sshd, "fresh", None, AiPolicy::default(), "");
    assert_eq!(r.engine.lock().unwrap().store.server(id).unwrap().ai_policy, AiPolicy::Ask);
    // nobody at the window: it is refused, and nothing ran
    let alone = call(&r, "oso_run", json!({ "host": "fresh", "command": "df -h" }));
    assert_eq!(alone["ok"], false, "df -h on a server with no policy must not run by itself: {alone}");
    assert!(!audit(&r).iter().any(|l| l["decision"] == "finished"), "it did not run");
    // the person is there and says yes once: it asks, runs, and the record says who decided and why
    let seen = person(&r.approvals, |_| Decision::Once);
    let out = call(&r, "oso_run", json!({ "host": "fresh", "command": "df -h" }));
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(seen.lock().unwrap().len(), 1, "it asked");
    let log = audit(&r);
    let asked = log.iter().find(|l| l["decision"] == "allowed").expect("an allowed line");
    assert_eq!(asked["policy"], "ask");
    assert_eq!(asked["reason"].as_str().unwrap(), "The server's policy is \u{201c}ask\u{201d}: you allowed it once.");
    let finished = log.iter().find(|l| l["decision"] == "finished").expect("a finished line");
    assert_eq!(finished["id"], asked["id"], "the two lines belong to one request");
    assert!(finished["preview"].as_str().unwrap().contains("Filesystem"), "the first lines of the output are kept");
    let detail = oso_core::audit::find_detail(r.handler.audit_path(), finished["id"].as_u64().unwrap()).expect("the detail of the request");
    assert_eq!(detail.command, "df -h");
    assert!(detail.output.contains("Filesystem") && detail.output.lines().count() >= 2, "the whole output, not only the first lines");
    // the panel's view of it
    let acts = oso_core::audit::recent_activity(r.handler.audit_path(), 10);
    let mine = acts.iter().find(|a| a.command == "df -h" && a.decision == "allowed").unwrap();
    assert_eq!((mine.state.as_str(), mine.exit_code, mine.policy.as_str(), mine.host.as_str(), mine.pane), ("finished", Some(0), "ask", "fresh", 2));
    assert!(mine.preview.contains("Filesystem"));

    // a server set to "read-only auto" lets it through, and says that is why
    add_server(&r, &sshd, "trusted", None, AiPolicy::ReadOnlyAuto, "");
    seen.lock().unwrap().clear();
    assert_eq!(call(&r, "oso_run", json!({ "host": "trusted", "command": "df -h" }))["ok"], true);
    assert!(seen.lock().unwrap().is_empty());
    let auto = audit(&r).into_iter().find(|l| l["host"] == "trusted" && l["decision"] == "allowed_without_asking").unwrap();
    assert_eq!(auto["policy"], "read_only_auto");
    assert!(auto["reason"].as_str().unwrap().starts_with("Automatic"));
    // and production asks whatever its policy says, with that reason
    add_server(&r, &sshd, "prod", Some(EnvKind::Production), AiPolicy::ReadOnlyAuto, "");
    assert_eq!(call(&r, "oso_run", json!({ "host": "prod", "command": "df -h" }))["ok"], true);
    assert_eq!(seen.lock().unwrap().len(), 1, "production asked");
    let prod = audit(&r).into_iter().find(|l| l["host"] == "prod" && l["decision"] == "allowed").unwrap();
    assert!(prod["reason"].as_str().unwrap().starts_with("Production always asks"));
}

#[test]
fn what_an_agent_types_and_what_comes_back_are_blanked_of_secrets_in_the_record_too() {
    let sshd = need_sshd!("recordhides");
    let r = rig("recordhides");
    add_server(&r, &sshd, "box", None, AiPolicy::ReadOnlyAuto, "");
    let out = call(&r, "oso_run", json!({ "host": "box", "command": format!("echo {TRAP_API}") }));
    assert!(!out.to_string().contains("TRAP"), "{out}");
    let raw = std::fs::read_to_string(r.handler.audit_path()).unwrap();
    assert!(!raw.contains("TRAP"), "the log holds no vault secret: {raw}");
    let acts = oso_core::audit::recent_activity(r.handler.audit_path(), 10);
    assert!(acts.iter().any(|a| a.command.contains("[hidden by oso]") && a.preview.contains("[hidden by oso]")), "{acts:?}");
    let id = acts.iter().find(|a| a.command.contains("[hidden by oso]")).unwrap().id;
    let detail = oso_core::audit::find_detail(r.handler.audit_path(), id).unwrap();
    assert!(detail.output.contains("[hidden by oso]") && !detail.output.contains("TRAP") && !detail.command.contains("TRAP"), "{detail:?}");
}

#[test]
fn sudo_on_a_server_with_no_saved_sudo_password_says_so_instead_of_failing_quietly() {
    let sshd = need_sshd!("sudonote");
    let r = rig("sudonote");
    let id = add_server(&r, &sshd, "bare", None, AiPolicy::ReadOnlyAuto, "");
    person(&r.approvals, |_| Decision::Once);
    // the command hides its own errors, as the real one did: the agent would see nothing wrong
    let out = call(&r, "oso_run", json!({ "host": "bare", "command": "sudo -n true 2>/dev/null; echo after" }));
    assert_eq!(out["ok"], true, "{out}");
    let note = out["result"]["sudo_note"].as_str().expect("a note about sudo");
    assert!(note.contains("no sudo password saved for bare") && note.contains("add the sudo password for bare in oso"), "{note}");
    // a command that only names the word gets no note
    let plain = call(&r, "oso_run", json!({ "host": "bare", "command": "echo sudo" }));
    assert!(plain["result"].get("sudo_note").is_none(), "{plain}");
    // with a password saved the note changes: it is saved, but oso does not type it into a command run this way
    r.engine.lock().unwrap().vault_set(id, true, Some(TRAP_SUDO)).unwrap();
    let saved = call(&r, "oso_run", json!({ "host": "bare", "command": "sudo -n true; echo after" }));
    let note = saved["result"]["sudo_note"].as_str().expect("a note about sudo");
    assert!(!note.contains("no sudo password saved") && note.contains("does not type a sudo password"), "{note}");
    assert!(!saved.to_string().contains("TRAP"), "the password never comes back");
    // the vault locked: it cannot tell, and says so
    r.engine.lock().unwrap().vault_lock();
    let locked = call(&r, "oso_run", json!({ "host": "bare", "command": "sudo -n true" }));
    assert!(locked["result"]["sudo_note"].as_str().unwrap().contains("vault is locked"), "{locked}");
}

#[test]
fn always_for_reads_changes_the_servers_policy_but_only_for_a_read_and_never_on_production() {
    let sshd = need_sshd!("always");
    let r = rig("always");
    let id = add_server(&r, &sshd, "box", None, AiPolicy::Ask, "");
    let prod = add_server(&r, &sshd, "prod", Some(EnvKind::Production), AiPolicy::Ask, "");
    let seen = person(&r.approvals, |_| Decision::AlwaysRead);
    assert_eq!(call(&r, "oso_run", json!({ "host": "box", "command": "echo one" }))["ok"], true);
    assert!(seen.lock().unwrap()[0].can_remember, "a plain read can be remembered");
    assert_eq!(r.engine.lock().unwrap().store.server(id).unwrap().ai_policy, AiPolicy::ReadOnlyAuto);
    // the next read does not ask
    seen.lock().unwrap().clear();
    assert_eq!(call(&r, "oso_run", json!({ "host": "box", "command": "echo two" }))["ok"], true);
    assert!(seen.lock().unwrap().is_empty(), "it did not ask again");
    // a window that answers "always" to a write (or on production) does not widen anything
    let id2 = add_server(&r, &sshd, "box2", None, AiPolicy::Ask, "");
    call(&r, "oso_run", json!({ "host": "box2", "command": "touch /tmp/oso-x-never; true" }));
    assert_eq!(r.engine.lock().unwrap().store.server(id2).unwrap().ai_policy, AiPolicy::Ask);
    call(&r, "oso_run", json!({ "host": "prod", "command": "echo read" }));
    assert_eq!(r.engine.lock().unwrap().store.server(prod).unwrap().ai_policy, AiPolicy::Ask);
    // it survives a restart
    let again = Store::load_from(r.engine.lock().unwrap().store_path()).unwrap();
    assert_eq!(again.server(id).unwrap().ai_policy, AiPolicy::ReadOnlyAuto);
}

#[test]
fn sudo_on_this_computer_types_the_password_and_the_password_never_comes_back() {
    let r = rig("sudo");
    // a stand-in for sudo that does what a bad one would: print everything it is given on stdin
    let fake = r.dir.join("fake-sudo");
    std::fs::write(&fake, "#!/bin/sh\nread pw\necho \"sudo got: $pw\"\necho \"args: $*\" >&2\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut h = Handler::new(Arc::clone(&r.engine), Arc::clone(&r.approvals));
    h.sudo_program = fake.to_string_lossy().into();
    let r = Rig { handler: h, ..r };
    // asks first
    assert_eq!(call(&r, "oso_sudo", json!({ "command": "systemctl restart x" }))["ok"], false, "nobody to ask: no");
    let seen = person(&r.approvals, |_| Decision::Once);
    let out = call(&r, "oso_sudo", json!({ "command": "systemctl restart x" }));
    assert_eq!(out["ok"], true, "{out}");
    let all = out.to_string();
    assert!(!all.contains(TRAP_SUDO), "the password was printed by the program and oso blanked it: {all}");
    assert!(all.contains("[hidden by oso]"), "{all}");
    assert!(out["result"]["stderr"].as_str().unwrap().contains("-S -k -p  -- sh -c systemctl restart x"), "it is run as `sudo -S … sh -c <command>`");
    let q = seen.lock().unwrap()[0].clone();
    assert_eq!((q.action.as_str(), q.host.as_str(), q.command.as_str()), ("sudo", "this computer", "sudo systemctl restart x"));
    assert!(!std::fs::read_to_string(r.handler.audit_path()).unwrap().contains(TRAP_SUDO));
    // no sudo password saved, or the vault locked: said in words
    r.engine.lock().unwrap().vault_set(0, true, None).unwrap();
    assert!(call(&r, "oso_sudo", json!({ "command": "ls" }))["error"].as_str().unwrap().contains("No sudo password"));
    r.engine.lock().unwrap().vault_lock();
    assert!(call(&r, "oso_sudo", json!({ "command": "ls" }))["error"].as_str().unwrap().contains("locked"));
}

#[test]
fn a_local_command_gets_only_the_secrets_it_names_and_what_it_prints_is_blanked() {
    let r = rig("with");
    let seen = person(&r.approvals, |_| Decision::Once);
    let out = call(&r, "oso_run_with", json!({ "command": "echo token=$DEPLOY_TOKEN len=${#DEPLOY_TOKEN} api=[$ANTHROPIC_API_KEY]", "secrets": ["deploy token"] }));
    assert_eq!(out["ok"], true, "{out}");
    let so = stdout_of(&out);
    assert!(so.contains("token=[hidden by oso]") && so.contains(&format!("len={}", TRAP_TOKEN.len())), "the command did get the value: {so}");
    assert!(so.contains("api=[]"), "and no other secret came along: {so}");
    assert!(!out.to_string().contains(TRAP_TOKEN));
    let q = seen.lock().unwrap()[0].clone();
    assert!(q.command.contains("deploy token") && !q.command.contains(TRAP_TOKEN), "the person is shown the names: {}", q.command);
    assert!(call(&r, "oso_run_with", json!({ "command": "ls", "secrets": ["nope"] }))["error"].as_str().unwrap().contains("no secret called"));
    assert!(call(&r, "oso_run_with", json!({ "command": "ls", "secrets": [] }))["error"].as_str().unwrap().contains("which secrets"));
    let none = rig_like(&r);
    person(&none.approvals, |_| Decision::Deny);
    assert!(call(&none, "oso_run_with", json!({ "command": "echo $DEPLOY_TOKEN", "secrets": ["deploy token"] }))["error"].as_str().unwrap().contains("did not allow"));
}

#[test]
fn a_server_login_password_never_shows_up_in_what_an_agent_gets() {
    // the remote command prints what looks like the login password: it is blanked
    let sshd = need_sshd!("blank");
    let r = rig("blank");
    add_server(&r, &sshd, "box", None, AiPolicy::Ask, "");
    r.engine.lock().unwrap().vault_set(0, false, Some(TRAP_LOGIN)).unwrap();
    person(&r.approvals, |_| Decision::Once);
    let out = call(&r, "oso_run", json!({ "host": "box", "command": format!("echo {TRAP_LOGIN} {TRAP_API} {TRAP_TOKEN}") }));
    assert_eq!(out["ok"], true);
    for trap in [TRAP_LOGIN, TRAP_API, TRAP_TOKEN] {
        assert!(!out.to_string().contains(trap), "{trap}");
    }
}

#[test]
fn a_tunnel_can_be_started_with_approval_and_stopped() {
    let sshd = need_sshd!("tunnel");
    let r = rig("tunnel");
    let id = add_server(&r, &sshd, "box", None, AiPolicy::Ask, "");
    let target = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let bind = free_port();
    let f = oso_core::store::Forward { id: 0, name: "acme db".into(), auto: false, kind: oso_core::store::ForwardKind::Local, bind_port: bind, host: "127.0.0.1".into(), port: target.local_addr().unwrap().port() };
    r.engine.lock().unwrap().set_forwards(id, vec![f]).unwrap();
    assert_eq!(call(&r, "oso_tunnel", json!({ "name": "acme db", "action": "start" }))["ok"], false, "nobody to ask: no");
    let seen = person(&r.approvals, |_| Decision::Once);
    let up = call(&r, "oso_tunnel", json!({ "name": "acme db", "action": "start" }));
    assert_eq!(up["result"]["state"], "up", "{up}");
    assert_eq!(seen.lock().unwrap()[0].action, "tunnel");
    let ctx = call(&r, "oso_context", Value::Null);
    assert_eq!(ctx["result"]["servers"][0]["tunnels"][0]["state"], "up");
    assert_eq!(call(&r, "oso_tunnel", json!({ "name": "acme db", "action": "stop" }))["result"]["state"], "off");
    assert!(call(&r, "oso_tunnel", json!({ "name": "nope", "action": "start" }))["error"].as_str().unwrap().contains("no tunnel"));
}

// ---- through the background server, with a real token

struct Chain {
    rig: Rig,
    socket: PathBuf,
    daemon: Daemon,
}

fn chain(name: &str) -> Chain {
    let rig = rig(name);
    let socket = rig.dir.join("d.sock");
    let p = socket.clone();
    std::thread::spawn(move || {
        let _ = daemon::serve(p, false);
    });
    let end = Instant::now() + Duration::from_secs(3);
    while Daemon::connect_at(&socket).is_err() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
    }
    let daemon = Daemon::connect_at(&socket).ok().unwrap();
    Chain { rig, socket, daemon }
}

/// A terminal with a token: it writes its own variables to a file and waits.
fn terminal(c: &Chain, name: &str) -> (u64, String, u64, RemoteHold) {
    terminal_in(c, name, None)
}

/// The same, opened in a Space (the window says so when it creates the terminal; the background server keeps it).
fn terminal_in(c: &Chain, name: &str, space: Option<u64>) -> (u64, String, u64, RemoteHold) {
    let file = c.rig.dir.join(format!("{name}.env"));
    let script = format!("echo \"$OSO_PANE $OSO_TOKEN $OSO_SOCKET\" > {}; sleep 60", file.display());
    let opts = SpawnOpts { argv: vec!["sh".into(), "-c".into(), script], rows: 24, cols: 80, fills: vec![], login_expected: false, cwd: None, env: None, tap: None, sudo_fill: None, agent: true, space };
    let session = c.daemon.spawn(opts).unwrap();
    let end = Instant::now() + Duration::from_secs(5);
    while !file.exists() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(30));
    }
    std::thread::sleep(Duration::from_millis(100));
    let text = std::fs::read_to_string(&file).unwrap();
    let mut it = text.split_whitespace();
    let pane: u64 = it.next().unwrap().parse().unwrap();
    let token = it.next().unwrap().to_string();
    let sock = it.next().unwrap();
    assert_eq!(Path::new(sock), c.socket, "the terminal is told where the server is");
    (pane, token, 0, RemoteHold(session))
}

struct RemoteHold(#[allow(dead_code)] oso_core::daemon::RemoteSession);

fn install(c: &Chain) {
    let h = Arc::new(Handler::new(Arc::clone(&c.rig.engine), Arc::clone(&c.rig.approvals)));
    c.daemon.set_agent_handler(Arc::new(move |call| h.handle(call)));
    std::thread::sleep(Duration::from_millis(150));
}

/// Two Spaces, a server and a secret in each, and one of each that is in neither: the world the isolation tests use.
struct World {
    c: Chain,
    sshd: Sshd,
    a: (u64, u64), // space A: its server and its secret
    b: (u64, u64),
    free_server: u64,
    space_a: u64,
    space_b: u64,
}

const SECRET_A: &str = "SECRET-OF-SPACE-A-111111";
const SECRET_B: &str = "SECRET-OF-SPACE-B-222222";

fn world(name: &str) -> Option<World> {
    let sshd = start_sshd(name)?;
    let c = chain(name);
    let (a, b, free_server, space_a, space_b);
    {
        let r = &c.rig;
        let sa = add_server(r, &sshd, "alpha-api", None, AiPolicy::ReadOnlyAuto, "alpha notes: the api of Alpha");
        let sb = add_server(r, &sshd, "beta-db", None, AiPolicy::ReadOnlyAuto, "beta notes: the db of Beta");
        free_server = add_server(r, &sshd, "free-box", None, AiPolicy::ReadOnlyAuto, "in no Space");
        let meta = |n: &str, var: &str| SecretMeta { id: 0, name: n.into(), kind: SecretKind::ApiKey, profile: None, env_var: Some(var.into()), agent_visible: false, notes: String::new(), updated: 0 };
        let (ka, kb) = {
            let mut e = r.engine.lock().unwrap();
            (e.secret_save(meta("alpha key", "ALPHA_KEY"), Some(SECRET_A)).unwrap(), e.secret_save(meta("beta key", "BETA_KEY"), Some(SECRET_B)).unwrap())
        };
        let mut e = r.engine.lock().unwrap();
        // a tunnel on each Space's server
        for (id, name, port) in [(sa, "alpha tunnel", 15431u16), (sb, "beta tunnel", 15432u16)] {
            let mut s = e.store.server(id).unwrap().clone();
            s.forwards.push(oso_core::store::Forward { id: 1, name: name.into(), auto: false, kind: oso_core::store::ForwardKind::Local, bind_port: port, host: "127.0.0.1".into(), port: 5432 });
            e.store.update_server(s);
        }
        space_a = e.space_save(&Space { name: "Alpha".into(), notes: "Alpha is the shop".into(), folders: vec!["/srv/alpha".into()], servers: vec![sa], secrets: vec![ka], ..Default::default() }).unwrap();
        space_b = e.space_save(&Space { name: "Beta".into(), notes: "Beta is the blog".into(), servers: vec![sb], secrets: vec![kb], ..Default::default() }).unwrap();
        a = (sa, ka);
        b = (sb, kb);
    }
    install(&c);
    Some(World { c, sshd, a, b, free_server, space_a, space_b })
}

/// What an agent in a terminal of `space` gets from `tool`, through the real background server and the real handler.
fn ask(w: &World, term: &(u64, String, u64, RemoteHold), tool: &str, args: Value) -> Value {
    // the agent also tries to name its own Space in the request: it is not listened to
    let body = json!({ "tool": tool, "args": args, "agent": "claude-code", "space": w.space_b });
    daemon::agent_call(&w.c.socket, &term.1, term.0, body, Duration::from_secs(30)).unwrap()
}

fn names_of(v: &Value, key: &str) -> Vec<String> {
    let mut out: Vec<String> = v["result"][key].as_array().map(|a| a.iter().filter_map(|s| s["name"].as_str().map(str::to_string)).collect()).unwrap_or_default();
    out.sort();
    out
}

#[test]
fn an_agent_of_a_space_sees_and_uses_only_what_that_space_holds() {
    let Some(w) = world("isolation") else { return eprintln!("skipped: no sshd here") };
    person(&w.c.rig.approvals, |_| Decision::Once);
    let ta = terminal_in(&w.c, "ta", Some(w.space_a));
    let tb = terminal_in(&w.c, "tb", Some(w.space_b));
    let free = terminal_in(&w.c, "tf", None);

    // what it is told: its Space's servers, notes and variables, and nothing about the other
    let ctx = ask(&w, &ta, "oso_context", Value::Null);
    assert_eq!(ctx["ok"], true, "{ctx}");
    assert_eq!(names_of(&ctx, "servers"), ["alpha-api"], "{ctx}");
    assert_eq!(ctx["result"]["space"]["name"], "Alpha");
    assert!(ctx["result"]["space"]["notes"].as_str().unwrap().contains("the shop"));
    let text = ctx.to_string();
    for outsider in ["beta", "Beta", "free-box", "BETA_KEY", "blog", "15432"] {
        assert!(!text.contains(outsider), "{outsider:?} leaked into the context of Space A: {text}");
    }
    assert_eq!(ctx["result"]["secrets_you_can_use_with_oso_run_with"], json!(["alpha key"]));
    let ctx_b = ask(&w, &tb, "oso_context", Value::Null);
    assert_eq!(names_of(&ctx_b, "servers"), ["beta-db"]);
    assert!(!ctx_b.to_string().contains("alpha") && !ctx_b.to_string().contains("ALPHA_KEY"));
    // a terminal outside any Space sees what the profile shows (as it always did)
    assert_eq!(names_of(&ask(&w, &free, "oso_context", Value::Null), "servers"), ["alpha-api", "beta-db", "free-box"]);

    // searching: a name of another Space finds nothing
    assert_eq!(names_of(&ask(&w, &ta, "oso_hosts", json!({ "query": "beta" })), "hosts"), Vec::<String>::new());
    assert_eq!(names_of(&ask(&w, &ta, "oso_hosts", json!({})), "hosts"), ["alpha-api"]);

    // running: its own server works; another Space's is answered exactly like one that does not exist
    let own = ask(&w, &ta, "oso_run", json!({ "host": "alpha-api", "command": "echo hello-from-alpha" }));
    assert_eq!((own["ok"].as_bool(), stdout_of(&own).trim()), (Some(true), "hello-from-alpha"), "{own}");
    let other = ask(&w, &ta, "oso_run", json!({ "host": "beta-db", "command": "echo hello" }));
    let ghost = ask(&w, &ta, "oso_run", json!({ "host": "nothing-like-it", "command": "echo hello" }));
    let free_try = ask(&w, &ta, "oso_run", json!({ "host": "free-box", "command": "echo hello" }));
    assert_eq!(other["ok"], false);
    assert_eq!(other["error"].as_str().unwrap().replace("beta-db", "X"), ghost["error"].as_str().unwrap().replace("nothing-like-it", "X"), "the agent cannot tell a server of another Space from one that is not there");
    assert!(free_try["error"].as_str().unwrap().contains("no server called"), "{free_try}");
    // by address too, not only by name
    assert_eq!(ask(&w, &ta, "oso_run", json!({ "host": "127.0.0.1", "command": "echo hello" }))["ok"], true, "a host name that is the address of its own server still works");
    let before = audit(&w.c.rig).len();
    assert!(!audit(&w.c.rig).iter().any(|l| l["host"] == "beta-db" && l["decision"] == "finished"), "nothing ran on the other Space's server");
    let _ = before;

    // secrets: its own is handed to a command; another Space's is answered like a missing one, and its value never appears
    let marker = w.c.rig.dir.join("env-a");
    let ok = ask(&w, &ta, "oso_run_with", json!({ "command": format!("echo \"$ALPHA_KEY\" > {}; echo done", marker.display()), "secrets": ["alpha key"] }));
    assert_eq!(ok["ok"], true, "{ok}");
    assert_eq!(std::fs::read_to_string(&marker).unwrap().trim(), SECRET_A, "its own secret reaches the command");
    let steal = ask(&w, &ta, "oso_run_with", json!({ "command": "echo $BETA_KEY", "secrets": ["beta key"] }));
    let none = ask(&w, &ta, "oso_run_with", json!({ "command": "echo x", "secrets": ["no such key"] }));
    assert_eq!(steal["ok"], false);
    assert_eq!(steal["error"].as_str().unwrap().replace("beta key", "X"), none["error"].as_str().unwrap().replace("no such key", "X"));
    assert!(!steal.to_string().contains(SECRET_B) && !ok.to_string().contains(SECRET_A), "values are never in an answer");

    // tunnels: only its own Space's
    let t = ask(&w, &ta, "oso_tunnel", json!({ "name": "beta tunnel", "action": "start" }));
    assert_eq!(t["ok"], false);
    assert!(t["error"].as_str().unwrap().contains("no tunnel called"), "{t}");
    assert!(ask(&w, &ta, "oso_tunnel", json!({ "name": "alpha tunnel", "action": "stop" }))["ok"].as_bool().unwrap(), "its own can be reached");

    // and the other way round
    let from_b = ask(&w, &tb, "oso_run", json!({ "host": "alpha-api", "command": "echo hello" }));
    assert_eq!(from_b["ok"], false);
    assert_eq!(ask(&w, &tb, "oso_run", json!({ "host": "beta-db", "command": "echo from-beta" }))["ok"], true);

    // the request said it was in Space B (the agent tried); it was still treated as A: the Space is the terminal's, not the agent's
    assert_eq!(names_of(&ask(&w, &ta, "oso_context", Value::Null), "servers"), ["alpha-api"]);
    let _ = (&w.sshd, w.free_server, w.a, w.b);
}

#[test]
fn a_space_that_is_gone_or_whose_secret_is_not_listed_closes_everything_for_its_agents() {
    let Some(w) = world("isolation-gone") else { return eprintln!("skipped: no sshd here") };
    person(&w.c.rig.approvals, |_| Decision::Once);
    let ta = terminal_in(&w.c, "ta", Some(w.space_a));
    assert_eq!(ask(&w, &ta, "oso_run", json!({ "host": "alpha-api", "command": "echo hi" }))["ok"], true);
    // taking a server out of the Space takes it away from the agent at once
    {
        let mut e = w.c.rig.engine.lock().unwrap();
        let mut sp = e.spaces.get(w.space_a).unwrap().clone();
        sp.servers.clear();
        e.space_save(&sp).unwrap();
    }
    let r = ask(&w, &ta, "oso_run", json!({ "host": "alpha-api", "command": "echo hi" }));
    assert_eq!(r["ok"], false, "{r}");
    assert!(names_of(&ask(&w, &ta, "oso_context", Value::Null), "servers").is_empty());
    // deleting the Space: nothing at all works, and the answer says why
    w.c.rig.engine.lock().unwrap().space_delete(w.space_a).unwrap();
    for tool in ["oso_context", "oso_hosts", "oso_run", "oso_run_with", "oso_tunnel", "oso_focus", "oso_sudo"] {
        let r = ask(&w, &ta, tool, json!({ "host": "alpha-api", "command": "echo hi", "secrets": ["alpha key"], "name": "x", "action": "stop" }));
        assert_eq!(r["ok"], false, "{tool}: {r}");
        assert!(r["error"].as_str().unwrap().contains("Space no longer exists"), "{tool}: {r}");
    }
}

#[test]
fn the_sudo_password_of_this_computer_is_only_for_a_space_that_lists_it() {
    let Some(w) = world("isolation-sudo") else { return eprintln!("skipped: no sshd here") };
    person(&w.c.rig.approvals, |_| Decision::Deny);
    let ta = terminal_in(&w.c, "ta", Some(w.space_a));
    let r = ask(&w, &ta, "oso_sudo", json!({ "command": "true" }));
    assert_eq!(r["ok"], false);
    assert!(r["error"].as_str().unwrap().contains("does not include the sudo password"), "{r}");
    // once the Space lists it, the agent gets as far as the question (and the person here says no)
    {
        let mut e = w.c.rig.engine.lock().unwrap();
        let sudo = e.vault.links(0).sudo.expect("the rig saved this computer's sudo password");
        let mut sp = e.spaces.get(w.space_a).unwrap().clone();
        sp.secrets.push(sudo);
        e.space_save(&sp).unwrap();
    }
    let r = ask(&w, &ta, "oso_sudo", json!({ "command": "true" }));
    assert!(r["error"].as_str().unwrap().contains("did not allow"), "{r}");
}

#[test]
fn oso_focus_says_what_the_person_looks_at_inside_the_scope_and_nothing_outside_it() {
    let Some(w) = world("focus") else { return eprintln!("skipped: no sshd here") };
    let (alpha, beta, free) = (w.a.0, w.b.0, w.free_server);
    let say = |f: Focus| w.c.rig.engine.lock().unwrap().set_focus(f);
    let ta = terminal_in(&w.c, "ta", Some(w.space_a));
    let plain = terminal_in(&w.c, "tp", None);
    // nothing selected: empty, not an error
    let r = ask(&w, &plain, "oso_focus", Value::Null);
    assert_eq!((r["ok"].as_bool(), r["result"]["selected_server"].is_null(), r["result"]["open_ssh_sessions"].as_array().map(Vec::len)), (Some(true), true, Some(0)), "{r}");
    // the person selected beta-db, has ssh open on alpha-api and free-box, and is on alpha-api's terminal
    say(Focus { selected: Some(beta), open: vec![alpha, free], active: Some(alpha) });
    let r = ask(&w, &plain, "oso_focus", Value::Null);
    assert_eq!(r["result"]["selected_server"], "beta-db");
    assert_eq!(r["result"]["server_of_the_terminal_in_front"], "alpha-api");
    assert_eq!(r["result"]["open_ssh_sessions"], json!(["alpha-api", "free-box"]));
    assert_eq!(r["result"]["profile"], "default");
    assert!(r["result"]["space"].is_null());
    // oso_context carries the same, so that "this server" works from the first call
    assert_eq!(ask(&w, &plain, "oso_context", Value::Null)["result"]["focus"]["selected_server"], "beta-db");
    // an agent of Space A is told only about what is in Space A
    let r = ask(&w, &ta, "oso_focus", Value::Null);
    assert!(r["result"]["selected_server"].is_null(), "beta-db is not in Space A: {r}");
    assert_eq!(r["result"]["open_ssh_sessions"], json!(["alpha-api"]), "{r}");
    assert_eq!(r["result"]["server_of_the_terminal_in_front"], "alpha-api");
    assert_eq!(r["result"]["space"], "Alpha");
    assert!(!r.to_string().contains("beta") && !r.to_string().contains("free-box"), "{r}");
    // never what is on a screen: the answer holds names and nothing else
    assert!(r["result"].as_object().unwrap().keys().all(|k| ["selected_server", "server_of_the_terminal_in_front", "open_ssh_sessions", "profile", "space"].contains(&k.as_str())));
    // another profile is active: what was selected in the old one is not told
    {
        let mut e = w.c.rig.engine.lock().unwrap();
        let work = e.profile_save("", "Work").unwrap();
        let mut s = e.store.server(beta).unwrap().clone();
        s.profile = Some("default".into());
        e.store.update_server(s);
        let mut s = e.store.server(alpha).unwrap().clone();
        s.profile = Some("default".into());
        e.store.update_server(s);
        let mut s = e.store.server(free).unwrap().clone();
        s.profile = Some("default".into());
        e.store.update_server(s);
        e.set_active_profile(&work).unwrap();
    }
    let r = ask(&w, &plain, "oso_focus", Value::Null);
    assert!(r["result"]["selected_server"].is_null() && r["result"]["server_of_the_terminal_in_front"].is_null(), "{r}");
    assert_eq!(r["result"]["open_ssh_sessions"], json!([]), "nothing of the profile that is not active: {r}");
    assert_eq!(r["result"]["profile"], "Work");
    assert!(!r.to_string().contains("beta-db") && !r.to_string().contains("alpha-api"), "{r}");
    assert!(names_of(&ask(&w, &plain, "oso_context", Value::Null), "servers").is_empty(), "and the context of that profile has none of their servers");
}

#[test]
fn what_an_agents_hooks_report_reaches_the_window_only_from_inside_its_terminal() {
    let c = chain("hooks");
    let (pane, token, _, _hold) = terminal(&c, "a");
    install(&c);
    let ask = |token: &str, pane: u64, agent: &str, event: &str| daemon::agent_call(&c.socket, token, pane, json!({ "tool": "oso_event", "args": { "agent": agent, "event": event } }), Duration::from_secs(10)).unwrap();
    let r = ask(&token, pane, "claude", "Stop");
    assert_eq!((r["ok"].as_bool(), r["result"]["known"].as_bool()), (Some(true), Some(true)), "{r}");
    assert_eq!(ask(&token, pane, "opencode", "session.idle")["result"]["known"], true);
    assert_eq!(ask(&token, pane, "claude", "SomethingNew")["result"]["known"], false, "an event oso does not know is not an error");
    assert_eq!(ask(&token, pane, "codex", "Stop")["result"]["known"], false);
    // a token that is not that terminal's is refused like any other request
    let r = ask("0000", pane, "claude", "Stop");
    assert_eq!(r["ok"], false, "{r}");
    // and nothing of it is in the audit log: it is a signal, not something an agent asked for
    assert!(!audit(&c.rig).iter().any(|l| l["action"] == "oso_event"));
}

#[test]
fn a_request_from_inside_a_terminal_reaches_the_window_and_one_from_outside_does_not() {
    let c = chain("chain");
    let (pane, token, _, _hold) = terminal(&c, "a");
    // no window is answering yet
    let r = daemon::agent_call(&c.socket, &token, pane, json!({ "tool": "oso_context", "args": null }), Duration::from_secs(10)).unwrap();
    assert_eq!(r["ok"], false);
    assert!(r["error"].as_str().unwrap().contains("window is not open"), "{r}");
    install(&c);
    let r = daemon::agent_call(&c.socket, &token, pane, json!({ "tool": "oso_context", "args": null, "agent": "claude-code" }), Duration::from_secs(10)).unwrap();
    assert_eq!(r["ok"], true, "{r}");
    assert!(!r.to_string().contains("TRAP"));
    // the rest is refused with the same words, whatever is wrong with it
    for (tok, pane) in [("0".repeat(64), pane), (String::new(), pane), (token.clone(), pane + 50), ("x".into(), 0)] {
        let r = daemon::agent_call(&c.socket, &tok, pane, json!({ "tool": "oso_context", "args": null }), Duration::from_secs(10)).unwrap();
        assert_eq!(r["ok"], false);
        assert!(r["error"].as_str().unwrap().contains("not inside an oso terminal"), "{r}");
    }
    // another terminal's token does not work for this pane
    let (pane2, token2, _, _hold2) = terminal(&c, "b");
    assert_ne!(token, token2);
    let r = daemon::agent_call(&c.socket, &token2, pane, json!({ "tool": "oso_context", "args": null }), Duration::from_secs(10)).unwrap();
    assert_eq!(r["ok"], false, "the token of {pane2} is not the token of {pane}");
}

#[test]
fn a_token_stops_working_when_its_terminal_closes() {
    let c = chain("closed");
    install(&c);
    let (pane, token, _, hold) = terminal(&c, "a");
    let ask = || daemon::agent_call(&c.socket, &token, pane, json!({ "tool": "oso_context", "args": null }), Duration::from_secs(10)).unwrap();
    assert_eq!(ask()["ok"], true);
    let mut hold = hold;
    hold.0.kill();
    std::thread::sleep(Duration::from_millis(400));
    let r = ask();
    assert_eq!(r["ok"], false);
    assert!(r["error"].as_str().unwrap().contains("not inside an oso terminal"), "{r}");
}

#[test]
fn the_whole_way_from_the_agents_tool_call_to_a_command_on_a_server() {
    let sshd = need_sshd!("whole");
    let c = chain("whole");
    add_server(&c.rig, &sshd, "acme-api", Some(EnvKind::Production), AiPolicy::Ask, "nginx");
    install(&c);
    let seen = person(&c.rig.approvals, |_| Decision::Once);
    let (pane, token, _, _hold) = terminal(&c, "a");
    let backend = mcp::Daemon::new(c.socket.clone(), token, pane);
    // the MCP server, as the agent would talk to it over stdio
    let input = [
        json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18", "clientInfo": { "name": "claude-code" } } }),
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "oso_run", "arguments": { "host": "acme-api", "command": "echo from-the-server" } } }),
        json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "oso_context", "arguments": {} } }),
    ]
    .iter()
    .map(|l| format!("{l}\n"))
    .collect::<String>();
    let mut out = Vec::new();
    mcp::serve(input.as_bytes(), &mut out, &backend).unwrap();
    let lines: Vec<Value> = String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let instructions = lines[0]["result"]["instructions"].as_str().unwrap();
    assert!(instructions.contains("acme-api (production): nginx"), "{instructions}");
    assert_eq!(lines[1]["result"]["isError"], false, "{}", lines[1]);
    assert!(lines[1]["result"]["content"][0]["text"].as_str().unwrap().contains("from-the-server"));
    let q = seen.lock().unwrap()[0].clone();
    assert_eq!(q.label, format!("Claude in terminal, pane {pane}"), "who asks and from which terminal");
    assert!(q.production && q.command == "echo from-the-server");
    // nothing the agent received holds a trap value
    for l in &lines {
        for trap in [TRAP_API, TRAP_TOKEN, TRAP_SUDO, TRAP_LOGIN] {
            assert!(!l.to_string().contains(trap), "{trap} in {l}");
        }
    }
}

#[test]
fn an_agent_waiting_for_a_window_that_closes_is_told_so() {
    use std::io::{BufRead, BufReader, Write};
    let c = chain("gone");
    let (pane, token, _, _hold) = terminal(&c, "a");
    // a window, played by hand: it takes the request and then disappears without answering
    let mut window = std::os::unix::net::UnixStream::connect(&c.socket).unwrap();
    writeln!(window, "{}", json!({ "t": "agent_handler" })).unwrap();
    std::thread::sleep(Duration::from_millis(150));
    let (socket, tok) = (c.socket.clone(), token.clone());
    let asking = std::thread::spawn(move || daemon::agent_call(&socket, &tok, pane, json!({ "tool": "oso_context", "args": null }), Duration::from_secs(20)).unwrap());
    let mut line = String::new();
    BufReader::new(window.try_clone().unwrap()).read_line(&mut line).unwrap();
    let ev: Value = serde_json::from_str(&line).unwrap();
    assert_eq!((ev["t"].as_str(), ev["sid"].as_u64(), ev["body"]["tool"].as_str()), (Some("agent_call"), Some(pane), Some("oso_context")));
    drop(window);
    let r = asking.join().unwrap();
    assert_eq!(r["ok"], false);
    assert!(r["error"].as_str().unwrap().contains("window was closed"), "{r}");
}
