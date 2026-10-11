// What the window asks of the engine. In the app these go to Rust (Tauri); in a plain browser (development and
// tests) a small mock stands in, so the whole interface can be tried without a background server.

export interface Server {
  id: number;
  name: string;
  host: string;
  port: number;
  user: string;
  auth: "agent" | "key" | "password";
  key_path: string;
  parent: number | null;
  jump: number | null;
  has_secret: boolean;
  has_sudo: boolean;
}
export interface Folder {
  id: number;
  name: string;
  parent: number | null;
  expanded: boolean;
}
export interface TreeData {
  folders: Folder[];
  servers: Server[];
}
export interface VaultStatus {
  exists: boolean;
  unlocked: boolean;
}
export interface SessionInfo {
  sid: number;
  title: string;
  server_id: number;
  meta: Record<string, unknown>;
}
export type SessionEvent =
  | { sid: number; kind: "snapshot"; replay: string; screen: string; rows: number; cols: number; exit: number | null }
  | { sid: number; kind: "output"; data: string }
  | { sid: number; kind: "exit"; code: number }
  | { sid: number; kind: "sudo"; on: boolean }
  | { sid: number; kind: "lost" };

export class VaultLockedError extends Error {
  constructor() {
    super("vault locked");
  }
}

export interface Backend {
  tree(): Promise<TreeData>;
  vaultStatus(): Promise<VaultStatus>;
  vaultUnlock(master: string): Promise<void>;
  vaultLock(): Promise<void>;
  vaultReveal(serverId: number, sudo: boolean): Promise<string | null>;
  vaultSet(serverId: number, sudo: boolean, secret: string | null): Promise<void>;
  vaultChangeMaster(master: string): Promise<void>;
  saveServer(s: Server, login: string | null, sudo: string | null): Promise<number>;
  addFolder(name: string, parent: number | null): Promise<number>;
  renameFolder(id: number, name: string): Promise<void>;
  setFolderOpen(id: number, open: boolean): Promise<void>;
  deleteNode(kind: "folder" | "server", id: number): Promise<void>;
  moveInto(kind: "folder" | "server", id: number, dest: number | null): Promise<boolean>;
  open(serverId: number, rows: number, cols: number, useVault?: boolean): Promise<SessionInfo>;
  reconnect(sid: number, rows: number, cols: number, useVault?: boolean): Promise<SessionInfo>;
  restore(rows: number, cols: number): Promise<SessionInfo[]>;
  write(sid: number, data: string): Promise<void>;
  writeBytes(sid: number, b64: string): Promise<void>;
  resize(sid: number, rows: number, cols: number): Promise<void>;
  close(sid: number): Promise<void>;
  setMeta(sid: number, meta: Record<string, unknown>): Promise<void>;
  fillSudo(sid: number, serverId: number): Promise<void>;
  forgetHostKey(host: string): Promise<string>;
  readConfig(name: string): Promise<string | null>;
  writeConfig(name: string, text: string): Promise<void>;
  onSession(f: (e: SessionEvent) => void): void;
}

export const inTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export async function loadBackend(): Promise<Backend> {
  if (inTauri()) return (await import("./api.tauri")).tauriBackend();
  return (await import("./api.mock")).mockBackend();
}

export const b64 = {
  encode(bytes: Uint8Array): string {
    let s = "";
    for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
    return btoa(s);
  },
  decode(text: string): Uint8Array {
    const s = atob(text);
    const out = new Uint8Array(s.length);
    for (let i = 0; i < s.length; i++) out[i] = s.charCodeAt(i);
    return out;
  },
};
