// oso: the window. Everything it does is in `oso-core` (servers, vault, SSH sessions, the background server);
// this file only connects those to the interface.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use oso_core::daemon::Stream;
use oso_core::engine::{Emit, Engine, ImportRequest, ImportResult, OpenError, SessionInfo, TransferStatus, TunnelStatus, VaultStatus};
use oso_core::store::{Forward, NodeId, Proxy, Server, Store};
use oso_core::vault::{SecretMeta, Vault};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

struct AppState {
    /// What AI agents asked and the person has not answered yet.
    approvals: Arc<oso_core::approvals::Approvals>,
    /// Shared with oso's ssh-agent, which signs with the keys of the vault while it is unlocked.
    engine: Arc<Mutex<Engine>>,
}

/// Keeps the command line's socket open as long as the app runs.
struct ControlHolder(#[allow(dead_code)] Option<oso_core::control::Control>);

/// Keeps the agent alive as long as the app runs (dropping it removes its socket).
struct AgentHolder(#[allow(dead_code)] Option<oso_core::agent::Agent>);

type Res<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    format!("{e:#}")
}

/// What the window hears about a session. Bytes travel as base64.
fn payload(sid: u64, ev: Stream) -> Value {
    match ev {
        Stream::Snapshot { replay, screen, rows, cols, exit } => json!({
            "sid": sid, "kind": "snapshot", "replay": B64.encode(replay), "screen": B64.encode(screen),
            "rows": rows, "cols": cols, "exit": exit,
        }),
        Stream::Output(b) => json!({ "sid": sid, "kind": "output", "data": B64.encode(b) }),
        Stream::Exit(code) => json!({ "sid": sid, "kind": "exit", "code": code }),
        Stream::Sudo(on) => json!({ "sid": sid, "kind": "sudo", "on": on }),
        Stream::Lost => json!({ "sid": sid, "kind": "lost" }),
    }
}

fn node(kind: &str, id: u64) -> Res<NodeId> {
    match kind {
        "folder" => Ok(NodeId::Folder(id)),
        "server" => Ok(NodeId::Server(id)),
        _ => Err("unknown kind".into()),
    }
}

fn open_err(e: OpenError) -> String {
    match e {
        OpenError::VaultLocked => "vault_locked".into(),
        OpenError::NoSuchServer => "This server no longer exists".into(),
        OpenError::Network { wanted, current } => format!("network:{wanted}|{current}"),
        OpenError::Other(m) => m,
    }
}

/// Only the files the window itself keeps may be read or written through the window.
fn config_path(name: &str) -> Res<std::path::PathBuf> {
    match name {
        "workspace.json" | "hotkeys.json" | "snippets.json" | "groups.json" | "selftest.flag" | "selftest.out" => Ok(Store::config_dir().join(name)),
        _ => Err("not allowed".into()),
    }
}

macro_rules! engine {
    ($state:expr) => {
        $state.engine.lock().map_err(|_| "internal error: the engine is stuck".to_string())?
    };
}

#[tauri::command]
fn tree(state: State<AppState>) -> Res<Value> {
    Ok(engine!(state).tree())
}

#[tauri::command]
fn vault_status(state: State<AppState>) -> Res<VaultStatus> {
    Ok(engine!(state).vault_status())
}

#[tauri::command]
fn vault_unlock(state: State<AppState>, master: String) -> Res<()> {
    engine!(state).vault_unlock(&master)
}

#[tauri::command]
fn vault_lock(state: State<AppState>) -> Res<()> {
    engine!(state).vault_lock();
    Ok(())
}

#[tauri::command]
fn vault_reveal(state: State<AppState>, server_id: u64, sudo: bool) -> Res<Option<String>> {
    engine!(state).vault_reveal(server_id, sudo).map_err(err)
}

#[tauri::command]
fn vault_set(state: State<AppState>, server_id: u64, sudo: bool, secret: Option<String>) -> Res<()> {
    engine!(state).vault_set(server_id, sudo, secret.as_deref()).map_err(err)
}

#[tauri::command]
fn vault_change_master(state: State<AppState>, master: String) -> Res<()> {
    engine!(state).vault_change_master(&master).map_err(err)
}

#[tauri::command]
fn profile_save(state: State<AppState>, id: String, name: String) -> Res<String> {
    engine!(state).profile_save(&id, &name).map_err(err)
}

#[tauri::command]
fn profile_delete(state: State<AppState>, id: String) -> Res<()> {
    engine!(state).profile_delete(&id).map_err(err)
}

/// Switches the profile and the accounts it asks for; the answer lists what could not be switched.
#[tauri::command]
fn set_active_profile(state: State<AppState>, id: String) -> Res<Vec<String>> {
    engine!(state).switch_profile(&id).map_err(err)
}

#[tauri::command]
fn accounts(state: State<AppState>) -> Res<oso_core::accounts::AccountsView> {
    Ok(engine!(state).accounts())
}

#[tauri::command]
fn account_use(state: State<AppState>, tool: String, id: String) -> Res<()> {
    engine!(state).account_use(&tool, &id).map_err(err)
}

#[tauri::command]
fn profile_set_accounts(state: State<AppState>, profile: String, gh_user: Option<String>, tailscale_account: Option<String>) -> Res<()> {
    engine!(state).profile_set_accounts(&profile, gh_user, tailscale_account).map_err(err)
}

#[tauri::command]
fn network_accept(state: State<AppState>, server_id: u64) -> Res<()> {
    engine!(state).network_accept(server_id);
    Ok(())
}

#[tauri::command]
fn set_view_all(state: State<AppState>, all: bool) -> Res<()> {
    engine!(state).set_view_all(all);
    Ok(())
}

#[tauri::command]
fn set_folder_profile(state: State<AppState>, id: u64, profile: Option<String>) -> Res<()> {
    engine!(state).set_folder_profile(id, profile).map_err(err)
}

#[tauri::command]
fn proxy_save(state: State<AppState>, proxy: Proxy) -> Res<u64> {
    engine!(state).proxy_save(proxy).map_err(err)
}

#[tauri::command]
fn proxy_delete(state: State<AppState>, id: u64) -> Res<()> {
    engine!(state).proxy_delete(id).map_err(err)
}

#[tauri::command]
fn profile_set_proxy(state: State<AppState>, profile: String, proxy: Option<u64>) -> Res<()> {
    engine!(state).profile_set_proxy(&profile, proxy).map_err(err)
}

/// The person's answer to what an AI agent asked: `once`, `always_read`, `deny` or `terminal`.
#[tauri::command]
fn approval_answer(state: State<AppState>, id: u64, decision: String) -> Res<bool> {
    let d = match decision.as_str() {
        "once" => oso_core::approvals::Decision::Once,
        "always_read" => oso_core::approvals::Decision::AlwaysRead,
        "terminal" => oso_core::approvals::Decision::Terminal,
        _ => oso_core::approvals::Decision::Deny,
    };
    Ok(state.approvals.answer(id, d))
}

/// The last lines of what agents asked and what happened (`audit.jsonl`), newest last.
#[tauri::command]
fn audit_recent(state: State<AppState>, limit: usize) -> Res<Vec<Value>> {
    let path = engine!(state).store_path().with_file_name("audit.jsonl");
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    Ok(lines[lines.len().saturating_sub(limit.min(500))..].iter().filter_map(|l| serde_json::from_str(l).ok()).collect())
}

/// What agents asked, one entry per request (decided how, and how it ended), oldest first: the activity panel.
#[tauri::command]
fn agent_activity(state: State<AppState>, limit: usize) -> Res<Vec<oso_core::audit::Activity>> {
    let path = engine!(state).store_path().with_file_name("audit.jsonl");
    Ok(oso_core::audit::recent_activity(&path, limit.clamp(1, 300)))
}

fn agent_of(name: &str) -> Res<oso_core::integration::Agent> {
    oso_core::integration::Agent::parse(name).ok_or_else(|| "unknown agent".to_string())
}

fn home_dir() -> Res<std::path::PathBuf> {
    directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf()).ok_or_else(|| "no home folder".to_string())
}

/// Which AI agents already have `oso mcp` registered (reads their files, changes nothing).
#[tauri::command]
fn integration_status() -> Res<Value> {
    let home = home_dir()?;
    use oso_core::integration::{Agent, installed};
    Ok(json!({ "claude": installed(Agent::Claude, &home), "opencode": installed(Agent::Opencode, &home), "codex": installed(Agent::Codex, &home) }))
}

/// What installing (or removing) would change in the agent's files, line by line. Writes nothing.
#[tauri::command]
fn integration_plan(agent: String, install: bool) -> Res<Value> {
    let (agent, home) = (agent_of(&agent)?, home_dir()?);
    let exe = std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| "oso".into());
    let plan = if install { oso_core::integration::plan_install(agent, &home, &exe) } else { oso_core::integration::plan_uninstall(agent, &home) }.map_err(err)?;
    Ok(Value::Array(plan.iter().map(|c| json!({ "path": c.path.to_string_lossy(), "changed": c.changed(), "diff": c.diff() })).collect()))
}

/// Does it, after the window showed the plan and the person pressed the button. A copy of each old file is kept.
#[tauri::command]
fn integration_apply(agent: String, install: bool) -> Res<Vec<String>> {
    let (agent, home) = (agent_of(&agent)?, home_dir()?);
    let exe = std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| "oso".into());
    let plan = if install { oso_core::integration::plan_install(agent, &home, &exe) } else { oso_core::integration::plan_uninstall(agent, &home) }.map_err(err)?;
    Ok(oso_core::integration::apply(&plan).map_err(err)?.iter().map(|p| p.to_string_lossy().into_owned()).collect())
}

#[tauri::command]
fn totp_code(state: State<AppState>, id: u64) -> Res<Value> {
    let (code, left) = engine!(state).totp_code(id).map_err(err)?;
    Ok(json!({ "code": code, "seconds_left": left }))
}

#[tauri::command]
fn server_set_totp(state: State<AppState>, server_id: u64, secret: Option<u64>) -> Res<()> {
    engine!(state).server_set_totp(server_id, secret).map_err(err)
}

#[tauri::command]
fn env_preview(state: State<AppState>, dir: String) -> Res<Vec<oso_core::envscan::EnvFile>> {
    Ok(engine!(state).env_preview(&oso_core::connect::expand_tilde(&dir)))
}

#[tauri::command]
fn env_import(state: State<AppState>, dir: String, choices: Vec<oso_core::envscan::EnvChoice>) -> Res<Value> {
    let s = engine!(state).env_import(&oso_core::connect::expand_tilde(&dir), &choices).map_err(err)?;
    Ok(json!({ "added": s.added, "skipped": s.skipped, "originals": s.originals }))
}

/// Only called after the window asked the person, with "keep them" as the first answer.
#[tauri::command]
fn env_delete(state: State<AppState>, dir: String, paths: Vec<String>) -> Res<Vec<String>> {
    engine!(state).env_delete(&oso_core::connect::expand_tilde(&dir), &paths).map_err(err)
}

#[tauri::command]
fn proton_preview(state: State<AppState>, path: String) -> Res<Vec<oso_core::envscan::ProtonItem>> {
    engine!(state).proton_preview(&oso_core::connect::expand_tilde(&path)).map_err(err)
}

#[tauri::command]
fn proton_import(state: State<AppState>, path: String, which: Vec<usize>) -> Res<Value> {
    let s = engine!(state).proton_import(&oso_core::connect::expand_tilde(&path), &which).map_err(err)?;
    Ok(json!({ "added": s.added, "skipped": s.skipped, "originals": [] }))
}

#[tauri::command]
fn cloud_options(state: State<AppState>) -> Res<oso_core::cloud::CloudOptions> {
    Ok(engine!(state).cloud_options())
}

#[tauri::command]
fn profile_set_cloud(state: State<AppState>, profile: String, aws_profile: Option<String>, kubeconfig: Option<String>, production: bool) -> Res<()> {
    engine!(state).profile_set_cloud(&profile, aws_profile, kubeconfig, production).map_err(err)
}

#[tauri::command]
fn import_preview(state: State<AppState>) -> Res<oso_core::ssh_import::Preview> {
    Ok(engine!(state).import_preview(None))
}

#[tauri::command]
fn import_apply(state: State<AppState>, request: ImportRequest) -> Res<ImportResult> {
    engine!(state).import_apply(None, request).map_err(err)
}

/// Only called after the window asked the person, with "keep them" as the first choice.
#[tauri::command]
fn import_delete_originals(state: State<AppState>, paths: Vec<String>) -> Res<Vec<String>> {
    engine!(state).import_delete_originals(None, &paths).map_err(err)
}

#[tauri::command]
fn server_set_key(state: State<AppState>, server_id: u64, secret: Option<u64>) -> Res<()> {
    engine!(state).server_set_key(server_id, secret).map_err(err)
}

#[tauri::command]
fn secret_list(state: State<AppState>) -> Res<Vec<SecretMeta>> {
    engine!(state).secret_list().map_err(err)
}

#[tauri::command]
fn secret_save(state: State<AppState>, meta: SecretMeta, value: Option<String>) -> Res<u64> {
    engine!(state).secret_save(meta, value.as_deref()).map_err(err)
}

#[tauri::command]
fn secret_delete(state: State<AppState>, id: u64) -> Res<()> {
    engine!(state).secret_delete(id).map_err(err)
}

#[tauri::command]
fn secret_reveal(state: State<AppState>, id: u64) -> Res<String> {
    engine!(state).secret_reveal(id).map_err(err)
}

#[tauri::command]
fn save_server(state: State<AppState>, server: Server, login: Option<String>, sudo: Option<String>) -> Res<u64> {
    engine!(state).save_server(server, login.as_deref(), sudo.as_deref()).map_err(err)
}

#[tauri::command]
fn add_folder(state: State<AppState>, name: String, parent: Option<u64>) -> Res<u64> {
    engine!(state).add_folder(name, parent).map_err(err)
}

#[tauri::command]
fn rename_folder(state: State<AppState>, id: u64, name: String) -> Res<()> {
    engine!(state).rename_folder(id, name).map_err(err)
}

#[tauri::command]
fn set_folder_open(state: State<AppState>, id: u64, open: bool) -> Res<()> {
    engine!(state).set_folder_open(id, open).map_err(err)
}

#[tauri::command]
fn delete_node(state: State<AppState>, kind: String, id: u64) -> Res<()> {
    engine!(state).delete_node(node(&kind, id)?).map(|_| ()).map_err(err)
}

#[tauri::command]
fn move_into(state: State<AppState>, kind: String, id: u64, dest: Option<u64>) -> Res<bool> {
    engine!(state).move_into(node(&kind, id)?, dest).map_err(err)
}

#[tauri::command]
fn open_session(state: State<AppState>, server_id: u64, rows: u16, cols: u16, use_vault: bool) -> Res<SessionInfo> {
    engine!(state).open(server_id, rows, cols, use_vault).map_err(open_err)
}

#[tauri::command]
fn reconnect_session(state: State<AppState>, sid: u64, rows: u16, cols: u16, use_vault: bool) -> Res<SessionInfo> {
    engine!(state).reconnect(sid, rows, cols, use_vault).map_err(open_err)
}

#[tauri::command]
fn restore_sessions(state: State<AppState>, rows: u16, cols: u16) -> Res<Vec<SessionInfo>> {
    engine!(state).restore(rows, cols).map_err(err)
}

#[tauri::command]
fn write_session(state: State<AppState>, sid: u64, data: String) -> Res<()> {
    engine!(state).write(sid, data.as_bytes());
    Ok(())
}

#[tauri::command]
fn write_session_bytes(state: State<AppState>, sid: u64, b64: String) -> Res<()> {
    let bytes = B64.decode(b64).map_err(err)?;
    engine!(state).write(sid, &bytes);
    Ok(())
}

#[tauri::command]
fn resize_session(state: State<AppState>, sid: u64, rows: u16, cols: u16) -> Res<()> {
    engine!(state).resize(sid, rows, cols);
    Ok(())
}

#[tauri::command]
fn close_session(state: State<AppState>, sid: u64) -> Res<()> {
    engine!(state).close(sid);
    Ok(())
}

#[tauri::command]
fn set_meta(state: State<AppState>, sid: u64, meta: Value) -> Res<()> {
    engine!(state).set_meta(sid, &meta);
    Ok(())
}

#[tauri::command]
fn fill_sudo(state: State<AppState>, sid: u64, server_id: u64) -> Res<()> {
    engine!(state).fill_sudo(sid, server_id).map_err(err)
}

#[tauri::command]
fn forget_host_key(host: String) -> Res<String> {
    oso_core::known_hosts::forget(&host, None).map_err(err)
}

#[tauri::command]
fn set_forwards(state: State<AppState>, server_id: u64, forwards: Vec<Forward>) -> Res<()> {
    engine!(state).set_forwards(server_id, forwards).map_err(err)
}

#[tauri::command]
fn tunnel_start(state: State<AppState>, server_id: u64, forward_id: u64, use_vault: bool) -> Res<()> {
    engine!(state).tunnel_start(server_id, forward_id, use_vault).map_err(open_err)
}

#[tauri::command]
fn tunnel_stop(state: State<AppState>, server_id: u64, forward_id: u64) -> Res<()> {
    engine!(state).tunnel_stop(server_id, forward_id);
    Ok(())
}

#[tauri::command]
fn tunnels(state: State<AppState>) -> Res<Vec<TunnelStatus>> {
    Ok(engine!(state).tunnels())
}

#[tauri::command]
fn transfer_start(state: State<AppState>, server_id: u64, upload: bool, local: Vec<String>, remote: String, use_vault: bool) -> Res<u64> {
    engine!(state).transfer_start(server_id, upload, local, &remote, use_vault).map_err(open_err)
}

#[tauri::command]
fn transfers(state: State<AppState>) -> Res<Vec<TransferStatus>> {
    Ok(engine!(state).transfers())
}

#[tauri::command]
fn transfer_clear(state: State<AppState>, id: u64) -> Res<()> {
    engine!(state).transfer_clear(id);
    Ok(())
}

/// A desktop notification (the ones of the desktop itself, through `notify-send`). Quietly nothing if there is none.
#[tauri::command]
fn notify(title: String, body: String) {
    let cut = |s: String| s.chars().take(200).collect::<String>();
    let child = std::process::Command::new("notify-send")
        .args(["--app-name=oso", "--icon=oso", "--"])
        .arg(cut(title))
        .arg(cut(body))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    if let Ok(mut c) = child {
        std::thread::spawn(move || {
            let _ = c.wait();
        });
    }
}

#[tauri::command]
fn read_config(name: String) -> Res<Option<String>> {
    let path = config_path(&name)?;
    match std::fs::read_to_string(path) {
        Ok(t) => Ok(Some(t)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(err(e)),
    }
}

#[tauri::command]
fn write_config(name: String, text: String) -> Res<()> {
    let path = config_path(&name)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(err)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(err)?;
    std::fs::rename(tmp, path).map_err(err)
}

fn build_engine(handle: AppHandle) -> anyhow::Result<Engine> {
    let store = Store::load()?;
    let vault = Vault::new(Store::config_dir().join("vault.json"));
    let emit: Emit = Arc::new(move |sid, ev| {
        let _ = handle.emit("session", payload(sid, ev));
    });
    let mut engine = Engine::new(store, vault, emit);
    if let Err(e) = engine.connect_daemon() {
        eprintln!("oso: no background server ({e:#}); sessions will end with the window");
    }
    Ok(engine)
}

fn main() {
    // oso's command line (`oso unlock`, `oso run -- …`): it talks to the window that is open, and never starts one
    let cli_args: Vec<String> = std::env::args().skip(1).collect();
    if oso_core::cli::is_command(&cli_args) {
        let code = oso_core::cli::run(&cli_args, &oso_core::control::socket_path(), &mut oso_core::cli::RealTerm);
        std::process::exit(code);
    }
    // The background server is this same program started with `daemon`: it holds the sessions so they survive the window.
    match std::env::args().nth(1).as_deref() {
        Some("daemon") => {
            if let Err(e) = oso_core::daemon::run_server() {
                eprintln!("oso daemon: {e:#}");
                std::process::exit(1);
            }
            return;
        }
        // `oso mcp`: the MCP server an AI agent starts (it talks over stdin and stdout)
        Some("mcp") => {
            if let Err(e) = oso_core::mcp::run() {
                eprintln!("oso mcp: {e}");
                std::process::exit(1);
            }
            return;
        }
        // `oso integration install|uninstall <agent>`
        Some("integration") => {
            let args: Vec<String> = std::env::args().skip(2).collect();
            let exe = std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| "oso".into());
            if let Err(e) = oso_core::integration::cli(&args, &exe, &mut std::io::stdout()) {
                eprintln!("{e:#}");
                std::process::exit(1);
            }
            return;
        }
        Some("kill-server") => {
            match oso_core::daemon::kill_server() {
                Ok(m) => println!("{m}"),
                Err(e) => eprintln!("{e:#}"),
            }
            return;
        }
        _ => {}
    }

    tauri::Builder::default()
        .setup(|app| {
            let engine = Arc::new(Mutex::new(build_engine(app.handle().clone())?));
            // oso's ssh-agent: the keys of the vault, offered to ssh and git only while the vault is unlocked
            let agent = match Engine::start_agent(&engine) {
                Ok(a) => Some(a),
                Err(e) => {
                    eprintln!("oso: no ssh-agent ({e:#}); keys kept in the vault cannot be used");
                    None
                }
            };
            app.manage(AgentHolder(agent));
            // what AI agents ask of oso: shown to the person, who answers; it all goes in the audit log
            let approvals = oso_core::approvals::Approvals::new();
            let shown = app.handle().clone();
            approvals.on_request(Arc::new(move |r| {
                let _ = shown.emit("approval", &r);
            }));
            let handler = Arc::new(oso_core::agent_api::Handler::new(Arc::clone(&engine), Arc::clone(&approvals)));
            if let Ok(e) = engine.lock() {
                e.set_agent_handler(Arc::new(move |call| handler.handle(call)));
            }
            // the command line: it asks this window (the vault is here); `oso ssh` and `oso host add` tell the page
            let to_page = app.handle().clone();
            let api = Arc::new(oso_core::control::Api {
                engine: Arc::clone(&engine),
                approvals: Arc::clone(&approvals),
                ui: Arc::new(move |event, data| {
                    let _ = to_page.emit("cli", json!({ "event": event, "data": data }));
                }),
            });
            let control = match oso_core::control::start(oso_core::control::socket_path(), Arc::new(move |req| api.handle(req))) {
                Ok(c) => Some(c),
                Err(e) => {
                    eprintln!("oso: the command line will not reach this window ({e:#})");
                    None
                }
            };
            app.manage(ControlHolder(control));
            // the vault locks with the screen and with sleep, as well as when it is idle
            let locker = Arc::clone(&engine);
            oso_core::lockwatch::start(Arc::new(move || {
                if let Ok(mut e) = locker.lock() {
                    e.vault_lock();
                }
            }));
            app.manage(AppState { engine, approvals });
            // The vault locks itself after a while without use.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(Duration::from_secs(30));
                    if let Some(state) = handle.try_state::<AppState>() {
                        if let Ok(mut e) = state.engine.lock() {
                            e.vault.lock_if_idle(Duration::from_secs(15 * 60));
                        }
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            tree,
            vault_status,
            vault_unlock,
            vault_lock,
            vault_reveal,
            vault_set,
            vault_change_master,
            profile_save,
            profile_delete,
            set_active_profile,
            accounts,
            account_use,
            profile_set_accounts,
            network_accept,
            set_view_all,
            set_folder_profile,
            proxy_save,
            proxy_delete,
            profile_set_proxy,
            approval_answer,
            audit_recent,
            agent_activity,
            integration_status,
            integration_plan,
            integration_apply,
            totp_code,
            server_set_totp,
            env_preview,
            env_import,
            env_delete,
            proton_preview,
            proton_import,
            cloud_options,
            profile_set_cloud,
            import_preview,
            import_apply,
            import_delete_originals,
            server_set_key,
            secret_list,
            secret_save,
            secret_delete,
            secret_reveal,
            save_server,
            add_folder,
            rename_folder,
            set_folder_open,
            delete_node,
            move_into,
            open_session,
            reconnect_session,
            restore_sessions,
            write_session,
            write_session_bytes,
            resize_session,
            close_session,
            set_meta,
            fill_sudo,
            forget_host_key,
            notify,
            set_forwards,
            tunnel_start,
            tunnel_stop,
            tunnels,
            transfer_start,
            transfers,
            transfer_clear,
            read_config,
            write_config,
        ])
        .run(tauri::generate_context!())
        .expect("error while running oso");
}
