import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Backend, SessionEvent, VaultLockedError } from "./api";

/** The commands of src-tauri, one for one. */
export function tauriBackend(): Backend {
  const call = async <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
    try {
      return await invoke<T>(cmd, args);
    } catch (e) {
      if (e === "vault_locked") throw new VaultLockedError();
      throw new Error(String(e));
    }
  };
  return {
    tree: () => call("tree"),
    vaultStatus: () => call("vault_status"),
    vaultUnlock: (master) => call("vault_unlock", { master }),
    vaultLock: () => call("vault_lock"),
    vaultReveal: (serverId, sudo) => call("vault_reveal", { serverId, sudo }),
    vaultSet: (serverId, sudo, secret) => call("vault_set", { serverId, sudo, secret }),
    vaultChangeMaster: (master) => call("vault_change_master", { master }),
    saveServer: (server, login, sudo) => call("save_server", { server, login, sudo }),
    addFolder: (name, parent) => call("add_folder", { name, parent }),
    renameFolder: (id, name) => call("rename_folder", { id, name }),
    setFolderOpen: (id, open) => call("set_folder_open", { id, open }),
    deleteNode: (kind, id) => call("delete_node", { kind, id }),
    moveInto: (kind, id, dest) => call("move_into", { kind, id, dest }),
    open: (serverId, rows, cols, useVault = true) => call("open_session", { serverId, rows, cols, useVault }),
    reconnect: (sid, rows, cols, useVault = true) => call("reconnect_session", { sid, rows, cols, useVault }),
    restore: (rows, cols) => call("restore_sessions", { rows, cols }),
    write: (sid, data) => call("write_session", { sid, data }),
    writeBytes: (sid, b64) => call("write_session_bytes", { sid, b64 }),
    resize: (sid, rows, cols) => call("resize_session", { sid, rows, cols }),
    close: (sid) => call("close_session", { sid }),
    setMeta: (sid, meta) => call("set_meta", { sid, meta }),
    fillSudo: (sid, serverId) => call("fill_sudo", { sid, serverId }),
    readConfig: (name) => call("read_config", { name }),
    writeConfig: (name, text) => call("write_config", { name, text }),
    onSession: (f) => void listen<SessionEvent>("session", (e) => f(e.payload)),
  };
}
