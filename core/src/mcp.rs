//! `oso mcp`: a Model Context Protocol server over stdio, for any AI agent that speaks MCP (Claude Code, OpenCode,
//! Codex…). It holds nothing: every tool is a request to oso's window through the background server, from inside
//! the terminal the agent runs in (the `OSO_TOKEN` of that terminal says which one). Outside oso it has no access.
//!
//! The descriptions of the tools are the manual the model reads: they say what each is for and when to use it.

use serde_json::{Value, json};
use std::io::{BufRead, Write};

/// What the tools are answered with.
pub trait Backend {
    /// Runs a tool: the answer is `{"ok": true, "result": …}` or `{"ok": false, "error": "…"}`.
    fn call(&self, tool: &str, args: Value, agent: &str) -> Value;
}

/// The protocol versions this server can speak (the newest the client asks for that it knows).
const VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

pub fn tools() -> Value {
    let host = json!({ "type": "string", "description": "Name of the server as oso lists it (see oso_context or oso_hosts), or its host name." });
    json!([
        {
            "name": "oso_context",
            "description": "Call this first, before working with servers, credentials or secrets. It tells you where you are (a Space, or General), every server with its name, user, whether it is production/staging/dev and the notes its owner wrote about it, the tunnels and whether they are on, the names (never the values) of the environment variables already set in your terminal, and the secrets you can use through oso_run_with. Never ask the user for a password or a key: oso holds them and uses them for you.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false },
        },
        {
            "name": "oso_hosts",
            "description": "Search the servers you may use by name, host, user or notes. Use it when oso_context says there are too many servers to list.",
            "inputSchema": { "type": "object", "properties": { "query": { "type": "string", "description": "Words to look for; empty lists the first ones." } }, "additionalProperties": false },
        },
        {
            "name": "oso_run",
            "description": "Run one command on a server through ssh, with no terminal, and get its stdout, stderr and exit code (each cut at 64 KB). oso logs in with the saved credentials for you. The user is asked to approve it, seeing the exact command, unless the server was set to let read-only commands through and this is one; production servers always ask. Pass the whole command line as one string. Prefer commands that only read. If the user says no, do not retry the same command. A command that uses sudo runs with no terminal, so a sudo that asks for a password cannot be answered; when a command uses sudo the answer carries a `sudo_note` saying whether a sudo password is saved for that server. Never hide a command's errors (2>/dev/null) when you need to know why it failed.",
            "inputSchema": { "type": "object", "properties": { "host": host, "command": { "type": "string", "description": "The command line, run by the remote shell." }, "timeout_seconds": { "type": "integer", "minimum": 1, "maximum": 300, "description": "Default 60." } }, "required": ["host", "command"], "additionalProperties": false },
        },
        {
            "name": "oso_sudo",
            "description": "Run a command with sudo on THIS computer. oso types the user's sudo password; you never see it. The user is asked to approve the exact command each time. It is not for remote servers: a command with sudo on a server goes through oso_run, which does not type a sudo password (read its `sudo_note`).",
            "inputSchema": { "type": "object", "properties": { "command": { "type": "string", "description": "The command, without the word sudo; it runs as `sudo sh -c`." }, "timeout_seconds": { "type": "integer", "minimum": 1, "maximum": 300 } }, "required": ["command"], "additionalProperties": false },
        },
        {
            "name": "oso_run_with",
            "description": "Run a command on THIS computer with some secrets put in its environment, for secrets that are not already in your terminal (oso_context lists them under secrets_you_can_use_with_oso_run_with). The command reads them as environment variables; their values are blanked out of what you get back. The user is asked to approve it each time.",
            "inputSchema": { "type": "object", "properties": { "command": { "type": "string" }, "secrets": { "type": "array", "items": { "type": "string" }, "description": "Names of the secrets, as oso_context lists them." }, "timeout_seconds": { "type": "integer", "minimum": 1, "maximum": 300 } }, "required": ["command", "secrets"], "additionalProperties": false },
        },
        {
            "name": "oso_put",
            "description": "Copy a file or folder from THIS computer to a server, disk to disk: the content never passes through you, so never paste a file's content into oso_run to copy it. `local_path` must be inside your working folder (or your scratch folder); `remote_path` is the full path the file will have on the server (for a folder, the path of the new folder, which must not exist yet). Nothing on the server is replaced unless `overwrite` is true (a file only). Pass `sha256` (of the file here) and oso checks it before sending and again on the server. The user is asked to approve it each time, seeing every file with its size and the destination; production and `blocked` rules apply. At most 1 GB and 2000 files. Never send keys or anything from ~/.ssh: oso refuses it.",
            "inputSchema": { "type": "object", "properties": { "host": host, "local_path": { "type": "string", "description": "A file or folder here, absolute or relative to your working folder." }, "remote_path": { "type": "string", "description": "Where it goes on the server (a plain path: no quotes, $, ; | & * ? and the like)." }, "sha256": { "type": "string", "description": "Optional. The expected sha256 (hex) of the file; verified before sending and on the server after. One file only." }, "overwrite": { "type": "boolean", "description": "Replace an existing file on the server. Default false." }, "timeout_seconds": { "type": "integer", "minimum": 1, "maximum": 3600, "description": "Default 600." } }, "required": ["host", "local_path", "remote_path"], "additionalProperties": false },
        },
        {
            "name": "oso_get",
            "description": "Copy a file or folder from a server to THIS computer, disk to disk: the content never passes through you. `local_path` is the full path it will have here, inside your working folder (or your scratch folder); it arrives under a temporary name and takes its place only when it is complete (and, if you pass `sha256`, only when the hash matches). An existing file is replaced only with `overwrite` true. The user is asked to approve it each time; the sizes are read from the server after they allow it, and more than 1 GB or 2000 files is refused. To read a small file's text, oso_run with cat is still the way.",
            "inputSchema": { "type": "object", "properties": { "host": host, "remote_path": { "type": "string", "description": "A file or folder on the server (a plain path)." }, "local_path": { "type": "string", "description": "Where it goes here, absolute or relative to your working folder." }, "sha256": { "type": "string", "description": "Optional. The expected sha256 (hex) of the file; checked before it is put in place. One file only." }, "overwrite": { "type": "boolean", "description": "Replace an existing local file. Default false." }, "timeout_seconds": { "type": "integer", "minimum": 1, "maximum": 3600, "description": "Default 600." } }, "required": ["host", "remote_path", "local_path"], "additionalProperties": false },
        },
        {
            "name": "oso_kubectl",
            "description": "Run kubectl on one of the Kubernetes clusters the user saved in oso (oso_context lists them under kubernetes_clusters). oso picks the kubeconfig, context and login: never pass --kubeconfig, --context, --server, --token or --as, and never ask for a kubeconfig. `args` are the words after kubectl, as a list. Plain reads (get, describe, logs, top, ...) can run without asking depending on the cluster's rule; anything else (apply, delete, scale, exec, rollout ...) asks the user each time, and a production cluster always asks. `get secrets` always asks. Output is cut at a limit: narrow it with -n, -l or --tail.",
            "inputSchema": { "type": "object", "properties": { "cluster": { "type": "string", "description": "The cluster's name." }, "args": { "type": "array", "items": { "type": "string" }, "description": "The words after kubectl, e.g. [\"get\", \"pods\", \"-n\", \"web\"]. No shell: each item is one argument." }, "timeout_seconds": { "type": "integer", "minimum": 1, "maximum": 300 } }, "required": ["cluster", "args"], "additionalProperties": false },
        },
        {
            "name": "oso_focus",
            "description": "What the user is looking at in oso right now: the server they have selected, the server of the terminal in front, and the servers they have an SSH session open on (names only, never what is on a screen), limited to what you may use. Call it when the user says \"this server\", \"the one I am on\" or \"the one I selected\". Empty answers mean nothing is selected.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false },
        },
        {
            "name": "oso_tunnel",
            "description": "Start or stop one of the saved tunnels (a port forward through a server, listed by oso_context). Starting it asks the user; stopping does not.",
            "inputSchema": { "type": "object", "properties": { "name": { "type": "string", "description": "The tunnel's name." }, "action": { "type": "string", "enum": ["start", "stop"] } }, "required": ["name", "action"], "additionalProperties": false },
        },
    ])
}

/// What an agent is told when it connects, from what `oso_context` answered (or why it could not).
pub fn instructions(context: &Value) -> String {
    let mut out = String::from("oso keeps this user's servers, credentials and secrets, and uses them for you: never ask for a password or a key. Call oso_context before working with servers or secrets.");
    if context.get("ok") != Some(&Value::Bool(true)) {
        let why = context.get("error").and_then(Value::as_str).unwrap_or("it is not reachable");
        out.push_str(&format!("\n\nRight now: {why}"));
        return out;
    }
    let r = &context["result"];
    let space = r["space"]["name"].as_str().unwrap_or("General");
    out.push_str(&format!("\n\nYou are in: {space}."));
    if let Some(notes) = r["space"]["notes"].as_str().filter(|n| !n.trim().is_empty()) {
        out.push_str(&format!(" {}", notes.trim()));
    }
    let servers = r["servers"].as_array().cloned().unwrap_or_default();
    if r["servers_note"].as_str().is_some_and(|n| !n.is_empty()) {
        out.push_str(&format!(" There are {} servers: use oso_hosts to search them.", servers.len()));
    } else if !servers.is_empty() {
        out.push_str("\nServers:");
        for s in &servers {
            let notes = s["notes"].as_str().unwrap_or("");
            out.push_str(&format!("\n- {} ({}){}", s["name"].as_str().unwrap_or("?"), s["environment"].as_str().unwrap_or("unspecified"), if notes.is_empty() { String::new() } else { format!(": {notes}") }));
        }
    }
    out
}

fn reply(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// Reads requests (one JSON per line) and answers on `output` until the input ends.
pub fn serve(input: impl BufRead, mut output: impl Write, backend: &dyn Backend) -> std::io::Result<()> {
    let mut agent = String::new();
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(m) => m,
            Err(_) => {
                writeln!(output, "{}", error(&Value::Null, -32700, "not JSON"))?;
                output.flush()?;
                continue;
            }
        };
        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        // a message with no id is a notification: it is never answered
        let Some(id) = id else { continue };
        let answer = match method {
            "initialize" => {
                agent = params["clientInfo"]["name"].as_str().unwrap_or("").chars().take(60).collect();
                let asked = params["protocolVersion"].as_str().unwrap_or("");
                let version = VERSIONS.iter().find(|v| **v == asked).unwrap_or(&VERSIONS[0]);
                let ctx = backend.call("oso_context", Value::Null, &agent);
                reply(&id, json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": { "name": "oso", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": instructions(&ctx),
                }))
            }
            "ping" => reply(&id, json!({})),
            "tools/list" => reply(&id, json!({ "tools": tools() })),
            "tools/call" => {
                let name = params["name"].as_str().unwrap_or("");
                if !tools().as_array().is_some_and(|t| t.iter().any(|x| x["name"] == name)) {
                    error(&id, -32602, &format!("unknown tool {name:?}"))
                } else {
                    let out = backend.call(name, params.get("arguments").cloned().unwrap_or(Value::Null), &agent);
                    let (text, is_error) = match out.get("ok") {
                        Some(Value::Bool(true)) => (serde_json::to_string_pretty(&out["result"]).unwrap_or_default(), false),
                        _ => (out["error"].as_str().unwrap_or("oso did not answer").to_string(), true),
                    };
                    reply(&id, json!({ "content": [{ "type": "text", "text": text }], "isError": is_error }))
                }
            }
            other => error(&id, -32601, &format!("method {other:?} is not supported")),
        };
        writeln!(output, "{answer}")?;
        output.flush()?;
    }
    Ok(())
}

/// The real backend: asks the background server, as the terminal this runs in.
pub struct Daemon {
    socket: std::path::PathBuf,
    token: String,
    pane: u64,
}

impl Daemon {
    pub fn new(socket: std::path::PathBuf, token: String, pane: u64) -> Daemon {
        Daemon { socket, token, pane }
    }

    /// From the environment oso gave the terminal; `Err` says in words that this is not inside oso.
    pub fn from_env() -> Result<Daemon, String> {
        let token = std::env::var("OSO_TOKEN").ok().filter(|t| !t.is_empty());
        let socket = std::env::var_os("OSO_SOCKET").filter(|s| !s.is_empty());
        let pane = std::env::var("OSO_PANE").ok().and_then(|p| p.parse::<u64>().ok());
        match (token, socket, pane) {
            (Some(token), Some(socket), Some(pane)) => Ok(Daemon { socket: socket.into(), token, pane }),
            _ => Err("You are not inside oso: this agent was not started from an oso terminal (OSO_TOKEN is missing). Open a terminal in oso and start the agent there.".into()),
        }
    }
}

impl Daemon {
    /// A request that is not worth waiting for as long as the person may take (what an agent's hooks send).
    pub fn call_within(&self, tool: &str, args: Value, agent: &str, wait: std::time::Duration) -> Value {
        let body = json!({ "tool": tool, "args": args, "agent": agent });
        match crate::daemon::agent_call(&self.socket, &self.token, self.pane, body, wait) {
            Ok(v) => v,
            Err(e) => json!({ "ok": false, "error": format!("{e:#}") }),
        }
    }
}

impl Backend for Daemon {
    fn call(&self, tool: &str, mut args: Value, agent: &str) -> Value {
        // the folder the agent works in decides where a transfer may read and write: it comes from this process, never from the
        // agent's own words (anything it put under that name is dropped)
        if matches!(tool, "oso_put" | "oso_get") {
            if let Some(o) = args.as_object_mut() {
                o.remove("_cwd");
                if let Ok(cwd) = std::env::current_dir() {
                    o.insert("_cwd".into(), json!(cwd.to_string_lossy()));
                }
            }
        }
        self.call_within(tool, args, agent, std::time::Duration::from_secs(900))
    }
}

/// `oso hook <agent> <event>`: what an agent's own hook runs, to tell oso what it is doing. It prints nothing (what a hook
/// prints can end up in the agent's context), never fails (a hook that fails can stop an agent) and waits at most 3 s.
/// Outside a terminal of oso it does nothing.
pub fn hook(agent: &str, event: &str) {
    if let Ok(d) = Daemon::from_env() {
        let _ = d.call_within("oso_event", json!({ "agent": agent, "event": event }), agent, std::time::Duration::from_secs(3));
    }
}

/// What answers when there is no oso around.
pub struct Outside(pub String);

impl Backend for Outside {
    fn call(&self, _tool: &str, _args: Value, _agent: &str) -> Value {
        json!({ "ok": false, "error": self.0 })
    }
}

/// `oso mcp`.
pub fn run() -> std::io::Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    match Daemon::from_env() {
        Ok(d) => serve(stdin.lock(), stdout.lock(), &d),
        Err(why) => serve(stdin.lock(), stdout.lock(), &Outside(why)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct Fake(Mutex<Vec<(String, Value, String)>>);

    impl Backend for Fake {
        fn call(&self, tool: &str, args: Value, agent: &str) -> Value {
            self.0.lock().unwrap().push((tool.into(), args, agent.into()));
            match tool {
                "oso_context" => json!({ "ok": true, "result": { "space": { "name": "Work", "notes": "" }, "servers": [{ "name": "example-api", "environment": "production", "notes": "nginx + API node" }, { "name": "dev-box", "environment": "dev", "notes": "" }], "servers_note": "" } }),
                "oso_run" => json!({ "ok": true, "result": { "exit_code": 0, "stdout": "hi\n" } }),
                _ => json!({ "ok": false, "error": "The user did not allow this command." }),
            }
        }
    }

    fn talk(lines: &[Value]) -> (Vec<Value>, Vec<(String, Value, String)>) {
        let fake = Fake(Mutex::new(vec![]));
        let input: String = lines.iter().map(|l| format!("{l}\n")).collect();
        let mut out = Vec::new();
        serve(input.as_bytes(), &mut out, &fake).unwrap();
        let answers = String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        let calls = fake.0.lock().unwrap().clone();
        (answers, calls)
    }

    #[test]
    fn it_introduces_itself_with_a_summary_of_the_space_and_its_servers() {
        let (a, calls) = talk(&[json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": { "name": "claude-code", "version": "2" } } }), json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })]);
        assert_eq!(a.len(), 1, "a notification is not answered");
        let r = &a[0]["result"];
        assert_eq!(r["protocolVersion"], "2025-03-26");
        assert_eq!(r["serverInfo"]["name"], "oso");
        assert!(r["capabilities"]["tools"].is_object());
        let text = r["instructions"].as_str().unwrap();
        assert!(text.contains("You are in: Work") && text.contains("example-api (production): nginx + API node") && text.contains("dev-box (dev)"), "{text}");
        assert!(text.contains("never ask for a password"));
        assert_eq!(calls[0].0, "oso_context");
        assert_eq!(calls[0].2, "claude-code");
    }

    #[test]
    fn an_unknown_version_gets_ours_and_a_big_server_list_is_not_dumped() {
        let (a, _) = talk(&[json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "1999-01-01" } })]);
        assert_eq!(a[0]["result"]["protocolVersion"], "2025-06-18");
        let many = json!({ "ok": true, "result": { "space": { "name": "Big" }, "servers": [{ "name": "a" }, { "name": "b" }], "servers_note": "There are many servers" } });
        let t = instructions(&many);
        assert!(t.contains("use oso_hosts") && !t.contains("- a"), "{t}");
        assert!(instructions(&json!({ "ok": false, "error": "oso's window is not open" })).contains("window is not open"));
    }

    #[test]
    fn the_tools_are_listed_with_descriptions_that_explain_when_to_use_them() {
        let (a, _) = talk(&[json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" })]);
        let tools = a[0]["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(names, vec!["oso_context", "oso_hosts", "oso_run", "oso_sudo", "oso_run_with", "oso_put", "oso_get", "oso_kubectl", "oso_focus", "oso_tunnel"]);
        assert!(tools[0]["description"].as_str().unwrap().starts_with("Call this first"));
        assert!(tools[2]["description"].as_str().unwrap().contains("asked to approve"));
        for t in tools {
            assert_eq!(t["inputSchema"]["type"], "object");
        }
        assert_eq!(tools[2]["inputSchema"]["required"], json!(["host", "command"]));
    }

    #[test]
    fn a_call_goes_to_oso_and_its_answer_comes_back_as_text_or_as_an_error() {
        let (a, calls) = talk(&[
            json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "clientInfo": { "name": "opencode" } } }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "oso_run", "arguments": { "host": "example-api", "command": "echo hi" } } }),
            json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "oso_sudo", "arguments": { "command": "reboot" } } }),
            json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": { "name": "rm_rf", "arguments": {} } }),
            json!({ "jsonrpc": "2.0", "id": 5, "method": "nope" }),
            json!({ "jsonrpc": "2.0", "id": 6, "method": "ping" }),
        ]);
        assert_eq!(a[1]["result"]["isError"], false);
        assert!(a[1]["result"]["content"][0]["text"].as_str().unwrap().contains("\"stdout\": \"hi\\n\""));
        assert_eq!(a[2]["result"]["isError"], true);
        assert_eq!(a[2]["result"]["content"][0]["text"], "The user did not allow this command.");
        assert_eq!(a[3]["error"]["code"], -32602, "an unknown tool is not forwarded");
        assert_eq!(a[4]["error"]["code"], -32601);
        assert_eq!(a[5]["result"], json!({}));
        let run = calls.iter().find(|c| c.0 == "oso_run").unwrap();
        assert_eq!((run.1["host"].as_str(), run.2.as_str()), (Some("example-api"), "opencode"), "the agent's name travels with the call");
        assert!(!calls.iter().any(|c| c.0 == "rm_rf"));
    }

    #[test]
    fn garbage_is_answered_and_the_server_keeps_going() {
        let fake = Fake(Mutex::new(vec![]));
        let mut out = Vec::new();
        serve(&b"not json\n\n{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"ping\"}\n"[..], &mut out, &fake).unwrap();
        let lines: Vec<Value> = String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(lines[0]["error"]["code"], -32700);
        assert_eq!(lines[1]["id"], 7);
    }

    #[test]
    fn outside_oso_every_tool_says_so_clearly() {
        // this test process has no OSO_TOKEN
        if std::env::var_os("OSO_TOKEN").is_some() {
            return;
        }
        let why = Daemon::from_env().err().expect("not inside oso");
        assert!(why.contains("not inside oso") && why.contains("OSO_TOKEN"), "{why}");
        let mut out = Vec::new();
        let input = format!("{}\n{}\n", json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} }), json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "oso_run", "arguments": { "host": "x", "command": "ls" } } }));
        serve(input.as_bytes(), &mut out, &Outside(why)).unwrap();
        let lines: Vec<Value> = String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert!(lines[0]["result"]["instructions"].as_str().unwrap().contains("not inside oso"));
        assert_eq!(lines[1]["result"]["isError"], true);
        assert!(lines[1]["result"]["content"][0]["text"].as_str().unwrap().contains("not inside oso"));
    }
}
