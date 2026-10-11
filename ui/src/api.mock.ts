// A stand-in for the engine, for development and tests in a plain browser: a few servers, a vault whose master
// password is "master", and sessions that behave like a tiny shell ("echo", "exit", "date").

import { Backend, Folder, Server, SessionEvent, SessionInfo, TreeData, VaultLockedError, b64 } from "./api";

export function mockBackend(): Backend {
  let next = 10;
  const folders: Folder[] = [{ id: 1, name: "production", parent: null, expanded: true }];
  const servers: Server[] = [
    { id: 2, name: "web-1", host: "web1.example.com", port: 22, user: "root", auth: "agent", key_path: "", parent: 1, jump: null, has_secret: false, has_sudo: false },
    { id: 3, name: "db-1", host: "db1.example.com", port: 22, user: "admin", auth: "password", key_path: "", parent: 1, jump: null, has_secret: true, has_sudo: false },
    { id: 4, name: "staging", host: "stg.example.com", port: 2222, user: "deploy", auth: "key", key_path: "~/.ssh/id_ed25519", parent: null, jump: null, has_secret: false, has_sudo: false },
  ];
  const config = new Map<string, string>();
  const secrets = new Map<string, string>();
  const sessions = new Map<number, { info: SessionInfo; line: string; dead: boolean }>();
  let handler: (e: SessionEvent) => void = () => {};
  const vault = { exists: true, unlocked: false };
  const enc = new TextEncoder();
  const emit = (sid: number, text: string) => queueMicrotask(() => handler({ sid, kind: "output", data: b64.encode(enc.encode(text)) }));
  const prompt = (title: string) => `\x1b[32m${title}\x1b[0m:\x1b[34m~\x1b[0m$ `;

  const make = (title: string, serverId: number, meta: Record<string, unknown> = {}): SessionInfo => {
    const sid = next++;
    const info: SessionInfo = { sid, title, server_id: serverId, meta: { title, server_id: serverId, ...meta } };
    sessions.set(sid, { info, line: "", dead: false });
    setTimeout(() => handler({ sid, kind: "snapshot", replay: "", screen: b64.encode(enc.encode(`Welcome to ${title}\r\n${prompt(title)}`)), rows: 24, cols: 80, exit: null }), 5);
    return info;
  };

  // For the tests: make a session end the way a dropped connection does.
  const knobs = { failNext: 0 };
  const forgotten: string[] = [];
  const notices: [string, string][] = [];
  (globalThis as unknown as { __osoMock: unknown }).__osoMock = {
    knobs,
    forgotten,
    notices,
    lose: (sid: number, code = 255, text = "") => {
      const s = sessions.get(sid);
      if (!s) return;
      s.dead = true;
      if (text) emit(sid, text);
      queueMicrotask(() => handler({ sid, kind: "exit", code }));
    },
    say: (sid: number, text: string) => emit(sid, text),
  };

  return {
    tree: async () => ({ folders: structuredClone(folders), servers: structuredClone(servers) }) as TreeData,
    vaultStatus: async () => ({ ...vault }),
    vaultUnlock: async (master) => {
      if (master !== "master") throw new Error("Wrong master password");
      vault.unlocked = true;
    },
    vaultLock: async () => void (vault.unlocked = false),
    vaultReveal: async (id, sudo) => {
      if (!vault.unlocked) throw new Error("the vault is locked");
      return secrets.get(`${id}:${sudo ? "s" : "l"}`) ?? (id === 3 && !sudo ? "db-secret-pw" : null);
    },
    vaultSet: async (id, sudo, secret) => {
      if (!vault.unlocked) throw new Error("the vault is locked");
      const s = servers.find((x) => x.id === id)!;
      if (secret) secrets.set(`${id}:${sudo ? "s" : "l"}`, secret);
      else secrets.delete(`${id}:${sudo ? "s" : "l"}`);
      if (sudo) s.has_sudo = !!secret;
      else s.has_secret = !!secret;
    },
    vaultChangeMaster: async () => {},
    saveServer: async (s, login, sudo) => {
      if ((login || sudo) && !vault.unlocked) throw new Error("unlock the vault to save a password");
      const id = s.id || next++;
      const copy = { ...s, id, has_secret: login ? true : login === "" ? false : s.has_secret, has_sudo: sudo ? true : sudo === "" ? false : s.has_sudo };
      const at = servers.findIndex((x) => x.id === id);
      if (at >= 0) servers[at] = copy;
      else servers.push(copy);
      return id;
    },
    addFolder: async (name, parent) => {
      const id = next++;
      folders.push({ id, name, parent, expanded: true });
      return id;
    },
    renameFolder: async (id, name) => void (folders.find((f) => f.id === id)!.name = name),
    setFolderOpen: async (id, open) => void (folders.find((f) => f.id === id)!.expanded = open),
    deleteNode: async (kind, id) => {
      const list = kind === "folder" ? folders : servers;
      list.splice(list.findIndex((x) => x.id === id), 1);
    },
    moveInto: async (kind, id, dest) => {
      const n = (kind === "folder" ? folders : servers).find((x) => x.id === id);
      if (!n) return false;
      n.parent = dest;
      return true;
    },
    open: async (serverId, _rows, _cols, useVault = true) => {
      const s = servers.find((x) => x.id === serverId);
      if (serverId && !s) throw new Error("This server no longer exists");
      if (useVault && s?.has_secret && !vault.unlocked) throw new VaultLockedError();
      return make(s?.name ?? "Local", serverId);
    },
    reconnect: async (sid) => {
      const old = sessions.get(sid);
      if (!old) throw new Error("no such session");
      sessions.delete(sid);
      const info = make(old.info.title, old.info.server_id, old.info.meta);
      if (knobs.failNext > 0) {
        knobs.failNext--;
        setTimeout(() => {
          const n = sessions.get(info.sid);
          if (n) n.dead = true;
          handler({ sid: info.sid, kind: "exit", code: 255 });
        }, 60);
      }
      return info;
    },
    restore: async () => [...sessions.values()].map((s) => s.info),
    write: async (sid, data) => {
      const s = sessions.get(sid);
      if (!s) return;
      if (s.dead) {
        if (data.includes("\r")) emit(sid, "\r\n(reconnect with Enter handled by the window)\r\n");
        return;
      }
      for (const ch of data) {
        if (ch === "\r") {
          const cmd = s.line.trim();
          s.line = "";
          if (cmd === "exit") {
            s.dead = true;
            emit(sid, "\r\nlogout\r\n");
            queueMicrotask(() => handler({ sid, kind: "exit", code: 0 }));
            return;
          }
          const out = cmd.startsWith("echo ") ? cmd.slice(5) : cmd === "date" ? "Mon Jan  1 00:00:00 UTC 2026" : cmd ? `${cmd}: command not found` : "";
          emit(sid, `\r\n${out}${out ? "\r\n" : ""}${prompt(s.info.title)}`);
        } else if (ch === "\x7f") {
          if (s.line) {
            s.line = s.line.slice(0, -1);
            emit(sid, "\b \b");
          }
        } else if (ch >= " ") {
          s.line += ch;
          emit(sid, ch);
        }
      }
    },
    writeBytes: async () => {},
    resize: async () => {},
    close: async (sid) => void sessions.delete(sid),
    setMeta: async (sid, meta) => {
      const s = sessions.get(sid);
      if (s) s.info.meta = meta;
    },
    fillSudo: async () => {},
    notify: async (title, body) => void notices.push([title, body]),
    forgetHostKey: async (host) => {
      forgotten.push(host);
      return `Host ${host} found: line 1`;
    },
    readConfig: async (name) => config.get(name) ?? null,
    writeConfig: async (name, text) => void config.set(name, text),
    onSession: (f) => void (handler = f),
  };
}
