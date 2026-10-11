import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
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
    profileSave: (id, name) => call("profile_save", { id, name }),
    profileDelete: (id) => call("profile_delete", { id }),
    setActiveProfile: (id) => call("set_active_profile", { id }),
    setViewAll: (all) => call("set_view_all", { all }),
    setFolderProfile: (id, profile) => call("set_folder_profile", { id, profile }),
    accounts: () => call("accounts"),
    accountUse: (tool, id) => call("account_use", { tool, id }),
    profileSetAccounts: (profile, ghUser, tailscaleAccount) => call("profile_set_accounts", { profile, ghUser, tailscaleAccount }),
    networkAccept: (serverId) => call("network_accept", { serverId }),
    proxySave: (proxy) => call("proxy_save", { proxy }),
    proxyDelete: (id) => call("proxy_delete", { id }),
    profileSetProxy: (profile, proxy) => call("profile_set_proxy", { profile, proxy }),
    importPreview: () => call("import_preview"),
    importApply: (request) => call("import_apply", { request }),
    importDeleteOriginals: (paths) => call("import_delete_originals", { paths }),
    serverSetKey: (serverId, secret) => call("server_set_key", { serverId, secret }),
    secretList: () => call("secret_list"),
    secretSave: (meta, value) => call("secret_save", { meta, value }),
    secretDelete: (id) => call("secret_delete", { id }),
    secretReveal: (id) => call("secret_reveal", { id }),
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
    setForwards: (serverId, forwards) => call("set_forwards", { serverId, forwards }),
    tunnelStart: (serverId, forwardId, useVault = true) => call("tunnel_start", { serverId, forwardId, useVault }),
    tunnelStop: (serverId, forwardId) => call("tunnel_stop", { serverId, forwardId }),
    tunnels: () => call("tunnels"),
    transferStart: (serverId, upload, local, remote, useVault = true) => call("transfer_start", { serverId, upload, local, remote, useVault }),
    transfers: () => call("transfers"),
    transferClear: (id) => call("transfer_clear", { id }),
    onFileDrag: (f) =>
      void getCurrentWebview().onDragDropEvent((e) => {
        const p = e.payload;
        const scale = window.devicePixelRatio || 1;
        if (p.type === "leave") return f({ type: "leave", paths: [], x: 0, y: 0 });
        f({ type: p.type === "drop" ? "drop" : "over", paths: "paths" in p ? p.paths : [], x: p.position.x / scale, y: p.position.y / scale });
      }),
    notify: (title, body) => call("notify", { title, body }),
    forgetHostKey: (host) => call("forget_host_key", { host }),
    readConfig: (name) => call("read_config", { name }),
    writeConfig: (name, text) => call("write_config", { name, text }),
    onSession: (f) => void listen<SessionEvent>("session", (e) => f(e.payload)),
  };
}
