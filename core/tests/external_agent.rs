//! An agent that was NOT started in a terminal of oso (Claude from herdr, OpenCode, Hermes…): the real `oso mcp`
//! program, with no `OSO_TOKEN`, talking to the running app over its private socket. Skipped (and says so) without
//! `sshd` or without a built `oso` (`cargo build -p oso`).

mod common;

use common::*;
use oso_core::agent_api::Handler;
use oso_core::approvals::{ApprovalRequest, Approvals, Decision};
use oso_core::control::{self, Api};
use oso_core::engine::{Emit, Engine};
use oso_core::store::{Auth, Server, Store};
use oso_core::vault::Vault;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

const TRAP_LOGIN: &str = "TRAP-EXT-LOGIN-3d51ab";

fn user() -> String {
    String::from_utf8(Command::new("id").arg("-un").output().unwrap().stdout).unwrap().trim().to_string()
}

#[test]
fn an_agent_from_outside_oso_uses_oso_like_any_other_and_still_asks_for_what_is_not_a_read() {
    let Some(sshd) = start_sshd("external") else {
        eprintln!("skipped: no sshd here");
        return;
    };
    let oso = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/debug/oso");
    if !oso.exists() {
        eprintln!("skipped: build the app first (cargo build -p oso)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("oso-ext-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let emit: Emit = Arc::new(|_, _| {});
    let store = Store::load_from(dir.join("servers.json")).unwrap();
    let mut e = Engine::new(store, Vault::new(dir.join("vault.json")), emit);
    e.vault_unlock("master").unwrap();
    e.ssh_extra = vec![];
    e.store.extra_options = ["-o", "UserKnownHostsFile=/dev/null", "-o", "StrictHostKeyChecking=no", "-o", "LogLevel=ERROR"].iter().map(|s| s.to_string()).collect();
    e.save_server(
        Server { name: "box".into(), host: "127.0.0.1".into(), port: sshd.port, user: user(), auth: Auth::Key, key_path: sshd.dir.join("client").to_string_lossy().into(), notes: "the test box".into(), ..Default::default() },
        Some(TRAP_LOGIN),
        None,
    )
    .unwrap();
    let engine = Arc::new(Mutex::new(e));
    let approvals = Approvals::new();
    // the person at the window: says yes, and remembers what was shown
    let seen: Arc<Mutex<Vec<ApprovalRequest>>> = Arc::default();
    {
        let (s, a) = (Arc::clone(&seen), Arc::clone(&approvals));
        approvals.on_request(Arc::new(move |r| {
            s.lock().unwrap().push(r.clone());
            let a = Arc::clone(&a);
            std::thread::spawn(move || {
                a.answer(r.id, Decision::Once);
            });
        }));
    }
    let handler = Arc::new(Handler::new(Arc::clone(&engine), Arc::clone(&approvals)));
    let api = Arc::new(Api { engine: Arc::clone(&engine), approvals: Arc::clone(&approvals), ui: Arc::new(|_, _| {}) });
    let h = Arc::clone(&handler);
    let _control = control::start(
        dir.join("control.sock"),
        Arc::new(move |req| if req.get("cmd").and_then(Value::as_str) == Some("agent") { h.handle_external(&req) } else { api.handle(req) }),
    )
    .unwrap();

    // the agent's side: `oso mcp` with none of oso's variables, as herdr would start it
    let mut child = Command::new(&oso)
        .arg("mcp")
        .env_remove("OSO_TOKEN")
        .env_remove("OSO_SOCKET")
        .env_remove("OSO_PANE")
        .env_remove("OSO")
        .env("OSO_CONFIG_DIR", &dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut rpc = |id: u64, method: &str, params: Value| -> Value {
        writeln!(stdin, "{}", json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })).unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        out.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    };
    let init = rpc(1, "initialize", json!({ "protocolVersion": "2025-06-18", "clientInfo": { "name": "claude-code" } }));
    let text = init["result"]["instructions"].as_str().unwrap();
    assert!(text.contains("box") && !text.contains("not inside oso"), "{text}");
    let mut tool = |id: u64, name: &str, args: Value| rpc(id, "tools/call", json!({ "name": name, "arguments": args }));
    // the context lists the server and never a secret
    let ctx = tool(2, "oso_context", json!({}));
    let ctx_text = ctx["result"]["content"][0]["text"].as_str().unwrap();
    assert!(ctx_text.contains("the test box") && !ctx_text.contains(TRAP_LOGIN), "{ctx_text}");
    // a plain read goes through without asking (the rule for a server that is not production)
    let run = tool(3, "oso_run", json!({ "host": "box", "command": "cd /tmp && echo hello-from-outside" }));
    assert_ne!(run["result"]["isError"], true, "{run}");
    assert!(run["result"]["content"][0]["text"].as_str().unwrap().contains("hello-from-outside"), "{run}");
    assert!(seen.lock().unwrap().is_empty(), "a read is not asked about");
    // a command that changes something asks, the label says it is outside oso, and it runs once allowed
    let mark = std::env::temp_dir().join(format!("oso-ext-mark-{}", std::process::id()));
    let write = tool(4, "oso_run", json!({ "host": "box", "command": format!("touch {}", mark.display()) }));
    assert_ne!(write["result"]["isError"], true, "{write}");
    let shown = seen.lock().unwrap().clone();
    assert_eq!(shown.len(), 1, "the person was asked once, for the write");
    assert!(shown[0].label.contains("outside oso") && shown[0].command.contains("touch"), "{:?}", shown[0].label);
    assert!(mark.exists(), "the allowed command ran");
    let _ = std::fs::remove_file(&mark);
    assert!(!run.to_string().contains(TRAP_LOGIN) && !write.to_string().contains(TRAP_LOGIN));
    drop(stdin);
    let _ = child.wait();
}
