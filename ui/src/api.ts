import type { Tree } from "./tree";
// What the window asks of the engine. In the app these go to Rust (Tauri); in a plain browser (development and
// tests) a small mock stands in, so the whole interface can be tried without a background server.

export interface Server {
  id: number;
  name: string;
  host: string;
  port: number;
  user: string;
  auth: "agent" | "key" | "password" | "vaultkey";
  key_path: string;
  parent: number | null;
  jump: number | null;
  has_secret: boolean;
  has_sudo: boolean;
  /** A seed for its one-time codes is linked in the vault. */
  has_totp: boolean;
  forwards: Forward[];
  /** Free text about the machine. */
  notes: string;
  env_kind: "production" | "staging" | "dev" | null;
  /** Id of the saved proxy its connection leaves through. */
  proxy: number | null;
  /** What an AI agent may do here without asking. */
  ai_policy: "ask" | "read_only_auto" | "blocked";
  /** `tailscale:<account>`: the network it is on (oso asks before connecting on another account). */
  network: string | null;
  /** The secret id of the seed of its one-time codes (only in what `tree` returns, with the vault unlocked). */
  vault_totp?: number | null;
  /** The secret id of the key it logs in with, when that is a key of the vault (only in what `tree` returns, with the vault unlocked). */
  vault_key?: number | null;
  /** Its own profile (null: its folder's, else every profile). */
  profile: string | null;
  /** Where it ended up: its own or its folder's (only in what `tree` returns). */
  profile_eff?: string | null;
}
export interface Forward {
  /** 0 for one that is not saved yet. */
  id: number;
  name: string;
  /** Also opened by every terminal of the server. */
  auto: boolean;
  kind: "local" | "remote" | "dynamic";
  bind_port: number;
  host: string;
  port: number;
}
export interface Folder {
  id: number;
  name: string;
  parent: number | null;
  expanded: boolean;
  profile: string | null;
}
export interface Profile {
  id: string;
  name: string;
  /** Id of the proxy its local terminals go out through. */
  proxy: number | null;
  /** The GitHub account (`host/login`) to use while it is active. */
  gh_user: string | null;
  /** The Tailscale account to be on while it is active. */
  tailscale_account: string | null;
  /** `AWS_PROFILE` for its local terminals. */
  aws_profile: string | null;
  /** `KUBECONFIG` for them. */
  kubeconfig: string | null;
  /** Shown in red. */
  production: boolean;
}
export interface CloudOptions {
  aws_profiles: string[];
  kubeconfigs: { path: string; contexts: string[] }[];
}
export interface EnvFile {
  path: string;
  project: string;
  entries: { name: string; kind: SecretKind; length: number }[];
  others: number;
}
export interface ProtonItem {
  name: string;
  username: string;
  url: string;
  note: string;
  has_totp: boolean;
}
/** What an import did. */
export interface ImportSummary {
  added: string[];
  skipped: string[];
  originals: string[];
}
/** Something the command line asked the window to do (`oso ssh`, `oso host add`…). */
export interface CliEvent {
  event: "open" | "refresh";
  data: { server_id?: number } | null;
}
export interface Account {
  id: string;
  name: string;
  detail: string;
  active: boolean;
}
export interface ToolAccounts {
  /** False when the program is not installed (the window hides it). */
  installed: boolean;
  accounts: Account[];
  error: string | null;
}
export interface AccountsView {
  github: ToolAccounts;
  tailscale: ToolAccounts;
}
export interface Proxy {
  /** 0 for one that is not saved yet. */
  id: number;
  name: string;
  kind: "socks5" | "http";
  host: string;
  port: number;
  profile: string | null;
}
export interface TreeData {
  folders: Folder[];
  servers: Server[];
  profiles: Profile[];
  proxies: Proxy[];
  active_profile: string;
  /** The sidebar shows the hosts of every profile. */
  view_all: boolean;
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

export type SecretKind = "password" | "sudo" | "ssh_key" | "api_key" | "token" | "totp";
/** What a secret is, without its value: lists never carry the value. */
export interface SecretMeta {
  /** 0 for one that is not saved yet. */
  id: number;
  name: string;
  kind: SecretKind;
  profile: string | null;
  env_var: string | null;
  agent_visible: boolean;
  notes: string;
  /** Unix seconds of the last change. */
  updated: number;
}

export interface KeyFile {
  path: string;
  name: string;
  algorithm: string;
  fingerprint: string;
  comment: string;
  encrypted: boolean;
  /** Why it cannot go into the vault, or "". */
  unsupported: string;
}
export interface HostEntry {
  alias: string;
  host: string;
  user: string;
  port: number;
  identity_file: string | null;
  proxy_jump: string | null;
}
export interface ImportPreview {
  keys: KeyFile[];
  hosts: HostEntry[];
  skipped: string[];
}
export interface ImportRequest {
  keys: { path: string; passphrase: string | null }[];
  hosts: string[];
}
export interface ImportResult {
  keys_added: string[];
  hosts_added: string[];
  problems: string[];
  /** The files that were imported (for the question about deleting them). */
  originals: string[];
}

/** What an AI agent asks, for the person to answer. */
export interface ApprovalRequest {
  id: number;
  pane: number;
  /** "Claude in acme-api, pane 2". */
  label: string;
  agent: string;
  action: "run" | "sudo" | "run_with" | "tunnel";
  host: string;
  command: string;
  production: boolean;
  can_remember: boolean;
}
export type ApprovalDecision = "once" | "always_read" | "deny" | "terminal";
export interface AuditEntry {
  ts: number;
  pane: number;
  agent: string;
  action: string;
  host: string;
  command: string;
  decision: string;
  exit_code: number | null;
  note: string;
}

/** One thing an agent asked, with how it was decided and how it ended (one per request, from the audit log). */
export interface Activity {
  id: number;
  /** When it was asked, and when anything last happened to it (Unix seconds). */
  ts: number;
  updated: number;
  pane: number;
  agent: string;
  action: string;
  host: string;
  command: string;
  /** `allowed` (you said yes), `allowed_and_remembered_for_reads`, `allowed_without_asking` (automatic), `denied`, `blocked`. */
  decision: string;
  /** The server's policy at that moment: `ask`, `read_only_auto`, `blocked` (empty when none applied). */
  policy: string;
  reason: string;
  state: "running" | "finished" | "failed" | "denied" | "blocked" | "done" | "unknown";
  exit_code: number | null;
  bytes_out: number | null;
  /** The first lines of what it printed, already blanked of secrets. */
  preview: string;
  /** There is more than the preview: `activityDetail` has all of it. */
  has_output: boolean;
  note: string;
}

/** A terminal of a saved tab of a Space. */
export interface SpacePane {
  /** The server (0: this computer). */
  server: number;
  name: string;
  /** Typed once the terminal shows its prompt. */
  command: string;
  /** Where a local terminal opens (empty: the Space's first folder). */
  cwd: string;
}
export interface SpaceTab {
  name: string;
  broadcast: boolean;
  /** The tiling tree, the terminals numbered 0, 1, 2… as in `panes`. */
  shape: Tree;
  panes: SpacePane[];
}
export interface TunnelRef {
  server: number;
  forward: number;
}
/** One unit of work: what it uses, its notes, and the layout of its tabs. */
export interface Space {
  /** 0: not saved yet. */
  id: number;
  name: string;
  profile: string | null;
  folders: string[];
  servers: number[];
  server_folders: number[];
  secrets: number[];
  tunnels: TunnelRef[];
  notes: string;
  tabs: SpaceTab[];
}
/** What turning `groups.json` into Spaces did (told once). */
export interface SpaceMigration {
  imported: string[];
  skipped: string[];
}

/** The agents running in this window's terminals. */
export interface AgentStatusRow {
  sid: number;
  kind: string;
  name: string;
  pid: number | null;
  cwd: string;
  folder: string;
  branch: string;
  state: "needs_you" | "working" | "finished" | "idle";
  since: number;
  source: "approval" | "hook" | "process";
}
export interface AgentKindRow {
  id: string;
  name: string;
  command: string;
}

export interface ActivityDetail {
  command: string;
  output: string;
  /** The output was longer than oso keeps. */
  cut: boolean;
}

export interface TunnelStatus {
  server_id: number;
  forward_id: number;
  state: "starting" | "up" | "failed";
  message: string;
}
export interface TransferStatus {
  id: number;
  server_id: number;
  upload: boolean;
  what: string;
  state: "running" | "done" | "failed";
  percent: number | null;
  message: string;
}
/** Files dragged from the desktop over the window (positions in the window's own pixels). */
export interface FileDrag {
  type: "over" | "drop" | "leave";
  paths: string[];
  x: number;
  y: number;
}

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
  /** Adds a profile (`id` empty) or renames one; returns its id. */
  profileSave(id: string, name: string): Promise<string>;
  profileDelete(id: string): Promise<void>;
  /** Switches the profile and the accounts it asks for; returns what could not be switched, in words. */
  setActiveProfile(id: string): Promise<string[]>;
  accounts(): Promise<AccountsView>;
  accountUse(tool: "github" | "tailscale", id: string): Promise<void>;
  profileSetAccounts(profile: string, ghUser: string | null, tailscaleAccount: string | null): Promise<void>;
  /** "Connect anyway": the next open of this server skips the network question. */
  networkAccept(serverId: number): Promise<void>;
  setViewAll(all: boolean): Promise<void>;
  setFolderProfile(id: number, profile: string | null): Promise<void>;
  proxySave(p: Proxy): Promise<number>;
  proxyDelete(id: number): Promise<void>;
  profileSetProxy(profile: string, proxy: number | null): Promise<void>;
  /** What `~/.ssh` has that could come in (read only). */
  importPreview(): Promise<ImportPreview>;
  importApply(req: ImportRequest): Promise<ImportResult>;
  /** Deletes imported key files: only ever called after the person said yes. Returns the names deleted. */
  importDeleteOriginals(paths: string[]): Promise<string[]>;
  /** The server logs in with this key of the vault (null: unlink). */
  serverSetKey(serverId: number, secret: number | null): Promise<void>;
  totpCode(id: number): Promise<{ code: string; seconds_left: number }>;
  serverSetTotp(serverId: number, secret: number | null): Promise<void>;
  envPreview(dir: string): Promise<EnvFile[]>;
  envImport(dir: string, choices: { path: string; names: string[] }[]): Promise<ImportSummary>;
  /** Only called after the person said yes. */
  envDelete(dir: string, paths: string[]): Promise<string[]>;
  protonPreview(path: string): Promise<ProtonItem[]>;
  protonImport(path: string, which: number[]): Promise<ImportSummary>;
  cloudOptions(): Promise<CloudOptions>;
  profileSetCloud(profile: string, awsProfile: string | null, kubeconfig: string | null, production: boolean): Promise<void>;
  onCli(f: (e: CliEvent) => void): void;
  integrationStatus(): Promise<{ claude: boolean; opencode: boolean; codex: boolean }>;
  /** What connecting (or removing) an agent would change in its files; writes nothing. */
  integrationPlan(agent: string, install: boolean): Promise<{ path: string; changed: boolean; diff: string }[]>;
  integrationApply(agent: string, install: boolean): Promise<string[]>;
  onApproval(f: (r: ApprovalRequest) => void): void;
  approvalAnswer(id: number, decision: ApprovalDecision): Promise<boolean>;
  auditRecent(limit: number): Promise<AuditEntry[]>;
  /** The whole command and output of one request (what Details shows); `null` for an old log line. */
  activityDetail(id: number): Promise<ActivityDetail | null>;
  /** The name of the person using this computer ("seya"). */
  localUser(): Promise<string>;
  /** The agents that run in the terminals, and what each is doing. `pending`: terminals with a request waiting. */
  agentStatus(pending: number[]): Promise<AgentStatusRow[]>;
  /** The person looked at this terminal. */
  agentAck(sid: number): Promise<void>;
  agentKinds(): Promise<AgentKindRow[]>;
  agentKindsSave(kinds: AgentKindRow[]): Promise<void>;
  /** `git worktree add -b <branch>` next to the repository; the folder it made. */
  worktreeAdd(repo: string, branch: string): Promise<string>;
  /** What the person is looking at (ids only), for an agent that is told "this server". */
  focusUpdate(selected: number | null, open: number[], active: number | null): Promise<void>;
  /** A sound for the window to play: one of the desktop's theme (`system`: "complete" or "message") or a file (`file`: a path). */
  soundLoad(kind: "system" | "file", value: string): Promise<{ mime: string; b64: string }>;
  /** A desktop notice about an agent; clicking it brings oso back and calls the `onNotifyClick` handler with `sid`. */
  notifyAgent(title: string, body: string, sid: number): Promise<void>;
  onNotifyClick(f: (sid: number) => void): void;
  /** What agents asked, one entry per request, oldest first. */
  agentActivity(limit: number): Promise<Activity[]>;
  secretList(): Promise<SecretMeta[]>;
  /** `value` null keeps the saved one of an existing secret. */
  secretSave(meta: SecretMeta, value: string | null): Promise<number>;
  secretDelete(id: number): Promise<void>;
  secretReveal(id: number): Promise<string>;
  saveServer(s: Server, login: string | null, sudo: string | null): Promise<number>;
  addFolder(name: string, parent: number | null): Promise<number>;
  renameFolder(id: number, name: string): Promise<void>;
  setFolderOpen(id: number, open: boolean): Promise<void>;
  deleteNode(kind: "folder" | "server", id: number): Promise<void>;
  moveInto(kind: "folder" | "server", id: number, dest: number | null): Promise<boolean>;
  open(serverId: number, rows: number, cols: number, useVault?: boolean, opts?: { space?: number; cwd?: string }): Promise<SessionInfo>;
  spaceList(): Promise<Space[]>;
  /** Saves (id 0: a new one) and returns the id. */
  spaceSave(space: Space): Promise<number>;
  spaceDelete(id: number): Promise<void>;
  /** What the first start with Spaces did to the old saved groups; `null` when nothing was migrated (or it was told). */
  spaceMigration(): Promise<SpaceMigration | null>;
  reconnect(sid: number, rows: number, cols: number, useVault?: boolean): Promise<SessionInfo>;
  restore(rows: number, cols: number): Promise<SessionInfo[]>;
  write(sid: number, data: string): Promise<void>;
  writeBytes(sid: number, b64: string): Promise<void>;
  resize(sid: number, rows: number, cols: number): Promise<void>;
  close(sid: number): Promise<void>;
  setMeta(sid: number, meta: Record<string, unknown>): Promise<void>;
  fillSudo(sid: number, serverId: number): Promise<void>;
  forgetHostKey(host: string): Promise<string>;
  notify(title: string, body: string): Promise<void>;
  setForwards(serverId: number, forwards: Forward[]): Promise<void>;
  tunnelStart(serverId: number, forwardId: number, useVault?: boolean): Promise<void>;
  tunnelStop(serverId: number, forwardId: number): Promise<void>;
  tunnels(): Promise<TunnelStatus[]>;
  transferStart(serverId: number, upload: boolean, local: string[], remote: string, useVault?: boolean): Promise<number>;
  transfers(): Promise<TransferStatus[]>;
  transferClear(id: number): Promise<void>;
  onFileDrag(f: (e: FileDrag) => void): void;
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
