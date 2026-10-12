// A stand-in for the engine, for development and tests in a plain browser: a few servers, a vault whose master
// password is "master", and sessions that behave like a tiny shell ("echo", "exit", "date").

import { AccountsView, Activity, AgentStatusRow, Space, SpaceMigration, ApprovalRequest, AuditEntry, Backend, CliEvent, EnvFile, ProtonItem, FileDrag, Folder, Forward, Profile, Proxy, SecretMeta, Server, SessionEvent, SessionInfo, TransferStatus, TreeData, TunnelStatus, VaultLockedError, b64 } from "./api";

export function mockBackend(): Backend {
  let next = 10;
  const folders: Folder[] = [{ id: 1, name: "production", parent: null, expanded: true, profile: null }];
  const profiles: Profile[] = [{ id: "default", name: "default", proxy: null, gh_user: null, tailscale_account: null, aws_profile: null, kubeconfig: null, production: false }];
  const tools: AccountsView = {
    github: { installed: true, error: null, accounts: [{ id: "github.com/me", name: "me", detail: "github.com", active: true }, { id: "github.com/work-user", name: "work-user", detail: "github.com", active: false }] },
    tailscale: { installed: true, error: null, accounts: [{ id: "4ab1", name: "me@corp.example.com", detail: "corp.example.com", active: true }, { id: "9fe2", name: "me@gmail.com", detail: "home.ts.net", active: false }] },
  };
  const networkOk = new Set<number>();
  const use = (tool: "github" | "tailscale", id: string) => {
    const t = tools[tool];
    if (!t.accounts.some((a) => a.id === id || a.name === id || a.detail === id)) throw new Error(`${tool === "github" ? "GitHub" : "Tailscale"}: no such account`);
    t.accounts.forEach((a) => (a.active = a.id === id || a.name === id || a.detail === id));
  };
  const proxies: Proxy[] = [];
  let activeProfile = "default";
  let viewAll = false;
  const servers: Server[] = [
    { id: 2, name: "web-1", host: "web1.example.com", port: 22, user: "root", auth: "agent", key_path: "", parent: 1, jump: null, has_secret: false, has_sudo: false, has_totp: false, forwards: [], profile: null, notes: "", env_kind: null, proxy: null, network: null, ai_policy: "ask" },
    { id: 3, name: "db-1", host: "db1.example.com", port: 22, user: "admin", auth: "password", key_path: "", parent: 1, jump: null, has_secret: true, has_sudo: false, has_totp: false, forwards: [], profile: null, notes: "", env_kind: null, proxy: null, network: null, ai_policy: "ask" },
    { id: 4, name: "staging", host: "stg.example.com", port: 2222, user: "deploy", auth: "key", key_path: "~/.ssh/id_ed25519", parent: null, jump: null, has_secret: false, has_sudo: false, has_totp: false, forwards: [], profile: null, notes: "", env_kind: null, proxy: null, network: null, ai_policy: "ask" },
  ];
  const config = new Map<string, string>();
  // Every secret the vault holds (with its value) and which of them a server logs in or sudoes with, as in the engine.
  const vaultSecrets = new Map<number, { meta: SecretMeta; value: string }>([[900, { meta: { id: 900, name: "db-1 (login)", kind: "password", profile: null, env_var: null, agent_visible: false, notes: "", updated: Math.floor(Date.now() / 1000) }, value: "db-secret-pw" }]]);
  const links = new Map<string, number>([["3:l", 900]]);
  let nextSecret = 901;
  const setLink = (serverId: number, sudo: boolean, secret: string | null, label: string) => {
    const key = `${serverId}:${sudo ? "s" : "l"}`;
    const cur = links.get(key);
    if (secret && cur) vaultSecrets.get(cur)!.value = secret;
    else if (secret) {
      const id = nextSecret++;
      vaultSecrets.set(id, { meta: { id, name: `${label} (${sudo ? "sudo" : "login"})`, kind: sudo ? "sudo" : "password", profile: null, env_var: null, agent_visible: false, notes: "", updated: Math.floor(Date.now() / 1000) }, value: secret });
      links.set(key, id);
    } else if (cur) {
      vaultSecrets.delete(cur);
      links.delete(key);
    }
  };
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
  const knobs = { failNext: 0, noTools: false, empty: false, noRepo: false };
  const forgotten: string[] = [];
  const deletedOriginals: string[] = [];
  const approvalAnswers: [number, string][] = [];
  /** What the engine would say about the agents; a test changes it. */
  const agents: AgentStatusRow[] = [];
  const agentAcks: number[] = [];
  const agentKinds = [{ id: "claude", name: "Claude Code", command: "claude" }, { id: "opencode", name: "OpenCode", command: "opencode" }, { id: "codex", name: "Codex", command: "codex" }];
  const worktrees: [string, string][] = [];
  const focusSent: { selected: number | null; open: number[]; active: number | null }[] = [];
  let notifyClick: (sid: number) => void = () => {};
  const spaces: Space[] = [];
  let nextSpace = 1;
  /** What a test says the migration of the old groups did, to be told once. */
  const migration: { value: SpaceMigration | null } = { value: null };
  /** What the activity panel reads; tests add to it the way the audit log grows. */
  const activity: (Activity & { output?: string })[] = [];
  const deletedEnv: string[] = [];
  const connected = { claude: false, opencode: false, codex: false };
  let cliHandler: (e: CliEvent) => void = () => {};
  let approvalHandler: (r: ApprovalRequest) => void = () => {};
  const notices: [string, string][] = [];
  const tunnelState = new Map<string, TunnelStatus>();
  const transferList: TransferStatus[] = [];
  const started: unknown[] = [];
  let fileDrag: (e: FileDrag) => void = () => {};
  let nextTransfer = 1;
  (globalThis as unknown as { __osoMock: unknown }).__osoMock = {
    knobs,
    forgotten,
    deletedOriginals,
    approvalAnswers,
    spaces,
    migration,
    agents,
    agentAcks,
    agentKinds,
    worktrees,
    focusSent,
    clickNotice: (sid: number) => notifyClick(sid),
    activity,
    deletedEnv,
    cli: (e: CliEvent) => cliHandler(e),
    approval: (r: ApprovalRequest) => approvalHandler(r),
    notices,
    tunnelState,
    started,
    transferList,
    drag: (e: FileDrag) => fileDrag(e),
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
    tree: async () => {
      const folderProfile = (id: number | null): string | null => {
        for (let f = folders.find((x) => x.id === id), n = 0; f && n < 50; f = folders.find((x) => x.id === f!.parent), n++) if (f.profile) return f.profile;
        return null;
      };
      const eff = (s: Server) => s.profile ?? folderProfile(s.parent);
      const inProfile = (p: string | null) => viewAll || p === null || p === activeProfile;
      const srv = knobs.empty ? [] : servers.filter((s) => inProfile(eff(s)));
      const holds = (fid: number) => srv.some((s) => { for (let f = folders.find((x) => x.id === s.parent); f; f = folders.find((x) => x.id === f!.parent)) if (f.id === fid) return true; return false; });
      const fol = knobs.empty ? [] : folders.filter((f) => viewAll || folderProfile(f.id) === activeProfile || holds(f.id) || (folderProfile(f.id) === null && !servers.some((s) => s.parent === f.id)));
      return { folders: structuredClone(fol), servers: structuredClone(srv).map((s) => ({ ...s, profile_eff: eff(s) })), profiles: structuredClone(profiles), proxies: structuredClone(proxies), active_profile: activeProfile, view_all: viewAll } as TreeData;
    },
    profileSave: async (id, name) => {
      if (!name.trim()) throw new Error("a profile needs a name");
      if (id) {
        profiles.find((p) => p.id === id)!.name = name.trim();
        return id;
      }
      const slug = name.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
      if (!slug || profiles.some((p) => p.id === slug)) throw new Error(`there is already a profile called "${name}"`);
      profiles.push({ id: slug, name: name.trim(), proxy: null, gh_user: null, tailscale_account: null, aws_profile: null, kubeconfig: null, production: false });
      return slug;
    },
    profileDelete: async (id) => {
      if (id === "default") throw new Error("the default profile cannot be deleted");
      profiles.splice(profiles.findIndex((p) => p.id === id), 1);
      servers.forEach((s) => s.profile === id && (s.profile = "default"));
      folders.forEach((f) => f.profile === id && (f.profile = "default"));
      if (activeProfile === id) activeProfile = "default";
    },
    setActiveProfile: async (id) => {
      activeProfile = id;
      const p = profiles.find((x) => x.id === id)!;
      const problems: string[] = [];
      for (const [tool, want] of [["github", p.gh_user], ["tailscale", p.tailscale_account]] as const) {
        if (!want) continue;
        try {
          use(tool, want);
        } catch (e) {
          problems.push((e as Error).message);
        }
      }
      return problems;
    },
    accounts: async () => (knobs.noTools ? ({ github: { installed: false, accounts: [], error: null }, tailscale: { installed: false, accounts: [], error: null } } as AccountsView) : structuredClone(tools)),
    accountUse: async (tool, id) => use(tool, id),
    profileSetAccounts: async (profile, gh, ts) => {
      const p = profiles.find((x) => x.id === profile)!;
      p.gh_user = gh || null;
      p.tailscale_account = ts || null;
    },
    networkAccept: async (id) => void networkOk.add(id),
    setViewAll: async (all) => void (viewAll = all),
    proxySave: async (p) => {
      if (!p.name.trim()) throw new Error("a proxy needs a name");
      if (!p.host.trim() || /[\s'"`$;&|]/.test(p.host) || p.host.startsWith("-")) throw new Error("the proxy's host is a name or an address, with no spaces or special characters");
      if (!p.port) throw new Error("a proxy needs a port");
      const id = p.id || Math.max(0, ...proxies.map((x) => x.id)) + 1;
      const at = proxies.findIndex((x) => x.id === id);
      if (at >= 0) proxies[at] = { ...p, id };
      else proxies.push({ ...p, id });
      return id;
    },
    proxyDelete: async (id) => {
      proxies.splice(proxies.findIndex((x) => x.id === id), 1);
      servers.forEach((s) => s.proxy === id && (s.proxy = null));
      profiles.forEach((p) => p.proxy === id && (p.proxy = null));
    },
    profileSetProxy: async (profile, proxy) => void (profiles.find((p) => p.id === profile)!.proxy = proxy),
    setFolderProfile: async (id, profile) => void (folders.find((f) => f.id === id)!.profile = profile),
    vaultStatus: async () => ({ ...vault }),
    vaultUnlock: async (master) => {
      if (master !== "master") throw new Error("Wrong master password");
      vault.unlocked = true;
    },
    vaultLock: async () => void (vault.unlocked = false),
    vaultReveal: async (id, sudo) => {
      if (!vault.unlocked) throw new Error("the vault is locked");
      const at = links.get(`${id}:${sudo ? "s" : "l"}`);
      return at ? vaultSecrets.get(at)!.value : null;
    },
    vaultSet: async (id, sudo, secret) => {
      if (!vault.unlocked) throw new Error("the vault is locked");
      const s = servers.find((x) => x.id === id);
      setLink(id, sudo, secret, s?.name ?? "this computer");
      if (s && sudo) s.has_sudo = !!secret;
      else if (s) s.has_secret = !!secret;
    },
    vaultChangeMaster: async () => {},
    importPreview: async () => ({
      keys: [
        { path: "/home/me/.ssh/id_ed25519", name: "id_ed25519", algorithm: "ssh-ed25519", fingerprint: "SHA256:abc", comment: "me@laptop", encrypted: false, unsupported: "" },
        { path: "/home/me/.ssh/id_work", name: "id_work", algorithm: "ssh-ed25519", fingerprint: "SHA256:def", comment: "me@work", encrypted: true, unsupported: "" },
        { path: "/home/me/.ssh/id_old", name: "id_old", algorithm: "ssh-rsa", fingerprint: "", comment: "", encrypted: false, unsupported: "an old key format (PEM): convert it with `ssh-keygen -p -m RFC4716 -f <file>`" },
      ],
      hosts: [
        { alias: "bastion", host: "bastion.example.com", user: "ops", port: 22, identity_file: "/home/me/.ssh/id_ed25519", proxy_jump: null },
        { alias: "app", host: "app.internal", user: "deploy", port: 2200, identity_file: "/home/me/.ssh/id_work", proxy_jump: "bastion" },
      ],
      skipped: ["Host *.corp: a pattern, not a host"],
    }),
    importApply: async (req) => {
      if (!vault.unlocked) throw new Error("the vault is locked");
      const out = { keys_added: [] as string[], hosts_added: [] as string[], problems: [] as string[], originals: [] as string[] };
      for (const k of req.keys) {
        if (k.path.endsWith("id_work") && k.passphrase !== "right") {
          out.problems.push("id_work: wrong passphrase (or none given)");
          continue;
        }
        const id = nextSecret++;
        const name = k.path.split("/").pop()!;
        vaultSecrets.set(id, { meta: { id, name, kind: "ssh_key", profile: null, env_var: null, agent_visible: false, notes: "imported", updated: Math.floor(Date.now() / 1000) }, value: "PRIVATE" });
        out.keys_added.push(name);
        out.originals.push(k.path);
      }
      for (const a of req.hosts) {
        const id = next++;
        servers.push({ id, name: a, host: `${a}.example`, port: 22, user: "", auth: "agent", key_path: "", parent: null, jump: null, has_secret: false, has_sudo: false, has_totp: false, forwards: [], profile: null, notes: "", env_kind: null, proxy: null, network: null, ai_policy: "ask" });
        out.hosts_added.push(a);
      }
      return out;
    },
    importDeleteOriginals: async (paths) => {
      deletedOriginals.push(...paths);
      return paths.map((p) => p.split("/").pop()!);
    },
    serverSetKey: async (serverId, secret) => {
      if (!vault.unlocked) throw new Error("the vault is locked");
      const s = servers.find((x) => x.id === serverId)!;
      s.vault_key = secret;
      if (secret) s.auth = "vaultkey";
    },
    totpCode: async (id) => {
      if (!vault.unlocked) throw new Error("the vault is locked");
      const x = vaultSecrets.get(id);
      if (!x || x.meta.kind !== "totp") throw new Error("that secret does not make one-time codes");
      const now = Math.floor(Date.now() / 1000);
      return { code: String(123456 + (Math.floor(now / 30) % 1000)).slice(0, 6), seconds_left: 30 - (now % 30) };
    },
    serverSetTotp: async (serverId, secret) => {
      if (!vault.unlocked) throw new Error("the vault is locked");
      const s = servers.find((x) => x.id === serverId)!;
      s.has_totp = secret != null;
      s.vault_totp = secret;
    },
    envPreview: async (dir) => (dir.includes("nothing") ? [] : [
      { path: `${dir}/app/.env`, project: "app", entries: [{ name: "STRIPE_KEY", kind: "api_key", length: 17 }, { name: "DB_PASSWORD", kind: "password", length: 14 }], others: 1 },
      { path: `${dir}/web/.env.production`, project: "web", entries: [{ name: "API_TOKEN", kind: "token", length: 20 }], others: 0 },
    ] as EnvFile[]),
    envImport: async (_dir, choices) => {
      if (!vault.unlocked) throw new Error("the vault is locked");
      const out = { added: [] as string[], skipped: [] as string[], originals: [] as string[] };
      for (const c of choices) {
        const project = c.path.split("/").slice(-2)[0];
        const names = c.names.length ? c.names : project === "app" ? ["STRIPE_KEY", "DB_PASSWORD"] : ["API_TOKEN"];
        for (const n of names) {
          const id = nextSecret++;
          vaultSecrets.set(id, { meta: { id, name: `${project}/${n}`, kind: "token", profile: null, env_var: null, agent_visible: false, notes: `imported from ${c.path}`, updated: Math.floor(Date.now() / 1000) }, value: "v" });
          out.added.push(`${project}/${n}`);
        }
        out.originals.push(c.path);
      }
      return out;
    },
    envDelete: async (_dir, paths) => {
      deletedEnv.push(...paths);
      return paths;
    },
    protonPreview: async (path) => {
      if (path.includes("bad")) throw new Error("no logins found in that file (a Proton Pass export, CSV or JSON, without encryption)");
      return [
        { name: "GitHub", username: "me", url: "https://github.com", note: "", has_totp: true },
        { name: "Mail", username: "m@x.com", url: "https://mail", note: "", has_totp: false },
      ] as ProtonItem[];
    },
    protonImport: async (_path, which) => {
      if (!vault.unlocked) throw new Error("the vault is locked");
      return { added: which.map((i) => ["GitHub (me)", "Mail (m@x.com)"][i]), skipped: [], originals: [] };
    },
    cloudOptions: async () => ({ aws_profiles: ["default", "acme-prod"], kubeconfigs: [{ path: "/home/me/.kube/config", contexts: ["prod", "dev"] }] }),
    profileSetCloud: async (profile, aws, kube, production) => {
      const p = profiles.find((x) => x.id === profile)!;
      p.aws_profile = aws || null;
      p.kubeconfig = kube || null;
      p.production = production;
    },
    onCli: (f) => void (cliHandler = f),
    integrationStatus: async () => ({ ...connected }),
    integrationPlan: async (agent, install) => [
      { path: `/home/me/.${agent}.json`, changed: true, diff: install ? '+ "oso": {\n+   "command": "/home/me/.local/bin/oso-app",\n+   "args": ["mcp"]\n+ }\n' : '- "oso": { … }\n' },
      { path: `/home/me/.${agent}/AGENTS.md`, changed: true, diff: install ? "+ <!-- oso:begin -->\n+ If the environment variable `OSO=1` is set, call `oso_context` first.\n+ <!-- oso:end -->\n" : "- <!-- oso:begin --> … <!-- oso:end -->\n" },
    ],
    integrationApply: async (agent, install) => {
      (connected as Record<string, boolean>)[agent] = install;
      return [];
    },
    onApproval: (f) => void (approvalHandler = f),
    approvalAnswer: async (id, decision) => {
      approvalAnswers.push([id, decision]);
      return true;
    },
    agentActivity: async () => activity.map(({ output: _o, ...a }: Activity & { output?: string }) => ({ ...a, has_output: !!(a.has_output || _o) })),
    activityDetail: async (id) => {
      const a = activity.find((x) => x.id === id) as (Activity & { output?: string }) | undefined;
      return a ? { command: a.command, output: a.output ?? a.preview, cut: false } : null;
    },
    localUser: async () => "seya",
    agentStatus: async (pending) => {
      // as the engine does: a request waiting makes its terminal "needs you", also when nothing else is known about the agent there
      const rows: AgentStatusRow[] = agents.map((a) => ({ ...a, ...(pending.includes(a.sid) ? { state: "needs_you" as const, source: "approval" as const } : {}) }));
      for (const sid of pending) if (sessions.has(sid) && !rows.some((r) => r.sid === sid)) rows.push({ sid, kind: "agent", name: "AI agent", pid: null, cwd: "", folder: "", branch: "", state: "needs_you", since: Date.now(), source: "approval" });
      return rows;
    },
    agentAck: async (sid) => void agentAcks.push(sid),
    agentKinds: async () => agentKinds.map((k) => ({ ...k })),
    agentKindsSave: async (kinds) => void agentKinds.splice(0, agentKinds.length, ...kinds.map((k) => ({ ...k }))),
    worktreeAdd: async (repo, branch) => {
      if (knobs.noRepo) throw new Error(`${repo} is not inside a git repository`);
      worktrees.push([repo, branch]);
      return `${repo}.worktrees/${branch.replace(/[^a-zA-Z0-9._-]/g, "-")}`;
    },
    focusUpdate: async (selected, open, active) => void focusSent.push({ selected, open, active }),
    soundLoad: async (kind, value) => {
      if (kind === "file" && value.includes("missing")) throw new Error("could not read that file");
      return { mime: "audio/ogg", b64: "T2dnUw==" };
    },
    notifyAgent: async (title, body, sid) => void notices.push([`${title}|${sid}`, body]),
    onNotifyClick: (f) => void (notifyClick = f),
    auditRecent: async () => [
      { ts: 1_800_000_000, pane: 2, agent: "claude-code", action: "run", host: "web-1", command: "df -h", decision: "allowed_without_asking", exit_code: 0, note: "" },
      { ts: 1_800_000_060, pane: 2, agent: "claude-code", action: "run", host: "web-1", command: "rm -rf /tmp/x", decision: "denied", exit_code: null, note: "" },
    ] as AuditEntry[],
    secretList: async () => {
      if (!vault.unlocked) throw new Error("the vault is locked");
      return knobs.empty ? [] : [...vaultSecrets.values()].map((x) => ({ ...x.meta }));
    },
    secretSave: async (meta, value) => {
      if (!vault.unlocked) throw new Error("the vault is locked");
      if (!meta.name.trim()) throw new Error("a secret needs a name");
      const cur = vaultSecrets.get(meta.id);
      if (!cur && !value) throw new Error("a secret needs a value");
      const id = cur ? meta.id : nextSecret++;
      const updated = value || !cur ? Math.floor(Date.now() / 1000) : cur.meta.updated;
      vaultSecrets.set(id, { meta: { ...meta, id, name: meta.name.trim(), updated }, value: value || cur!.value });
      return id;
    },
    secretDelete: async (id) => {
      vaultSecrets.delete(id);
      for (const [k, v] of [...links]) {
        if (v !== id) continue;
        links.delete(k);
        const s = servers.find((x) => x.id === Number(k.split(":")[0]));
        if (s) k.endsWith(":s") ? (s.has_sudo = false) : (s.has_secret = false);
      }
    },
    secretReveal: async (id) => {
      if (!vault.unlocked) throw new Error("the vault is locked");
      const x = vaultSecrets.get(id);
      if (!x) throw new Error("this secret no longer exists");
      return x.value;
    },
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
      folders.push({ id, name, parent, expanded: true, profile: null });
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
    open: async (serverId, _rows, _cols, useVault = true, opts) => {
      const s = servers.find((x) => x.id === serverId);
      if (serverId && !s) throw new Error("This server no longer exists");
      if (s?.auth === "vaultkey" && !vault.unlocked) throw new VaultLockedError();
      const wanted = s?.network?.startsWith("tailscale:") ? s.network.slice(10) : "";
      if (wanted && !networkOk.delete(serverId)) {
        const now = tools.tailscale.accounts.find((a) => a.active);
        if (!now || ![now.id, now.name, now.detail].includes(wanted)) throw new Error(`network:${wanted}|${now?.name ?? "none"}`);
      }
      if (useVault && s?.has_secret && !vault.unlocked) throw new VaultLockedError();
      const local = activeProfile === "default" ? "Local" : `Local · ${profiles.find((p) => p.id === activeProfile)?.name ?? activeProfile}`;
      const space = opts?.space != null ? spaces.find((x) => x.id === opts.space) : undefined;
      if (opts?.space != null && !space) throw new Error("this Space no longer exists");
      const title = serverId === 0 && space ? `${local} · ${space.name}` : (s?.name ?? local);
      return make(title, serverId, { ...(space ? { space: space.id } : {}), ...(opts?.cwd ? { cwd: opts.cwd } : {}) });
    },
    spaceList: async () => JSON.parse(JSON.stringify(spaces)),
    spaceSave: async (space) => {
      const clean = { ...JSON.parse(JSON.stringify(space)), name: space.name.trim() } as Space;
      if (!clean.name) throw new Error("a Space needs a name");
      if (clean.id === 0) {
        clean.id = nextSpace++;
        spaces.push(clean);
      } else {
        const at = spaces.findIndex((x) => x.id === clean.id);
        if (at < 0) throw new Error("this Space no longer exists");
        spaces[at] = clean;
      }
      return clean.id;
    },
    spaceDelete: async (id) => void spaces.splice(0, spaces.length, ...spaces.filter((x) => x.id !== id)),
    spaceMigration: async () => {
      const m = migration.value;
      migration.value = null;
      return m;
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
    setForwards: async (serverId, forwards) => {
      const s = servers.find((x) => x.id === serverId);
      if (!s) throw new Error("this server no longer exists");
      let id = Math.max(0, ...forwards.map((f) => f.id)) + 1;
      s.forwards = forwards.map((f): Forward => ({ ...f, name: f.name.trim(), id: f.id || id++ }));
      for (const k of [...tunnelState.keys()]) if (k.startsWith(`${serverId}:`) && !s.forwards.some((f) => k === `${serverId}:${f.id}`)) tunnelState.delete(k);
    },
    tunnelStart: async (serverId, forwardId, useVault = true) => {
      const s = servers.find((x) => x.id === serverId);
      if (useVault && s?.has_secret && !vault.unlocked) throw new VaultLockedError();
      const key = `${serverId}:${forwardId}`;
      started.push([serverId, forwardId]);
      const busy = s?.forwards.find((f) => f.id === forwardId)?.bind_port === 1;
      tunnelState.set(key, { server_id: serverId, forward_id: forwardId, state: "starting", message: "" });
      setTimeout(() => {
        const cur = tunnelState.get(key);
        if (cur) tunnelState.set(key, busy ? { ...cur, state: "failed", message: "That port is already in use on this computer: pick another one." } : { ...cur, state: "up" });
      }, 400);
    },
    tunnelStop: async (serverId, forwardId) => void tunnelState.delete(`${serverId}:${forwardId}`),
    tunnels: async () => [...tunnelState.values()],
    transferStart: async (serverId, upload, local, remote, useVault = true) => {
      const s = servers.find((x) => x.id === serverId);
      if (useVault && s?.has_secret && !vault.unlocked) throw new VaultLockedError();
      const id = nextTransfer++;
      const t: TransferStatus = { id, server_id: serverId, upload, what: local.length > 1 ? `${local.length} files` : (upload ? local[0] : remote).split("/").pop() ?? "", state: "running", percent: 10, message: "" };
      started.push({ transfer: id, serverId, upload, local, remote });
      transferList.push(t);
      setTimeout(() => (t.percent = 60), 300);
      setTimeout(() => {
        t.state = remote.includes("nothing") ? "failed" : "done";
        t.message = t.state === "failed" ? "scp: No such file or directory" : "";
        t.percent = t.state === "done" ? 100 : null;
      }, 700);
      return id;
    },
    transfers: async () => transferList.map((t) => ({ ...t })),
    transferClear: async (id) => void transferList.splice(transferList.findIndex((t) => t.id === id), 1),
    onFileDrag: (f) => void (fileDrag = f),
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
