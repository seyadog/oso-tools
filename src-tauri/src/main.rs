// oso: the window. Everything it does is in `oso-core` (servers, vault, SSH sessions, the background server);
// this file only connects those to the interface.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use oso_core::daemon::Stream;
use oso_core::engine::{Emit, Engine, OpenError, SessionInfo, VaultStatus};
use oso_core::store::{NodeId, Server, Store};
use oso_core::vault::Vault;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

struct AppState {
    engine: Mutex<Engine>,
}

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
    // The background server is this same program started with `daemon`: it holds the sessions so they survive the window.
    match std::env::args().nth(1).as_deref() {
        Some("daemon") => {
            if let Err(e) = oso_core::daemon::run_server() {
                eprintln!("oso daemon: {e:#}");
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
            let engine = build_engine(app.handle().clone())?;
            app.manage(AppState { engine: Mutex::new(engine) });
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
            read_config,
            write_config,
        ])
        .run(tauri::generate_context!())
        .expect("error while running oso");
}
