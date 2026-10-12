// The window: sidebar, screens, tabs and mosaic, the dialogs and the hotkeys, wired to the workspace and the engine.

import { AccountsView, Backend, Space, SpaceSettings, SpaceTab, Folder, SecretMeta, Server, SessionEvent, SessionInfo, TreeData, TunnelStatus, VaultLockedError, VaultStatus, b64 } from "./api";
import { MenuItem, clear, confirmBox, contextMenu, el, field, makeSelect, modalOpen, openModal, promptBox, toast } from "./dom";
import { looksReady, withoutPanes } from "./layout";
import { SpacesCtx, editSpace, newSpace, renderSpaces } from "./spaces_ui";
import { emptySpace, openSpaces, tabFor, withTab } from "./spaces";
import { DEFAULTS, LABELS, MODIFIERS, buildBindings, comboText, matchAction, modifierOf, parseCombo, resolveMod } from "./hotkeys";
import { icon, logoMark, serverColor } from "./icons";
import { Transfers, filesDialog } from "./files_ui";
import { parseHostKeyWarning } from "./hostkey";
import { Pane } from "./pane";
import { MAX_TRIES, fatalReason, retryDelay, worthRetrying } from "./reconnect";
import { Id, Rect, Side, autoSide, dockSide, dockZone, leaves } from "./tree";
import { accountsRows } from "./accounts_ui";
import { connectAiDialog } from "./ai_ui";
import { shortcutsSheet } from "./shortcuts_ui";
import { State as StartState, StepId, startCard } from "./start_ui";
import { versionLabel } from "./version";
import { ApprovalQueue, auditDialog } from "./agents_ui";
import { AgentRow, RightPanel } from "./rpanel";
import { AgentState, AgentStatus, doneByQuiet, effectiveState, guessFromScreen, nextAttention, noticesFor, tabDot } from "./agents";
import { LaunchCtx, agentKindsDialog, newAgentDialog } from "./launch_ui";
import { ActionCtx, actionDef, applicable } from "./actions";
import { Item, loadRecent, pushRecent, saveRecent } from "./palette";
import { openPalette } from "./palette_ui";
import { SoundPlayer } from "./sound";
import { soundsDialog } from "./sounds_ui";
import { LEFT_DEFAULT, RAIL_WIDTH, SECTIONS, Section, clampLeft, clickSection, readSection, store, storeSet } from "./chrome";
import { importDialog } from "./import_ui";
import { envImportDialog, protonImportDialog, showCode } from "./importers_ui";
import { accountsAsk, generalDialog, spaceBar } from "./space_bar";
import { KIND_LABEL, secretForm, secretsDialog } from "./secrets_ui";
import { describe as describeTunnel } from "./tunnels";
import { switchOn, tunnelForm, tunnelsDialog } from "./tunnels_ui";
import { Watch, noteInput, noteOutput, startWatch, verdict } from "./watch";
import { Place, Workspace } from "./workspace";

const LOCAL = 0;

interface Drag {
  sid: Id;
  fromBar: boolean;
  startX: number;
  startY: number;
  active: boolean;
  target: { sid: Id | null; side: Side | null } | null;
  toBar: boolean;
}

export class App {
  ws = new Workspace();
  tree: TreeData = { folders: [], servers: [], proxies: [], active_space: null, view_all: false };
  /** What General fixes (accounts, proxy, cloud): the Spaces carry their own. */
  private general: SpaceSettings | null = null;
  vault: VaultStatus = { exists: false, unlocked: false };
  panes = new Map<Id, Pane>();
  private early = new Map<Id, SessionEvent[]>();
  private bindings = buildBindings(null).bindings;
  userKeys: Record<string, unknown> | null = null;
  /** The Spaces (saved by the engine). */
  spaces: Space[] = [];
  focusArea: "terminal" | "sidebar" = "terminal";
  selected: { kind: "folder" | "server"; id: number } | null = null;
  filter = "";
  private drag: Drag | null = null;
  /** True for an instant after a drag ended, so the click that follows it does nothing. */
  private justDragged = false;
  private saveTimer = 0;
  /** When each session started (to tell a connection that dropped from one that never worked). */
  private since = new Map<Id, number>();
  /** Terminals waiting to be connected again by themselves: which try they are on and the clock running. */
  private retry = new Map<Id, { attempt: number; timer: number }>();
  /** Terminals that will say when they are done, and the ones that already did (until you look at them). */
  private watches = new Map<Id, Watch>();
  private alerts = new Set<Id>();
  private transfers!: Transfers;
  /** Which servers are production, kept for those the Space filter hides while their terminal is open. */
  private prod = new Map<number, boolean>();
  private tunnelStatus = new Map<string, TunnelStatus>();
  private accountsView: AccountsView | null = null;
  /** The secrets (names and types only) while the vault is unlocked: the left panel lists them. */
  private secretsMeta: SecretMeta[] | null = null;
  /** The secret that is this computer's sudo password (null: none, or the vault is locked). */
  private localSudoId: number | null = null;
  /** How many tunnels are on (the mark on the Tunnels button). */
  private tunnelsUp = 0;
  private fileOver: Pane | null = null;
  /** What AI agents ask, waiting for the person. */
  private approvals!: ApprovalQueue;
  /** The panel on the right: open sessions and what agents asked. */
  private rpanel!: RightPanel;
  /** When the window opened each session (ms), kept in the session's meta so it survives a restart of the window. */
  private opened = new Map<Id, number>();
  /** Who is using this computer, for the label of a local terminal. */
  private localUser = "";

  snippets: { name: string; command: string }[] = [];

  // elements
  private rail = el("nav", { id: "rail", "aria-label": "Sections" });
  private side = el("div", { id: "side" });
  private searchWrap = el("div", { class: "search" });
  /** Which section the left panel shows (`""`: the panel is closed). */
  private leftSection: Section = readSection();
  private leftWidth = clampLeft(Number(store("oso.ui.left.w") ?? LEFT_DEFAULT));
  /** Focus mode: no rail and no panels, only the tabs and the terminals. */
  private focusMode = false;
  private main = el("div", { id: "main" });
  private topbar = el("div", { id: "topbar" });
  private stage = el("div", { id: "stage" });
  private hint = el("div", { id: "dock-hint" });
  private tabsEl = el("div", { class: "tabs" });
  private toolsEl = el("div", { class: "tools" });
  private treeEl = el("div", { class: "tree", tabindex: "0" });
  private spaceEl = el("div", { class: "space-slot" });
  private search = el("input", { type: "search", placeholder: "Search servers", autocomplete: "off", spellcheck: "false" });
  private dividers: HTMLElement[] = [];

  constructor(
    private be: Backend,
    private root: HTMLElement,
  ) {
    this.sound = new SoundPlayer(be);
  }

  // ------------------------------------------------------------ start

  async start() {
    this.root.append(this.rail, this.side, this.main);
    this.topbar.append(this.tabsEl, this.toolsEl);
    this.main.append(this.topbar, this.stage);
    this.stage.append(this.hint);
    this.buildSidebar();
    this.be.onSession((e) => this.onSession(e));
    this.approvals = new ApprovalQueue({
      be: this.be,
      openInTerminal: async (r) => {
        // a command for a server goes to a terminal of that server; anything else to a local one
        const host = this.tree.servers.find((s) => s.name === r.host);
        const before = new Set(this.ws.tabs.keys());
        await this.openServer(host && r.action === "run" ? host.id : LOCAL);
        const sid = [...this.ws.tabs.keys()].find((k) => !before.has(k));
        if (sid != null) {
          // typed, not sent: the person looks at it and presses Enter (after the shell has shown its prompt)
          await new Promise((done) => setTimeout(done, 300));
          await this.be.write(sid, r.command);
          this.focusActive();
        }
      },
    });
    this.be.onApproval((r) => this.approvals.push(r));
    this.rpanel = new RightPanel({
      be: this.be,
      queue: this.approvals,
      agents: () => this.agentRows(),
      newAgent: () => this.newAgent(null),
      focus: (sid) => this.focusSession(sid),
      tabLabel: (pane) => this.tabLabel(pane),
      focusMode: () => this.focusMode,
      isProdHost: (name) => this.tree.servers.some((s) => s.name === name && this.isProd(s.id)),
      keyText: (a) => this.keyText(a),
    });
    this.rpanel.attach(this.root);
    // with the panel open the requests are answered there; closed (or in focus mode) they come in a window
    this.rpanel.onOpenChange(() => this.syncInline());
    this.applyChrome();
    void this.be.localUser().then((u) => (this.localUser = u)).catch(() => {});
    this.rpanel.refresh().catch(() => {});
    window.setInterval(() => void this.rpanel.refresh(), 2000);
    // which agents run in the terminals, and what each is doing; a notice when one needs you and oso is not in front
    this.be.onNotifyClick((sid) => this.focusSession(sid));
    window.setInterval(() => void this.pollAgents(), 1500);
    // `oso ssh <host>`, `oso host add`, `oso unlock` on the command line: the window follows
    this.be.onCli((e) => {
      if (e.event === "open" && e.data?.server_id != null) void this.openServer(e.data.server_id);
      else if (e.event === "refresh") void this.refreshAll();
    });
    // the vault also locks on its own (idle, the screen locking, the command line): the lock on the screen follows
    window.setInterval(() => void this.refreshVaultMark(), 4000);
    this.transfers = new Transfers(this.be);
    this.be.onFileDrag((e) => this.onFileDrag(e));
    window.setInterval(() => void this.pollTunnels(), 3000);
    await this.loadKeys();
    this.snippets = (await this.readJson<{ name: string; command: string }[]>("snippets.json")) ?? [];
    await this.refreshSpaces();
    // the saved groups of an older version became Spaces when the engine started: say so once
    const migrated = await this.be.spaceMigration().catch(() => null);
    if (migrated && (migrated.imported.length || migrated.skipped.length)) {
      const n = migrated.imported.length;
      toast(`Your saved groups are Spaces now (${n} moved${migrated.skipped.length ? `, ${migrated.skipped.length} could not be read: ${migrated.skipped.join(", ")}` : ""}). Find them under Spaces on the left.`, 9000);
    }
    const pm = await this.be.spaceProfileMigration().catch(() => null);
    if (pm) toast(`Your profiles are Spaces now (${pm.spaces.join(", ") || "none"}). What was in no profile is General. A copy of the old files is in ${pm.backup}.`, 12000);
    await this.refreshTree();
    this.vault = await this.be.vaultStatus();
    void this.refreshAccounts();
    void this.refreshSecrets();
    void this.refreshAi();

    window.addEventListener("keydown", (e) => this.onKey(e), true);
    window.setInterval(() => this.watchTick(), 1000);
    window.addEventListener("resize", () => this.layout());
    new ResizeObserver(() => this.layout()).observe(this.stage);
    this.ws.onChange(() => {
      this.render();
      this.scheduleSave();
    });

    // What was left running, and where it was.
    const live = await this.be.restore(this.guessRows(), this.guessCols()).catch(() => [] as SessionInfo[]);
    this.measure();
    live.forEach((s) => this.since.set(s.sid, Date.now() - 60000));
    live.forEach((s) => this.opened.set(s.sid, Number(s.meta.opened) || Date.now()));
    const saved = await this.readJson<ReturnType<Workspace["save"]>>("workspace.json");
    this.ws.restore(
      saved,
      live.map((s) => ({ sid: s.sid, title: s.title, serverId: s.server_id, screen: Number(s.meta.screen ?? 0), ...(s.meta.space != null ? { space: Number(s.meta.space) } : {}), ...(s.meta.cwd ? { cwd: String(s.meta.cwd) } : {}) })),
    );
    this.render();
    this.toastKeyProblems();
    if (await this.be.readConfig("selftest.flag")) void this.selfTest();
  }

  /** A check of the whole path (window, engine, background server, a real shell): when selftest.flag is in the
   *  config folder, the window opens a terminal, types into it and writes what it saw to selftest.out. */
  private async selfTest() {
    const report: string[] = [];
    const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));
    const text = (sid: number) => {
      const b = this.panes.get(sid)!.term.buffer.active;
      let s = "";
      for (let i = 0; i < b.length; i++) s += (b.getLine(i)?.translateToString(true) ?? "") + "\n";
      return s;
    };
    try {
      const want = ((await this.be.readConfig("selftest.flag")) ?? "").trim();
      if (want === "metrics") {
        // where everything is, in the real window: for finding out why something is drawn in the wrong place
        await wait(1500);
        const box = (sel: string) => {
          const e = document.querySelector(sel) as HTMLElement | null;
          if (!e) return `${sel}: none`;
          const r = e.getBoundingClientRect();
          return `${sel}: x=${Math.round(r.x)} y=${Math.round(r.y)} w=${Math.round(r.width)} h=${Math.round(r.height)} scrollTop=${e.scrollTop}`;
        };
        report.push(`window ${window.innerWidth}x${window.innerHeight} dpr=${window.devicePixelRatio} doc.scrollTop=${document.documentElement.scrollTop} body.scrollTop=${document.body.scrollTop} scrollY=${window.scrollY} docH=${document.documentElement.scrollHeight}`);
        for (const sel of ["#app", "#rail", "#side", "#main", "#topbar", "#stage", "#rpanel", "#rtoggle", ".space-slot", ".tree"]) report.push(box(sel));
        await this.be.writeConfig("selftest.out", report.join("\n"));
        return;
      }
      if (want.startsWith("open:")) {
        // opens the Space of that name and leaves the window as it is: for looking at it (or taking a picture of it)
        await wait(1500);
        const sp = this.spaces.find((x) => x.name === want.slice(5));
        if (sp) await this.openSpace(sp);
        await wait(4000);
        await this.be.writeConfig("selftest.out", sp ? `opened ${sp.name}` : `no Space called ${want.slice(5)}`);
        return;
      }
      if (want === "sound") {
        await wait(1500);
        await this.sound.test("finished");
        await wait(1500);
        report.push(`finished: ${this.sound.lastError || "played without error"}`);
        await this.sound.testWith("needs", { ...this.sound.settings, needs: { kind: "system" } });
        await wait(1500);
        report.push(`system sound: ${this.sound.lastError || "played without error"}`);
        const a = new Audio();
        report.push(`canPlayType ogg vorbis: "${a.canPlayType('audio/ogg; codecs="vorbis"')}", wav: "${a.canPlayType("audio/wav")}", mp3: "${a.canPlayType("audio/mpeg")}"`);
        try {
          const url = (await import("../../assets/sounds/pepSound2.ogg")).default;
          const r = await fetch(url);
          report.push(`url ${url} -> ${r.status} ${r.headers.get("content-type")} ${(await r.arrayBuffer()).byteLength} bytes`);
        } catch (e) {
          report.push(`fetch failed: ${e}`);
        }
        await this.be.writeConfig("selftest.out", report.join("\n"));
        return;
      }
      if (want === "agentdone") {
        // a program called claude (no hooks) prints for a few seconds and goes quiet: the sound must come, once
        await this.openServer(LOCAL);
        const sid = this.ws.screen.active!;
        await wait(1500);
        await this.openServer(LOCAL); // another tab, so that the first one is not the one on show
        this.onTerminalData(sid, "exec -a claude bash -c 'for i in 1 2 3 4 5 6; do echo working $i; sleep 1; done; sleep 90'\r");
        await wait(28000);
        report.push(`sounds played: ${JSON.stringify(this.sound.played)}`);
        report.push(`states: ${JSON.stringify(this.agentRows().map((r) => [r.sid, r.state, r.source]))}`);
        await this.be.writeConfig("selftest.out", report.join("\n"));
        return;
      }
      if (want === "settings") {
        this.settingsDialog();
        await wait(500);
        document.querySelector('[data-role="modifier"] .sel-btn')?.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, cancelable: true }));
        await wait(300);
        report.push(`popup visible: ${getComputedStyle(document.querySelector('[data-role="modifier"] .sel-pop')!).display}`);
        await this.be.writeConfig("selftest.out", report.join("\n"));
        return;
      }
      if (want === "features") {
        // groups, broadcast and the "done" watch, with local terminals in the real window
        await this.openServer(LOCAL);
        this.ws.prepareSplit("right");
        await this.openServer(LOCAL);
        const sh = this.ws.shape()!;
        const tab = tabFor("selftest", sh.shape, false, [{ server: LOCAL, name: "Local", cwd: "" }, { server: LOCAL, name: "Local", cwd: "" }]);
        tab.panes[0].command = "echo group-ok-$((6*7))";
        const spaceId = await this.be.spaceSave({ ...emptySpace(), name: "selftest", tabs: [tab] });
        await this.refreshSpaces();
        for (const id of [...this.ws.tabs.keys()]) await this.closeSession(id);
        await this.openSpace(this.spaces.find((x) => x.id === spaceId)!);
        await wait(4000);
        const sids = this.ws.tabsHere().map((t) => t.sid);
        report.push(`group: ${this.ws.screen.groups.length} tab, ${sids.length} terminals, mosaic ${this.ws.screen.mode}`);
        report.push(`start command typed: ${text(sids[0]).includes("group-ok-42")}`);
        this.ws.setBroadcast(true);
        this.onTerminalData(sids[0], "echo bc-$((1+1))\r");
        await wait(1500);
        report.push(`broadcast reached both: ${sids.every((s) => text(s).includes("bc-2"))}`);
        this.ws.setBroadcast(false);
        this.ws.focus(sids[0]);
        this.toggleWatch(sids[0]);
        report.push(`watching: ${this.watches.has(sids[0])}`);
        this.onTerminalData(sids[0], "sleep 3; echo watch-done\r");
        await wait(12000);
        report.push(`watch spoke and switched itself off: ${!this.watches.has(sids[0])}`);
        report.push(`tunnels: ${JSON.stringify(await this.be.tunnels())} transfers: ${JSON.stringify(await this.be.transfers())}`);
        report.push(`terminal 1 last lines: ${JSON.stringify(text(sids[0]).trim().split("\n").slice(-3))}`);
        await this.be.writeConfig("selftest.out", report.join("\n"));
        return;
      }
      if (want === "mosaic") {
        await this.openServer(LOCAL);
        this.ws.prepareSplit("right");
        await this.openServer(LOCAL);
        this.ws.prepareSplit("bottom");
        await this.openServer(LOCAL);
        const first = this.ws.screen.active!;
        await openTabAlone(this);
        this.ws.focus(first);
        await wait(1500);
        report.push(`tabs ${this.ws.screen.groups.length}, mode ${this.ws.screen.mode}, numbers ${[...document.querySelectorAll(".pane.show .title .num")].map((n) => n.textContent).join(",")}`);
        await this.be.writeConfig("selftest.out", report.join("\n"));
        return;
      }
      const named = this.tree.servers.find((s) => s.name === want);
      if (named) {
        await this.openServer(named.id);
        await wait(7000);
        const a = this.ws.screen.active;
        report.push(`server ${want}: ${a != null ? text(a).trim().split("\n").slice(-4).join("\n") : "no terminal"}`);
        if (a != null) {
          const p = this.panes.get(a)!;
          const t = p.term;
          const b = t.buffer.active;
          const r = (p.root.querySelector(".xterm") as HTMLElement).getBoundingClientRect();
          const rows = p.root.querySelector(".xterm-rows") as HTMLElement | null;
          report.push(`term ${t.cols}x${t.rows} baseY=${b.baseY} viewportY=${b.viewportY} cursor=${b.cursorX},${b.cursorY} length=${b.length}`);
          report.push(`xterm box ${Math.round(r.width)}x${Math.round(r.height)}; rows element: ${rows ? "yes " + rows.children.length + " rows, first text: " + JSON.stringify((rows.children[0] as HTMLElement | undefined)?.innerText?.slice(0, 40)) : "none"}`);
          report.push(`renderer: ${p.root.querySelector("canvas") ? "canvas" : "dom"}; screen: ${p.root.querySelector(".xterm-screen") ? "yes" : "no"}`);
          report.push(`font: ${getComputedStyle(p.root.querySelector(".xterm-rows") ?? p.root).fontFamily} ${getComputedStyle(p.root.querySelector(".xterm-rows") ?? p.root).fontSize} color ${getComputedStyle(p.root.querySelector(".xterm-rows") ?? p.root).color}`);
          for (let i = 0; i < Math.min(t.rows, 6); i++) report.push(`row ${i}: ${JSON.stringify(b.getLine(b.viewportY + i)?.translateToString(true).slice(0, 60))}`);
        }
        await this.be.writeConfig("selftest.out", report.join("\n"));
        return;
      }
      await this.openServer(LOCAL);
      const sid = this.ws.screen.active;
      report.push(`opened: ${sid != null}`);
      await wait(1000);
      await this.be.write(sid!, "echo oso-selftest-$((6*7))\r");
      await wait(1500);
      report.push(`output ok: ${text(sid!).includes("oso-selftest-42")}`);
      const t = this.panes.get(sid!)!.term;
      report.push(`size: ${t.cols}x${t.rows}`);
      this.ws.prepareSplit("right");
      await this.newTerminal();
      await wait(1000);
      report.push(`mosaic panes: ${document.querySelectorAll(".pane.show.mosaic").length}`);
    } catch (e) {
      report.push(`error: ${String(e)}`);
    }
    await this.be.writeConfig("selftest.out", report.join("\n"));
  }

  private async readJson<T>(name: string): Promise<T | null> {
    try {
      const t = await this.be.readConfig(name);
      return t ? (JSON.parse(t) as T) : null;
    } catch {
      return null;
    }
  }

  /** The shortcut of an action as it is set now ("Alt+M", or "Super+M" if the main key was changed). */
  keyText(action: string): string {
    return comboText(this.userKeys, action).split(" / ")[0];
  }

  private async loadKeys() {
    this.userKeys = await this.readJson<Record<string, unknown>>("hotkeys.json");
    this.bindings = buildBindings(this.userKeys).bindings;
  }
  private toastKeyProblems() {
    const { problems } = buildBindings(this.userKeys);
    if (problems.length) toast(`hotkeys.json: ${problems[0]}${problems.length > 1 ? ` (+${problems.length - 1} more)` : ""}`, 8000);
  }

  private scheduleSave() {
    clearTimeout(this.saveTimer);
    this.saveTimer = window.setTimeout(() => {
      void this.be.writeConfig("workspace.json", JSON.stringify(this.ws.save()));
      this.ws.tabs.forEach((t) => {
        const order = this.ws.screens[t.screen].order.indexOf(t.sid);
        void this.be.setMeta(t.sid, { title: t.title, server_id: t.serverId, screen: t.screen, order: t.screen * 100 + order, opened: this.opened.get(t.sid), ...(t.space != null ? { space: t.space } : {}), ...(t.cwd ? { cwd: t.cwd } : {}) });
      });
    }, 300);
  }

  async refreshTree() {
    this.tree = await this.be.tree();
    for (const s of this.tree.servers) this.prod.set(s.id, s.env_kind === "production");
    this.renderSidebar();
    this.render();
  }

  private async refreshVaultMark() {
    try {
      const v = await this.be.vaultStatus();
      if (v.exists !== this.vault.exists || v.unlocked !== this.vault.unlocked) {
        this.vault = v;
        this.renderRail();
        void this.refreshSecrets();
      }
    } catch {
      /* the next one will do */
    }
  }

  private async refreshAll() {
    await this.refreshTree();
    await this.refreshVaultMark();
    void this.refreshAccounts();
    void this.refreshSecrets();
    void this.refreshAi();
  }

  /** Who is logged in to GitHub and Tailscale (asked of the tools themselves; hidden if they are not installed). */
  async refreshAccounts() {
    this.accountsView = await this.be.accounts().catch(() => null);
    this.renderSidebar();
  }

  /** Is this a production machine? (Its tab and terminal are drawn in red.) */
  private isProd(serverId: number): boolean {
    return this.prod.get(serverId) === true;
  }

  // ------------------------------------------------------------ sessions

  private guessCols() {
    return Math.max(20, Math.floor(this.ws.size.w / 8.4));
  }
  private guessRows() {
    return Math.max(5, Math.floor(this.ws.size.h / 18));
  }

  private measure() {
    const r = this.stage.getBoundingClientRect();
    this.ws.size = { x: 0, y: 0, w: Math.max(100, r.width), h: Math.max(100, r.height) };
  }

  /** Opens a session for a server (or this computer) as a new terminal on this screen. */
  async openServer(serverId: number, useVault = true, place?: Place): Promise<void> {
    this.measure();
    try {
      const info = await this.be.open(serverId, this.guessRows(), this.guessCols(), useVault, this.tree.active_space === null ? undefined : { space: this.tree.active_space });
      if (serverId === LOCAL) localStorage.setItem("oso.openedLocal", "1");
      this.addSession(info, place);
    } catch (e) {
      if (e instanceof VaultLockedError) {
        const choice = await this.unlockDialog(true);
        if (choice === "unlocked") return this.openServer(serverId, true, place);
        if (choice === "without") return this.openServer(serverId, false, place);
        return;
      }
      const m = /^network:(.*)\|(.*)$/.exec((e as Error).message);
      if (m) return this.networkQuestion(serverId, m[1], m[2], useVault, place);
      toast(`Could not open: ${(e as Error).message}`);
    }
  }

  /** The server is on a Tailscale account other than the one in use: switch, connect anyway, or give up. */
  private networkQuestion(serverId: number, wanted: string, current: string, useVault: boolean, place?: Place): Promise<void> {
    const name = this.tree.servers.find((s) => s.id === serverId)?.name ?? "This server";
    return new Promise((resolve) => {
      let done = false;
      const go = async (f: () => Promise<void>) => {
        done = true;
        try {
          await f();
        } catch (e) {
          toast((e as Error).message, 6000);
          return resolve();
        }
        await this.openServer(serverId, useVault, place);
        resolve();
      };
      openModal(
        (close) => {
          const sw = el("button", { class: "btn primary", type: "button", "data-act": "network-switch" }, `Switch to ${wanted}`);
          const any = el("button", { class: "btn", type: "button", "data-act": "network-anyway" }, "Connect anyway");
          const no = el("button", { class: "btn", type: "button" }, "Cancel");
          sw.onclick = () => {
            close();
            void go(() => this.be.accountUse("tailscale", wanted).then(() => this.refreshAccounts()));
          };
          any.onclick = () => {
            close();
            void go(() => this.be.networkAccept(serverId));
          };
          no.onclick = close;
          return el("div", {}, el("h2", {}, `Switch to ${wanted}?`), el("div", {}, `${name} is reached on the Tailscale account ${wanted}, and ${current} is the one in use. Connecting now could go the wrong way or not at all.`), el("div", { class: "actions" }, no, any, sw));
        },
        () => !done && resolve(),
      );
    });
  }

  private addSession(info: SessionInfo, place?: Place) {
    this.since.set(info.sid, Date.now());
    this.opened.set(info.sid, Date.now());
    this.ws.addTab({ sid: info.sid, title: info.title, serverId: info.server_id, ...(info.meta.space != null ? { space: Number(info.meta.space) } : {}), ...(info.meta.cwd ? { cwd: String(info.meta.cwd) } : {}) }, place);
    this.focusArea = "terminal";
    this.ensurePane(info.sid);
    const buffered = this.early.get(info.sid);
    this.early.delete(info.sid);
    buffered?.forEach((e) => this.onSession(e));
  }

  private ensurePane(sid: Id): Pane | undefined {
    const tab = this.ws.tabs.get(sid);
    if (!tab) return undefined;
    let p = this.panes.get(sid);
    if (!p) {
      p = new Pane(sid, {
        onData: (s, d) => this.onTerminalData(s, d),
        onBinary: (s, d) => void this.be.writeBytes(s, b64.encode(Uint8Array.from(d, (c) => c.charCodeAt(0)))),
        onResize: (s, rows, cols) => void this.be.resize(s, rows, cols),
        onClose: (s) => void this.closeSession(s),
        onFocus: (s) => this.focusSession(s),
        onTitlePointerDown: (e, s) => this.startDrag(e, s, false),
        onWatch: (s) => this.toggleWatch(s),
        onContextMenu: (e, s) => this.paneMenu(e, s),
      });
      p.attach(this.stage);
      this.panes.set(sid, p);
    }
    p.setTitle(tab.title);
    return p;
  }

  private onSession(e: SessionEvent) {
    const tab = this.ws.tabs.get(e.sid);
    const pane = this.panes.get(e.sid);
    if (!tab || !pane) {
      // Output can arrive just before the window knows the session.
      const q = this.early.get(e.sid) ?? [];
      q.push(e);
      this.early.set(e.sid, q.slice(-200));
      return;
    }
    switch (e.kind) {
      case "snapshot":
        pane.snapshot(e.replay, e.screen);
        if (e.exit != null) this.sessionEnded(e.sid, e.exit, true);
        break;
      case "output": {
        pane.output(e.data);
        const w = this.watches.get(e.sid);
        if (w) noteOutput(w, Date.now());
        break;
      }
      case "exit":
        this.sessionEnded(e.sid, e.code);
        this.watchEnded(e.sid, e.code);
        break;
      case "lost":
        this.sessionEnded(e.sid, 255);
        break;
      case "sudo":
        if (e.on && this.ws.tabs.get(e.sid)) toast("sudo is asking for a password — Ctrl+Shift+P types the saved one");
        break;
    }
  }

  /** A clean exit closes the terminal, like a browser tab; a failure stays to show the error. */
  private sessionEnded(sid: Id, code: number, atStart = false) {
    if (code === 0) return void this.closeSession(sid, false);
    this.ws.setDead(sid, true);
    const tab = this.ws.tabs.get(sid);
    // a session found already ended when the window opened waits for you: it may have died long ago
    if (!tab || atStart) return;
    // what the terminal printed last decides (a refused login must not be retried): wait until it is drawn
    const pane = this.panes.get(sid);
    void (pane ? pane.written() : Promise.resolve()).then(() => {
      if (!this.ws.tabs.get(sid)?.dead) return;
      const warn = pane ? parseHostKeyWarning(pane.tail(40)) : null;
      if (pane && warn) this.showKeyWarning(sid, pane, warn.host, warn.fingerprint);
      const why = fatalReason(pane?.tail(6) ?? "");
      if (why) return this.stopRetry(sid, `${why} Enter: try again · Alt+W: close`);
      const before = this.retry.get(sid)?.attempt ?? 0;
      const verdict = worthRetrying(code, Date.now() - (this.since.get(sid) ?? 0), before, tab.serverId === LOCAL);
      if (verdict.retry) this.scheduleRetry(sid, verdict.attempt);
      else this.stopRetry(sid);
    });
  }

  // ------------------------------------------------------------ tunnels and files

  private tunnelCtx() {
    return {
      be: this.be,
      servers: () => this.tree.servers,
      refreshTree: () => this.refreshTree(),
      unlock: (offer: boolean) => this.unlockDialog(offer),
    };
  }

  openTunnels() {
    tunnelsDialog(this.tunnelCtx(), () => void this.pollTunnels());
  }

  private async pollTunnels() {
    try {
      const all = await this.be.tunnels();
      const up = all.filter((t) => t.state !== "failed").length;
      if (up !== this.tunnelsUp) {
        this.tunnelsUp = up;
        this.renderRail();
      }
      const next = new Map(all.map((t) => [`${t.server_id}:${t.forward_id}`, t]));
      const changed = JSON.stringify([...next]) !== JSON.stringify([...this.tunnelStatus]);
      this.tunnelStatus = next;
      if (changed) this.renderSidebar();
    } catch {
      /* the next one will do */
    }
  }

  /** Files of the server of the terminal you are in (or of the one under the pointer when files were dropped). */
  openFiles(sid: Id | null = this.ws.screen.active, dropped: string[] = []) {
    const tab = sid == null ? undefined : this.ws.tabs.get(sid);
    if (!tab) return toast("Open a terminal on a server first");
    const server = this.tree.servers.find((s) => s.id === tab.serverId);
    if (!server) return toast("Files go to a server: this terminal is on this computer");
    filesDialog({ be: this.be, unlock: (o) => this.unlockDialog(o) }, this.transfers, { id: server.id, name: server.name }, dropped);
  }

  /** Files dragged from the file manager over the window: the terminal under them lights up; dropped, it asks where. */
  private onFileDrag(e: { type: "over" | "drop" | "leave"; paths: string[]; x: number; y: number }) {
    const under = (): Pane | null => {
      const at = e.type === "leave" ? null : document.elementFromPoint(e.x, e.y);
      if (!at) return null;
      for (const p of this.panes.values()) if (p.root.classList.contains("show") && p.root.contains(at)) return p;
      return null;
    };
    const pane = under();
    if (this.fileOver && this.fileOver !== pane) this.fileOver.root.classList.remove("filedrop");
    this.fileOver = e.type === "drop" || e.type === "leave" ? null : pane;
    if (e.type === "over") return void pane?.root.classList.add("filedrop");
    pane?.root.classList.remove("filedrop");
    if (e.type !== "drop" || !e.paths.length) return;
    if (!pane) return toast("Drop the files on a terminal: they go to its server");
    this.openFiles(pane.sid, e.paths);
  }

  // ------------------------------------------------------------ "tell me when it is done"

  /** Mod+A and the bell: this terminal will say when its command is done (once), even if you are elsewhere. */
  toggleWatch(sid: Id | null = this.ws.screen.active) {
    const tab = sid == null ? undefined : this.ws.tabs.get(sid);
    if (sid == null || !tab) return;
    if (tab.dead) return toast("This terminal is not connected");
    if (this.watches.has(sid)) {
      this.watches.delete(sid);
      toast("Not watching this terminal");
    } else {
      this.watches.set(sid, startWatch(Date.now()));
      toast("Watching: you will be told when this terminal is done (once)");
    }
    this.alerts.delete(sid);
    this.render();
  }

  private watchTick() {
    const now = Date.now();
    for (const [sid, w] of this.watches) {
      const pane = this.panes.get(sid);
      const v = pane ? verdict(w, now, pane.cursorLine()) : null;
      if (v) this.tellDone(sid, v === "prompt" ? "Done: the terminal is back at its prompt" : `Went quiet: nothing printed for ${Math.round((now - w.lastOutput) / 1000)} s`);
    }
  }

  private watchEnded(sid: Id, code: number) {
    if (this.watches.has(sid)) this.tellDone(sid, code === 0 ? "The session ended" : "The connection ended");
  }

  /** It is done: a toast, a mark on its tab, and a desktop notification if you are not looking at it. */
  tellDone(sid: Id, what: string) {
    this.watches.delete(sid);
    const tab = this.ws.tabs.get(sid);
    if (!tab) return;
    const looking = document.hasFocus() && this.ws.screen.group && leaves(this.ws.screen.group.tree).includes(sid) && this.ws.screen.active === sid;
    if (!looking) this.alerts.add(sid);
    void this.sound.notify("finished", sid, !!looking);
    toast(`${tab.title}: ${what}`, 6000);
    if (!looking) void this.be.notify(tab.title, what).catch(() => {});
    this.render();
  }

  /** A server shows a different key than the saved one: explain it in plain words and offer the fix. */
  private showKeyWarning(sid: Id, pane: Pane, host: string | null, fingerprint: string | null) {
    const tab = this.ws.tabs.get(sid);
    const server = tab ? this.tree.servers.find((s) => s.id === tab.serverId) : undefined;
    const name = host ?? (server ? (server.port === 22 ? server.host : `[${server.host}]:${server.port}`) : null);
    const forget = el("button", { class: "btn primary", type: "button" }, "I reinstalled it: forget the old key");
    const leave = el("button", { class: "btn", type: "button" }, "Leave it");
    const card = el(
      "div",
      { class: "keycard", role: "alertdialog" },
      el("h3", {}, "This server's key changed"),
      el("p", {}, "The key it shows now is not the one saved from the last time. That happens when a server is reinstalled or its address now belongs to another machine."),
      el("p", { class: "warn" }, "If you were not expecting it, do not connect: someone may be listening in between you and the server."),
      fingerprint ? el("p", { class: "fp" }, "New fingerprint: ", el("code", {}, fingerprint)) : "",
      el("div", { class: "actions" }, leave, name ? forget : ""),
    );
    leave.onclick = () => pane.showCard(null);
    forget.onclick = async () => {
      if (!name) return;
      try {
        await this.be.forgetHostKey(name);
      } catch (e) {
        return void toast(`Could not forget the key: ${(e as Error).message}`, 6000);
      }
      pane.showCard(null);
      toast(`Old key of ${name} forgotten. Connecting: you will be asked to accept its new key.`, 6000);
      void this.reconnect(sid);
    };
    pane.showCard(card);
  }

  /** The connection is gone: say so on the terminal, count down, and connect again. */
  private scheduleRetry(sid: Id, attempt: number) {
    this.retry.get(sid) && clearInterval(this.retry.get(sid)!.timer);
    const pane = this.panes.get(sid);
    const end = Date.now() + retryDelay(attempt) * 1000;
    const paint = () => pane?.setBanner(`Connection lost: trying again in ${Math.max(0, Math.ceil((end - Date.now()) / 1000))} s (try ${attempt + 1} of ${MAX_TRIES}) · Enter: now · Esc: stop`);
    const timer = window.setInterval(() => {
      if (Date.now() >= end) {
        clearInterval(timer);
        void this.autoReconnect(sid, attempt);
      } else paint();
    }, 500);
    this.retry.set(sid, { attempt, timer });
    paint();
    this.renderTopbar();
  }

  private stopRetry(sid: Id, message: string | null = null) {
    const r = this.retry.get(sid);
    if (r) clearInterval(r.timer);
    this.retry.delete(sid);
    this.panes.get(sid)?.setBanner(message);
    if (r) this.renderTopbar();
  }

  /** The timer ran out: connect again (never asking for the vault: that waits for you). */
  private async autoReconnect(sid: Id, attempt: number) {
    const tab = this.ws.tabs.get(sid);
    if (!tab || !tab.dead) return this.stopRetry(sid);
    const ok = await this.reconnect(sid, true, true);
    if (ok === "vault") return this.stopRetry(sid, "Connection lost. The vault is locked: unlock it and press Enter to connect again");
    if (ok === "error") return this.stopRetry(sid);
    // the new session carries on the count: if it dies at once, the next try is the one after this
    const now = this.ws.tabs.get(ok);
    if (now) this.retry.set(ok, { attempt: attempt + 1, timer: 0 });
  }

  private onTerminalData(sid: Id, data: string) {
    const tab = this.ws.tabs.get(sid);
    if (!tab) return;
    if (tab.dead) {
      if (data === "\r") {
        this.stopRetry(sid);
        void this.reconnect(sid);
      } else if (data === "\x1b" && this.retry.has(sid)) {
        this.stopRetry(sid);
        toast("Not reconnecting. Enter connects again");
      }
      return;
    }
    const w = this.watches.get(sid);
    if (w) noteInput(w, Date.now());
    const team = this.ws.broadcastTeam(sid);
    if (team) {
      for (const id of team) if (!this.ws.tabs.get(id)?.dead) void this.be.write(id, data);
      return;
    }
    void this.be.write(sid, data);
  }

  /**
   * The same connection again in the terminal that ended, keeping what it showed. Returns the new session's id,
   * or why not: `vault` (it needs the vault unlocked and `auto` does not ask) or `error`.
   */
  async reconnect(sid: Id, useVault = true, auto = false): Promise<Id | "vault" | "error"> {
    const pane = this.panes.get(sid);
    try {
      const info = await this.be.reconnect(sid, pane?.term.rows ?? 24, pane?.term.cols ?? 80, useVault);
      this.ws.rekey(sid, info.sid);
      this.since.delete(sid);
      this.since.set(info.sid, Date.now());
      this.opened.delete(sid);
      this.opened.set(info.sid, Date.now());
      this.alerts.delete(sid);
      this.watches.delete(sid);
      const r = this.retry.get(sid);
      if (r) clearInterval(r.timer);
      this.retry.delete(sid);
      if (pane) {
        this.panes.delete(sid);
        pane.sid = info.sid;
        pane.joinOld = true;
        pane.setBanner(null);
        pane.showCard(null);
        this.panes.set(info.sid, pane);
      }
      const buffered = this.early.get(info.sid);
      this.early.delete(info.sid);
      buffered?.forEach((e) => this.onSession(e));
      return info.sid;
    } catch (e) {
      if (e instanceof VaultLockedError) {
        if (auto) return "vault";
        const c = await this.unlockDialog(true);
        if (c === "unlocked") return this.reconnect(sid, true);
        if (c === "without") return this.reconnect(sid, false);
        return "error";
      }
      toast(`Could not reconnect: ${(e as Error).message}`);
      return "error";
    }
  }

  /** The × of a tab: closes all its terminals (asks first if there are several). */
  private async closeTab(ids: Id[]) {
    if (ids.length > 1 && !(await confirmBox("Close this tab?", `It holds ${ids.length} terminals; all of them will be closed.`, "Close all"))) return;
    for (const id of ids) await this.closeSession(id);
  }

  async closeSession(sid: Id, kill = true) {
    const pane = this.panes.get(sid);
    this.stopRetry(sid);
    this.since.delete(sid);
    this.opened.delete(sid);
    this.watches.delete(sid);
    this.alerts.delete(sid);
    this.ws.closeTab(sid);
    pane?.dispose();
    this.panes.delete(sid);
    if (kill) await this.be.close(sid).catch(() => {});
    this.focusActive();
  }

  focusSession(sid: Id) {
    this.focusArea = "terminal";
    if (this.ws.screen.active !== sid || this.ws.tabs.get(sid)?.screen !== this.ws.current) this.ws.focus(sid);
    else this.focusActive();
  }

  private focusActive() {
    if (modalOpen() || this.focusArea !== "terminal") return;
    const a = this.ws.screen.active;
    if (a != null) this.panes.get(a)?.focus();
  }

  // ------------------------------------------------------------ rendering

  /** The tab of a terminal as the person sees it: "tab 1 · Local · seya". `undefined` when it is closed. */
  private tabLabel(sid: number): string | undefined {
    const t = this.ws.tabs.get(sid);
    if (!t) return undefined;
    const at = this.ws.screen.groups.findIndex((g) => leaves(g.tree).includes(sid));
    const user = t.serverId === LOCAL ? this.localUser : (this.tree.servers.find((s) => s.id === t.serverId)?.user ?? "");
    return [at >= 0 ? `tab ${at + 1}` : "", t.title, user].filter(Boolean).join(" · ");
  }

  // ------------------------------------------------------------ agents

  /** The sounds (what called for you, and when it may sound). */
  readonly sound: SoundPlayer;

  /** Is the window in front with the tab of this terminal on show? Then nothing needs to call for you. */
  private looking(sid: Id): boolean {
    const g = this.ws.screen.group;
    return document.hasFocus() && !!g && leaves(g.tree).includes(sid);
  }

  /**
   * Marks, with a colour, the terminals that call for you until you look at them: green for one that is done (an agent,
   * or a terminal you were watching), amber for an agent that needs you. It is on the terminal and on its tab.
   */
  private applyAlerts() {
    const state = new Map(this.agentRows().map((r) => [r.sid, r.state]));
    for (const tab of this.ws.tabs.values()) {
      const pane = this.panes.get(tab.sid);
      if (!pane) continue;
      const s = state.get(tab.sid);
      pane.root.classList.toggle("call-done", this.alerts.has(tab.sid) || s === "finished");
      pane.root.classList.toggle("call-needs", s === "needs_you");
    }
    for (const el of this.tabsEl.querySelectorAll<HTMLElement>(".tab[data-sid]")) {
      const g = this.ws.screen.groups.find((x) => x.active === Number(el.dataset.sid));
      const ids = g ? leaves(g.tree) : [Number(el.dataset.sid)];
      el.classList.toggle("call-needs", ids.some((id) => state.get(id) === "needs_you"));
      el.classList.toggle("call-done", !ids.some((id) => state.get(id) === "needs_you") && ids.some((id) => this.alerts.has(id) || state.get(id) === "finished"));
    }
  }

  /** What the engine says about the agents, as last asked. */
  private agents: AgentStatus[] = [];
  /** When an agent without hooks was last seen producing output (so that its silence afterwards means it is done). */
  private workedAt = new Map<number, number>();
  /** The state each agent was in the last time, so that only a change is news. */
  private agentSeen = new Map<number, AgentState>();

  /** Asks the engine which agents run where and what they are doing (a request waiting in oso is the exact signal). */
  async pollAgents() {
    const pending = this.approvals.list().map((r) => r.pane);
    const list = await this.be.agentStatus(pending).catch(() => null);
    if (!list) return;
    this.agents = list;
    const rows = this.agentRows();
    // an agent without hooks that worked and went quiet was working a moment ago, as far as news go
    const before = new Map(this.agentSeen);
    for (const sid of this.workedAt.keys()) if (before.get(sid) === "idle") before.set(sid, "working");
    const news = noticesFor(before, rows.map((r) => ({ sid: r.sid, name: r.name, state: r.state, where: r.where })));
    this.agentSeen = new Map(rows.map((r) => [r.sid, r.state]));
    if (!document.hasFocus()) for (const n of news) void this.be.notifyAgent(n.title, n.body, n.sid).catch(() => {});
    for (const n of news) void this.sound.notify(n.event, n.sid, this.looking(n.sid));
    // looking at a tab whose agent finished calms it down
    const on = this.ws.screen.active;
    if (on != null && document.hasFocus() && this.agents.some((a) => a.sid === on && a.state === "finished")) void this.be.agentAck(on).catch(() => {});
    if (on != null && document.hasFocus()) this.workedAt.delete(on);
    this.renderTopbar();
    this.applyAlerts();
    this.rpanel.render();
  }

  /** The rows of the Agents tab: the engine's states, and the screen's guess only where nothing exact is known. */
  private agentRows(): AgentRow[] {
    return this.agents.map((a) => {
      const pane = this.panes.get(a.sid);
      const guess = pane ? guessFromScreen(pane.tailLines(8), pane.lastOutputAt ? Date.now() - pane.lastOutputAt : -1) : null;
      let e = effectiveState(a, guess);
      // no hooks to say "finished": it worked on screen, then went quiet
      if (a.source === "process") {
        if (e.guessed && e.state === "working") this.workedAt.set(a.sid, Date.now());
        else if (e.state === "idle" && doneByQuiet(this.workedAt.get(a.sid), pane?.lastOutputAt ?? 0, Date.now())) e = { state: "finished", guessed: true };
        else if (e.state === "needs_you") this.workedAt.delete(a.sid);
      }
      const space = this.ws.tabs.get(a.sid)?.space;
      return { sid: a.sid, name: a.name, where: this.tabLabel(a.sid) ?? "terminal closed", folder: a.folder, branch: a.branch, state: e.state, since: a.since, space: space != null ? (this.spaces.find((x) => x.id === space)?.name ?? "") : "", source: e.guessed ? "screen" : a.source };
    });
  }

  /** `Alt+N`: the next agent that needs you (the one that has waited longest first); when nobody does, one that finished. */
  jumpToAttention() {
    const rows = this.agentRows().filter((r) => this.ws.tabs.has(r.sid));
    const next = nextAttention(rows, this.ws.screen.active);
    if (next == null) return toast("No agent needs you right now");
    if (!rows.some((r) => r.state === "needs_you")) toast("Nobody needs you: this one finished");
    this.focusSession(next);
  }

  private launchCtx(): LaunchCtx {
    return {
      be: this.be,
      openLocal: (space, cwd) => this.openIn(space, LOCAL, cwd),
      rename: (sid, title) => this.ws.setTitle(sid, title),
      start: (sid, command) => void this.typeWhenReady(sid, command),
    };
  }

  /** "New agent": which folder, which agent, in a worktree or not. */
  newAgent(space: Space | null = null) {
    void newAgentDialog(this.launchCtx(), space);
  }

  /** Tells the engine what the person is looking at, when it changes (ids only; an agent is answered with names). */
  private lastFocus = "";
  private syncFocus() {
    const tabs = [...this.ws.tabs.values()].filter((t) => !t.dead && t.serverId !== LOCAL);
    const selected = this.selected?.kind === "server" ? this.selected.id : null;
    const activeTab = this.ws.screen.active != null ? this.ws.tabs.get(this.ws.screen.active) : undefined;
    const active = activeTab && activeTab.serverId !== LOCAL && !activeTab.dead ? activeTab.serverId : null;
    const open = [...new Set(tabs.map((t) => t.serverId))].sort((a, b) => a - b);
    const sig = JSON.stringify([selected, open, active]);
    if (sig === this.lastFocus) return;
    this.lastFocus = sig;
    void this.be.focusUpdate(selected, open, active).catch(() => {});
  }

  render() {
    this.syncFocus();
    this.rpanel?.render();
    this.layout();
    this.renderTopbar();
    this.renderSidebar();
    this.renderRail();
    requestAnimationFrame(() => this.focusActive());
  }

  /** Puts every terminal where the workspace says. */
  layout() {
    this.measure();
    const ws = this.ws;
    const s = ws.screen;
    const mosaic = s.mode === "mosaic";
    const { boxes, splits } = ws.boxes();
    const place = new Map<Id, Rect>();
    boxes.forEach((b) => place.set(b.id, b.rect));
    const here = s.group ? leaves(s.group.tree) : [];

    for (const tab of ws.tabs.values()) {
      const pane = this.ensurePane(tab.sid);
      if (!pane) continue;
      const rect = tab.screen === ws.current ? place.get(tab.sid) : undefined;
      pane.root.classList.toggle("show", !!rect);
      if (!rect) continue;
      pane.root.classList.toggle("mosaic", mosaic);
      pane.root.classList.toggle("active", tab.sid === s.active);
      if (tab.sid === s.active) this.alerts.delete(tab.sid);
      pane.root.classList.toggle("dead", tab.dead);
      this.applyAlerts();
      pane.root.classList.toggle("prod", this.isProd(tab.serverId));
      const bc = ws.broadcasting && !tab.dead;
      pane.root.classList.toggle("broadcast", bc);
      pane.badge.style.display = bc ? "" : "none";
      pane.setTitle(tab.title);
      pane.setNumber(here.indexOf(tab.sid) + 1);
      pane.setWatching(this.watches.has(tab.sid));
      pane.setColor(serverColor(tab.serverId));
      Object.assign(pane.root.style, { left: `${rect.x}px`, top: `${rect.y}px`, width: `${rect.w}px`, height: `${rect.h}px` });
      pane.forceRefit();
    }

    // the lines between terminals, to drag
    this.dividers.forEach((d) => d.remove());
    this.dividers = [];
    if (mosaic) {
      for (const sp of splits) {
        const d = el("div", { class: `divider ${sp.side ? "v" : "h"}` });
        if (sp.side) Object.assign(d.style, { left: `${sp.at}px`, top: `${sp.whole.y}px`, height: `${sp.whole.h}px` });
        else Object.assign(d.style, { top: `${sp.at}px`, left: `${sp.whole.x}px`, width: `${sp.whole.w}px` });
        d.addEventListener("pointerdown", (e) => this.startResize(e, d, sp.path, sp.side, sp.whole));
        this.stage.append(d);
        this.dividers.push(d);
      }
    }

    // nothing open on this screen: your hosts, to open with a click
    this.stage.querySelector(".home")?.remove();
    if (s.groups.length === 0) this.stage.append(this.homeView());
  }

  /** What an empty screen shows: where to start (until you close it), and where the shortcuts are. */
  private homeView(): HTMLElement {
    const home = el("div", { class: "home" });
    const logo = el("div", { class: "logo", title: "oso" });
    logo.append(logoMark());
    home.append(el("div", { class: "g-head" }, logo, el("div", {}, el("h1", {}, "oso"), el("div", { class: "lead" }, `Nothing is open. Open a server from the list on the left (double-click), or press ${this.keyText("search")} to search.`))));
    if (localStorage.getItem("oso.start") !== "closed") home.append(startCard(this.startState(), (id) => void this.startStep(id), () => {
      localStorage.setItem("oso.start", "closed");
      this.render();
    }));
    const help = el("button", { class: "btn small", type: "button", "data-act": "home-shortcuts" }, `All shortcuts  (${this.keyText("help.open")} or ?)`);
    help.onclick = () => this.shortcutsSheet();
    home.append(el("div", { class: "tip" }, `Secrets live in the encrypted vault (Secrets, left). Everything has a tooltip with its shortcut. Press ${this.keyText("palette.open")} to find any command, server, Space or tab. `, help));
    return home;
  }

  private startState(): StartState {
    return { spaces: this.spaces.length, apiKeys: this.secretsMeta ? this.secretsMeta.filter((m) => m.kind === "api_key").length : null, openedLocal: localStorage.getItem("oso.openedLocal") === "1", aiConnected: this.aiConnected };
  }

  private aiConnected = false;

  private async refreshAi() {
    const st = await this.be.integrationStatus().catch(() => null);
    const on = !!st && (st.claude || st.opencode || st.codex);
    if (on !== this.aiConnected) {
      this.aiConnected = on;
      this.render();
    }
  }

  /** The button of a step of "Getting started". */
  private async startStep(id: StepId) {
    if (id === "space") return void newSpace(this.spacesCtx());
    if (id === "key") return void this.newSecret({ kind: "api_key", agent_visible: true });
    if (id === "terminal") return void this.openServer(LOCAL);
    return void connectAiDialog(this.be, () => void this.refreshAi());
  }

  shortcutsSheet() {
    shortcutsSheet((a) => this.keyText(a), () => this.settingsDialog());
  }

  /** Help: the sheet of shortcuts, getting started, what agents asked, connecting an AI, and which version this is. */
  helpDialog() {
    openModal((close) => {
      const item = (act: string, title: string, text: string, run: () => void) => {
        const b = el("button", { class: "btn way", type: "button", "data-act": act }, el("b", {}, title), el("small", {}, text));
        b.onclick = () => {
          close();
          run();
        };
        return b;
      };
      const done = el("button", { class: "btn primary", type: "button" }, "Done");
      done.onclick = close;
      return el("div", {}, el("h2", {}, "Help"), el("div", { class: "ways" },
        item("help-shortcuts", `Shortcuts  (${this.keyText("help.open")} or ?)`, "Every shortcut, as they are set right now", () => this.shortcutsSheet()),
        item("help-start", "Getting started", "Space, API key, local terminal, connect your AI", () => {
          localStorage.removeItem("oso.start");
          this.render();
          openModal((closeStart) => {
            const card = startCard(this.startState(), (id) => {
              closeStart();
              void this.startStep(id);
            }, closeStart);
            return el("div", { class: "wide" }, card);
          });
        }),
        item("help-ai", "Connect your AI", "Let Claude Code, OpenCode or Codex use oso instead of passwords", () => void connectAiDialog(this.be, () => void this.refreshAi())),
        item("help-activity", "What AI agents asked", "The record of every request and what you answered", () => void auditDialog(this.be)),
      ), el("div", { class: "hint", style: "margin-top:12px" }, `oso ${versionLabel()}`), el("div", { class: "actions" }, done));
    });
  }

  private ibtn(name: string, title: string, onclick: () => void, on = false, extra = ""): HTMLButtonElement {
    const b = el("button", { class: `ib${on ? " on" : ""} ${extra}`.trim(), type: "button", title, "aria-label": title }) as HTMLButtonElement;
    b.append(icon(name, 17));
    b.onclick = onclick;
    return b;
  }

  private renderTopbar() {
    const ws = this.ws;

    // one tab per group: a lone terminal, or several tiled together
    clear(this.tabsEl);
    ws.screen.groups.forEach((g, i) => {
      const ids = leaves(g.tree);
      const t = ws.tabs.get(g.active)!;
      const x = el("button", { class: "x", type: "button", title: ids.length > 1 ? `Close this tab and its ${ids.length} terminals` : `Close (${this.keyText("tab.close")})` });
      x.append(icon("close", 13));
      const count = el("span", { class: "count", title: `${ids.length} terminals together` });
      count.append(icon("grid", 12), String(ids.length));
      const tab = el(
        "div",
        { class: `tab${i === ws.screen.cur ? " on" : ""}${t.dead ? " dead" : ""}${t.dead && this.retry.has(g.active) ? " retrying" : ""}${this.isProd(t.serverId) ? " prod" : ""}`, "data-sid": String(g.active), title: `Tab ${i + 1}${i < 9 ? `  (${this.keyText(`tab.${i + 1}`)})` : ""}` },
        el("span", { class: "num" }, String(i + 1)),
        el("span", { class: "dot", style: `background:${serverColor(t.serverId)}` }),
        el("span", {}, t.title),
      );
      const dot = tabDot(ids.flatMap((id) => this.agentRows().filter((r) => r.sid === id).map((r) => r.state)));
      if (dot) tab.append(el("span", { class: "adot", "data-state": dot, title: { needs_you: "An agent here needs you", working: "An agent here is working", finished: "An agent here finished", idle: "An agent is running here" }[dot] }));
      if (ids.length > 1) tab.append(count);
      const watching = ids.some((id) => this.watches.has(id));
      const alerted = ids.some((id) => this.alerts.has(id));
      if (watching || alerted) {
        const m = el("span", { class: `wm${alerted ? " done" : ""}`, title: alerted ? "A terminal of this tab finished" : "Watching: it will tell you when it is done" });
        m.append(icon("bell", 13));
        tab.append(m);
      }
      if (g.broadcast && ids.length > 1) {
        const m = el("span", { class: "bcm", title: "Typing goes to every terminal of this tab" });
        m.append(icon("broadcast", 13));
        tab.append(m);
      }
      tab.append(x);
      tab.addEventListener("contextmenu", (e) => {
        e.preventDefault();
        ws.focus(g.active);
        contextMenu(e.clientX, e.clientY, [
          { label: "Rename", run: () => void this.renameTab(g.active), hint: this.keyText("tab.rename") },
          { label: "Save in a Space…", run: () => void this.saveTabToSpace(), hint: this.keyText("group.save") },
          { label: ids.length > 1 ? `Close tab (${ids.length} terminals)` : "Close", run: () => void this.closeTab(ids), danger: true, hint: this.keyText("tab.close") },
        ]);
      });
      x.onclick = (e) => {
        e.stopPropagation();
        void this.closeTab(ids);
      };
      tab.addEventListener("pointerdown", (e) => {
        if ((e.target as HTMLElement).closest(".x")) return;
        this.startDrag(e, g.active, true);
      });
      tab.onclick = () => {
        if (!this.justDragged) ws.focus(g.active);
      };
      tab.ondblclick = () => void this.renameTab(g.active);
      this.tabsEl.append(tab);
    });

    // tools: one menu, and what must never be hidden (a broadcast that is on)
    clear(this.toolsEl);
    const bc = ws.broadcasting;
    const chip = this.cloudChip();
    if (chip) this.toolsEl.append(chip, el("span", { class: "sep" }));
    if (bc) this.toolsEl.append(this.ibtn("broadcast", `Typing goes to every terminal of this tab: click to stop  (${this.keyText("broadcast.toggle")})`, () => this.toggleBroadcast(), true, "red"));
    const menu = this.ibtn("more", `Menu: new terminals, broadcast, files, snippets…`, () => {}, false);
    menu.dataset.act = "menu";
    menu.onclick = () => this.openMenu(menu);
    this.toolsEl.append(el("span", { class: "version", title: "The version of oso you are running" }, versionLabel()), menu);
  }

  /** The one menu of the top bar: short, in groups. Everything else is in the palette, or where it belongs (the menu of a terminal, of a tab). */
  private openMenu(anchor: HTMLElement) {
    const hint = (a: string) => this.keyText(a);
    const r = anchor.getBoundingClientRect();
    contextMenu(r.right, r.bottom + 4, [
      { title: "View" },
      { label: this.focusMode ? "Leave focus mode" : "Focus mode: only the terminals", hint: hint("focus.toggle"), run: () => this.setFocusMode(!this.focusMode) },
      { title: "Agents" },
      { label: "Next agent that needs you", hint: hint("attention.next"), run: () => this.run("attention.next") },
      { title: "Tools" },
      { label: "Snippets…", run: () => this.run("snippets.open") },
      { label: "Command palette", hint: hint("palette.open"), run: () => this.run("palette.open") },
      { label: "All commands…", hint: hint("palette.open"), run: () => this.run("palette.open"), foot: true, act: "menu-all" },
    ]);
  }

  /** What applies now, for the registry's `when`. */
  private actionCtx(): ActionCtx {
    const ws = this.ws;
    const active = ws.screen.active;
    const tab = active != null ? ws.tabs.get(active) : undefined;
    const live = !!tab && !tab.dead;
    return {
      terminal: live,
      ssh: live && tab!.serverId !== LOCAL,
      several: !!ws.screen.group && leaves(ws.screen.group.tree).length > 1,
      tabs: ws.screen.groups.length > 0,
      manyTabs: ws.screen.groups.length > 1,
      broadcasting: ws.broadcasting,
      watching: active != null && this.watches.has(active),
      attention: this.agentRows().some((r) => r.state === "needs_you" || r.state === "finished"),
      vaultExists: this.vault.exists,
      vaultUnlocked: this.vault.unlocked,
      focusMode: this.focusMode,
      rightOpen: !!this.rpanel?.open,
    };
  }

  /** A menu made of registered actions (those that apply now), with their keys. */
  private menuOf(ids: string[]): MenuItem[] {
    const c = this.actionCtx();
    const apply = new Set(applicable(c).map((a) => a.id));
    return ids
      .filter((id) => apply.has(id))
      .map((id): MenuItem => {
        const a = actionDef(id)!;
        const checked = id === "broadcast.toggle" ? c.broadcasting : id === "watch.toggle" ? c.watching : undefined;
        return { label: a.label, hint: this.keyText(id) || undefined, run: () => this.run(id), danger: id === "tab.close", checked, act: `act-${id}` };
      });
  }

  /** The menu of a terminal: what can be done with it, here. */
  private paneMenu(e: MouseEvent, sid: Id) {
    this.focusSession(sid);
    const items = this.menuOf(["split.right", "split.down", "broadcast.toggle", "watch.toggle", "files.open", "tab.close"]);
    if (items.length) contextMenu(e.clientX, e.clientY, items);
  }

  // ------------------------------------------------------------ the command palette

  /** Everything the palette can find: the actions that apply, servers, Spaces, secrets (names only) and the open tabs. */
  private paletteItems(): Item[] {
    const pending = this.ws.pending;
    const items: Item[] = applicable(this.actionCtx()).map((a) => ({ id: a.id, kind: "action", label: a.label, icon: a.icon, hotkey: this.keyText(a.id) || undefined, keywords: a.keywords, run: () => this.run(a.id) }));
    const open = (id: number) => () => {
      this.ws.pending = pending;
      void this.openServer(id);
    };
    items.push({ id: String(LOCAL), kind: "server", label: "Local terminal", sub: "this computer", icon: "terminal", run: open(LOCAL) });
    for (const s of this.tree.servers) items.push({ id: String(s.id), kind: "server", label: s.name, sub: `${s.user ? s.user + "@" : ""}${s.host}`, icon: "server", run: open(s.id) });
    for (const sp of this.spaces) items.push({ id: String(sp.id), kind: "space", label: sp.name, sub: "Space", icon: "layers", keywords: sp.notes.slice(0, 80), run: () => void this.openSpace(sp) });
    for (const m of this.secretsMeta ?? []) items.push({ id: String(m.id), kind: "secret", label: m.name, sub: `${KIND_LABEL[m.kind]}${m.env_var ? ` · $${m.env_var}` : ""}`, icon: "key", run: () => void this.vaultDialog() });
    this.ws.screen.groups.forEach((g, i) => {
      const label = this.tabLabel(g.active);
      if (label) items.push({ id: String(g.active), kind: "tab", label, sub: leaves(g.tree).length > 1 ? `${leaves(g.tree).length} terminals` : undefined, icon: "tabs", hotkey: i < 9 ? this.keyText(`tab.${i + 1}`) || undefined : undefined, run: () => this.focusSession(g.active) });
    });
    return items;
  }

  /** `Alt+Shift+Space`: every command, server, Space, secret and tab. `Alt+K` is the same, already filtered to servers. */
  openPalette(only?: ("server")[]) {
    const recent = loadRecent();
    openPalette({
      items: this.paletteItems(),
      recent,
      only,
      placeholder: only ? "Open a server…" : "Type a command, a server, a Space, a tab…",
      onRun: (item) => {
        if (item.kind === "action") saveRecent(pushRecent(recent, item.id));
        item.run();
      },
    });
  }

  /** The cloud account the active Space (or General) points the local terminals at, in red when it is a production one. */
  private cloudChip(): HTMLElement | null {
    const sp = this.tree.active_space === null ? null : this.spaces.find((x) => x.id === this.tree.active_space) ?? null;
    const p: SpaceSettings | null = sp ?? this.general;
    if (!p || (!p.production && !p.aws_profile && !p.kubeconfig)) return null;
    const name = sp ? sp.name : "General";
    const parts = [name, p.aws_profile ? `aws ${p.aws_profile}` : "", p.kubeconfig ? `k8s ${p.kubeconfig.split("/").pop()}` : ""].filter(Boolean);
    return el("span", { class: `cloud-chip${p.production ? " prod" : ""}`, title: p.production ? "A production Space: its terminals point at real machines" : "Where this Space's local terminals point", "data-chip": String(sp?.id ?? 0) }, parts.join(" · "));
  }

  // ------------------------------------------------------------ sidebar

  private buildSidebar() {
    const searchWrap = this.searchWrap;
    searchWrap.append(icon("search", 15), this.search);
    this.search.onfocus = () => (this.focusArea = "sidebar");
    this.search.oninput = () => {
      this.filter = this.search.value.trim().toLowerCase();
      this.renderSidebar();
    };
    const leaveSearch = () => {
      this.search.value = "";
      this.filter = "";
      this.renderSidebar();
      this.search.blur();
      this.focusArea = "terminal";
      this.focusActive();
    };
    this.search.onkeydown = (e) => {
      if (e.key === "Enter") {
        const first = this.visibleServers()[0];
        if (first) void this.openOrFocus(first.id);
        leaveSearch();
      } else if (e.key === "Escape") leaveSearch();
    };
    this.treeEl.addEventListener("pointerdown", () => (this.focusArea = "sidebar"));
    this.treeEl.addEventListener("keydown", (e) => this.treeKey(e));
    const grip = el("div", { id: "lgrip", title: "Drag to change the width" });
    grip.addEventListener("pointerdown", (e) => this.startLeftResize(e, grip));
    this.side.append(this.spaceEl, searchWrap, this.treeEl, grip);
    this.renderRail();
  }

  // ------------------------------------------------------------ the frame: rail, panels, focus mode

  /** Puts the rail and the left panel where they belong (or takes them away in focus mode). */
  private applyChrome() {
    const open = this.leftSection !== "" && !this.focusMode;
    this.root.classList.toggle("focus", this.focusMode);
    this.root.style.setProperty("--rail", this.focusMode ? "0px" : `${RAIL_WIDTH}px`);
    this.leftWidth = clampLeft(this.leftWidth, window.innerWidth, this.rpanel?.open && !this.focusMode ? this.rpanel.width : 0);
    this.root.style.setProperty("--lw", open ? `${this.leftWidth}px` : "0px");
    this.side.hidden = !open;
    this.rail.hidden = this.focusMode;
  }

  /** A click on a rail icon (or its shortcut): that section only, or nothing when it was already the open one. */
  toggleSection(key: Exclude<Section, "">) {
    if (this.focusMode) this.setFocusMode(false);
    this.leftSection = clickSection(this.leftSection, key);
    storeSet("oso.ui.left", this.leftSection);
    this.applyChrome();
    this.renderRail();
    this.renderSidebar();
  }

  setFocusMode(on: boolean) {
    if (this.focusMode === on) return;
    this.focusMode = on;
    this.applyChrome();
    this.renderRail();
    this.syncInline();
    this.render();
  }

  /** The approval window is only needed when the right panel is not there to answer in. */
  private syncInline() {
    this.approvals?.setInline(!!this.rpanel?.open && !this.focusMode);
  }

  private startLeftResize(e: PointerEvent, grip: HTMLElement) {
    e.preventDefault();
    grip.setPointerCapture(e.pointerId);
    document.body.classList.add("resizing-lpanel");
    const move = (m: PointerEvent) => {
      this.leftWidth = clampLeft(m.clientX - RAIL_WIDTH, window.innerWidth, this.rpanel.open ? this.rpanel.width : 0);
      this.applyChrome();
    };
    const up = () => {
      grip.removeEventListener("pointermove", move);
      grip.removeEventListener("pointerup", up);
      grip.removeEventListener("pointercancel", up);
      document.body.classList.remove("resizing-lpanel");
      storeSet("oso.ui.left.w", String(this.leftWidth));
    };
    grip.addEventListener("pointermove", move);
    grip.addEventListener("pointerup", up);
    grip.addEventListener("pointercancel", up);
  }

  /** The thin bar of icons: the five sections on top, the small tools below. */
  private renderRail() {
    clear(this.rail);
    const top = el("div", { class: "rail-top" });
    for (const sec of SECTIONS) {
      const on = this.leftSection === sec.key;
      const b = el("button", { class: `rail-btn${on ? " on" : ""}`, type: "button", "data-rail": sec.key, title: `${sec.label}  (${this.keyText(sec.action)})`, "aria-label": sec.label, "aria-pressed": String(on) });
      b.append(icon(sec.icon, 21));
      b.onclick = () => this.toggleSection(sec.key);
      top.append(b);
    }
    const bottom = el("div", { class: "rail-bottom" });
    const tool = (iconName: string, title: string, act: string, run: () => void, cls = "") => {
      const b = el("button", { class: `rail-btn small ${cls}`.trim(), type: "button", title, "aria-label": title, "data-act": act });
      b.append(icon(iconName, 19));
      b.onclick = run;
      return b;
    };
    const vaultTip = this.vault.unlocked ? "Vault unlocked" : this.vault.exists ? "Vault locked" : "Create the vault";
    bottom.append(
      tool(this.vault.unlocked ? "unlock" : "lock", `${vaultTip}  (${this.keyText("secrets.open")})`, "vault", () => void this.vaultDialog(), this.vault.unlocked ? "open" : ""),
      tool("terminal", `Local terminal  (${this.keyText("local.new")})`, "foot-local", () => void this.openServer(LOCAL)),
      tool("settings", `Settings  (${this.keyText("settings")})`, "foot-settings", () => this.settingsDialog()),
      tool("bell", `Help  (${this.keyText("help.open")})`, "foot-help", () => this.helpDialog()),
    );
    this.rail.append(top, bottom);
  }

  private visibleServers(): Server[] {
    const q = this.filter;
    return this.tree.servers.filter((s) => !q || `${s.name} ${s.host} ${s.user}`.toLowerCase().includes(q));
  }

  private openServerIds(): Set<number> {
    return new Set([...this.ws.tabs.values()].filter((t) => !t.dead).map((t) => t.serverId));
  }

  /** The rows of the tree, flattened: folders open or closed, servers inside. */
  private rows(): { kind: "folder" | "server"; id: number; depth: number }[] {
    const out: { kind: "folder" | "server"; id: number; depth: number }[] = [];
    if (this.filter) {
      this.visibleServers().forEach((s) => out.push({ kind: "server", id: s.id, depth: 0 }));
      return out;
    }
    const walk = (parent: number | null, depth: number) => {
      for (const f of this.tree.folders.filter((f) => f.parent === parent)) {
        out.push({ kind: "folder", id: f.id, depth });
        if (f.expanded) walk(f.id, depth + 1);
      }
      for (const s of this.tree.servers.filter((s) => s.parent === parent)) out.push({ kind: "server", id: s.id, depth });
    };
    walk(null, 0);
    return out;
  }

  private treeKey(e: KeyboardEvent) {
    const rows = this.rows();
    const at = rows.findIndex((r) => this.selected?.kind === r.kind && this.selected.id === r.id);
    const go = (i: number) => {
      const r = rows[Math.max(0, Math.min(rows.length - 1, i))];
      if (r) {
        this.selected = { kind: r.kind, id: r.id };
        this.renderSidebar();
        this.treeEl.querySelector(".row.sel")?.scrollIntoView({ block: "nearest" });
      }
    };
    const cur = rows[at];
    switch (e.key) {
      case "ArrowDown":
        go(at + 1);
        break;
      case "ArrowUp":
        go(at < 0 ? 0 : at - 1);
        break;
      case "ArrowRight":
      case "ArrowLeft":
        if (cur?.kind === "folder") {
          const f = this.tree.folders.find((f) => f.id === cur.id)!;
          if (f.expanded === (e.key === "ArrowLeft")) void this.be.setFolderOpen(f.id, !f.expanded).then(() => this.refreshTree());
        }
        break;
      case "Enter":
        if (cur?.kind === "server") void this.openOrFocus(cur.id);
        else if (cur) {
          const f = this.tree.folders.find((f) => f.id === cur.id)!;
          void this.be.setFolderOpen(f.id, !f.expanded).then(() => this.refreshTree());
        }
        break;
      case "Delete":
        void this.deleteSelected();
        break;
      case "F2":
      case "e":
        if (e.key === "e" && (e.ctrlKey || e.altKey)) return;
        this.editSelected();
        break;
      case "/":
        this.search.focus();
        break;
      default:
        return;
    }
    e.preventDefault();
  }

  renderSidebar() {
    this.syncFocus();
    if (this.side.hidden) return;
    clear(this.spaceEl);
    this.spaceEl.append(
      spaceBar(
        { spaces: this.spaces, active: this.tree.active_space, all: this.tree.view_all, production: !!this.activeSettings()?.production },
        {
          select: async (id) => void (await this.selectSpace(id)),
          newSpace: () => void newSpace(this.spacesCtx()),
          manage: () => void this.manageSpace(),
          setAll: async (all): Promise<void> => {
            await this.be.setViewAll(all);
            await this.refreshTree();
          },
        },
      ),
    );
    clear(this.treeEl);
    this.searchWrap.hidden = this.leftSection !== "servers";
    switch (this.leftSection) {
      case "servers": {
        const n = this.tree.servers.length;
        const servers = this.secHead("servers", "Servers", n ? String(n) : "", [["+ Add", `Add a server`, () => void this.serverForm(null)], ["+ Folder", "New folder", () => void this.newFolder()]]);
        this.treeEl.append(servers.head);
        this.renderServerRows();
        break;
      }
      case "spaces":
        this.renderSpacesPanel();
        break;
      case "secrets":
        this.renderSecrets();
        break;
      case "tunnels":
        this.renderTunnels();
        break;
      case "accounts":
        this.renderAccounts();
        break;
    }
  }

  /** The folders and servers, or what to do when there are none. */
  private renderServerRows() {
    const rows = this.rows();
    if (rows.length === 0) {
      if (this.filter) this.treeEl.append(el("div", { class: "empty" }, "No server matches."));
      else this.treeEl.append(this.emptyBox("You have no servers yet. Add one, or bring in the ones you already use.", [["Add a server", () => void this.serverForm(null), true, "empty-add-server"], ["Import from ~/.ssh", () => void this.importSsh(), false, "empty-import-ssh"]]));
      return;
    }
    const open = this.openServerIds();
    for (const r of rows) {
      const sel = this.selected?.kind === r.kind && this.selected.id === r.id;
      const row = el("div", { class: `row${sel ? " sel" : ""}${r.kind === "folder" ? " folder" : ""}`, style: `padding-left:${8 + r.depth * 16}px`, "data-kind": r.kind, "data-id": String(r.id) });
      if (r.kind === "folder") {
        const f = this.tree.folders.find((f) => f.id === r.id)!;
        const chev = el("span", { class: "chev" });
        chev.append(icon(f.expanded ? "chevron-down" : "chevron-right", 14));
        row.append(chev, el("span", { class: "txt" }, el("b", {}, f.name)));
        row.onclick = () => {
          this.selected = r;
          void this.be.setFolderOpen(f.id, !f.expanded).then(() => this.refreshTree());
        };
      } else {
        const s = this.tree.servers.find((s) => s.id === r.id)!;
        const tail = el("span", { class: "tail" });
        if (s.has_secret) tail.append(icon("key", 14));
        const go = this.ibtn("play", "Open a terminal on this server", () => void this.openServer(s.id), false, "go");
        go.style.cssText = "width:24px;height:24px";
        go.onclick = (e) => {
          e.stopPropagation();
          void this.openServer(s.id);
        };
        row.append(
          el("span", { class: "dot", style: `background:${serverColor(s.id)}` }),
          el("span", { class: "txt", title: `${s.user ? s.user + "@" : ""}${s.host}:${s.port}` }, el("b", {}, s.name), el("small", {}, `${s.user ? s.user + "@" : ""}${s.host}`)),
        );
        if (open.has(s.id)) row.append(el("span", { class: "live", title: "Open" }));
        row.append(tail, go);
        row.onclick = () => {
          this.selected = r;
          this.renderSidebar();
        };
        row.ondblclick = () => void this.openOrFocus(s.id);
      }
      // drag a server onto the stage to open it there, or a server / folder onto a folder to move it
      row.addEventListener("pointerdown", (e) => {
        if ((e.target as HTMLElement).closest("button")) return;
        this.startHostDrag(e, r.kind, r.id, row);
      });
      this.treeEl.append(row);
    }
  }

  /** Double click / Enter on a server. */
  async openOrFocus(serverId: number) {
    // Always a new terminal: the same host can be open as many times as you want.
    await this.openServer(serverId);
  }

  /** Ctrl+N and the + button: another terminal for the same server as the active one. */
  async newTerminal() {
    const t = this.ws.activeTab();
    if (t) return this.openServer(t.serverId);
    // nothing open on this screen yet: choose the host
    await this.hostPicker();
  }

  /** Alt+K and the + button: type a few letters of a host and open it. */
  async hostPicker() {
    this.openPalette(["server"]);
  }

  // ------------------------------------------------------------ spaces

  async refreshSpaces() {
    this.spaces = await this.be.spaceList().catch(() => [] as Space[]);
    this.general = await this.be.spaceSettings(null).catch(() => null);
    this.renderSidebar();
    this.render();
  }

  /** What the Space in use (or General) fixes. */
  private activeSettings(): SpaceSettings | null {
    return (this.tree.active_space === null ? null : this.spaces.find((x) => x.id === this.tree.active_space)) ?? this.general;
  }

  /** The options of a "Space" select: General first (`general` is its text), then each Space. Values are ids, `""` is General. */
  private spaceOptions(general: string): [string, string][] {
    return [["", general], ...this.spaces.map((sp): [string, string] => [String(sp.id), sp.name])];
  }

  /** The Space (id as text, `""` for General) a server, folder or secret is in. */
  private spaceOf(kind: "server" | "folder", id: number): string {
    const sp = this.spaces.find((x) => (kind === "server" ? x.servers : x.server_folders).includes(id));
    return sp ? String(sp.id) : "";
  }

  /**
   * Works in a Space (null: General). If it asks for other GitHub/Tailscale accounts than the ones in use, asks first and
   * switches only if told to. False when the person cancelled.
   */
  async selectSpace(id: number | null): Promise<boolean> {
    if (id === this.tree.active_space) return true;
    const name = id === null ? "General" : this.spaces.find((x) => x.id === id)?.name ?? "this Space";
    const changes = await this.be.spaceAccountChanges(id).catch(() => []);
    let answer: "switch" | "keep" | "cancel" = "keep";
    if (changes.length) answer = await accountsAsk(name, changes);
    if (answer === "cancel") {
      this.renderSidebar();
      return false;
    }
    if (answer === "switch") {
      for (const c of changes) await this.be.accountUse(c.tool, c.id).catch((e) => toast(`${c.tool}: ${(e as Error).message}`, 6000));
      void this.refreshAccounts();
    }
    await this.be.setActiveSpace(id);
    await this.refreshTree();
    return true;
  }

  /** "Manage": the form of the Space in use, or General's settings. */
  private async manageSpace() {
    const sp = this.tree.active_space === null ? null : this.spaces.find((x) => x.id === this.tree.active_space);
    if (sp) await editSpace(this.spacesCtx(), sp);
    else await generalDialog(this.be, () => this.tree.proxies, () => this.refreshAll());
  }

  /** The form of a new secret from inside another form: resolves when it is closed. */
  private async addSecretFromForm() {
    if ((await this.unlockDialog(false)) !== "unlocked") return;
    await new Promise<void>((done) => secretForm(this.secretsCtx(), null, () => done(), { agent_visible: false }));
    await this.refreshSecrets();
  }

  private spacesCtx(): SpacesCtx {
    return {
      be: this.be,
      tree: () => this.tree,
      secrets: () => this.secretsMeta,
      proxies: () => this.tree.proxies,
      localSudo: () => this.localSudoId,
      addSecret: () => this.addSecretFromForm(),
      spaces: () => this.spaces,
      openCount: (id) => openSpaces(this.ws.tabs.values()).get(id) ?? 0,
      open: (sp, again) => void this.openSpace(sp, again),
      newTerminal: (sp) => void this.newSpaceTerminal(sp),
      newAgent: (sp) => this.newAgent(sp),
      changed: async () => {
        await this.refreshSpaces();
        await this.refreshTree();
      },
      emptyBox: (text, buttons) => this.emptyBox(text, buttons),
    };
  }

  /** The Spaces section: its header with "+ New Space", then one row per Space (or what a Space is, when there is none). */
  private renderSpacesPanel() {
    const { head } = this.secHead("spaces", "Spaces", this.spaces.length ? String(this.spaces.length) : "", [["+ New Space", "Make a Space: the folders, servers, secrets and tabs of a piece of work", () => void newSpace(this.spacesCtx())]]);
    this.treeEl.append(head);
    renderSpaces(this.treeEl, this.spacesCtx());
  }

  /** The tab on show, kept in a Space (an existing one, or a new one made on the spot). */
  async saveTabToSpace() {
    const cur = this.ws.shape();
    if (!cur) return toast("Open a terminal first, then save its tab");
    const panes = cur.sids.map((sid) => {
      const t = this.ws.tabs.get(sid)!;
      return { server: t.serverId, name: this.tree.servers.find((s) => s.id === t.serverId)?.name ?? t.title, cwd: t.cwd ?? "" };
    });
    const first = this.ws.tabs.get(cur.sids[0])?.title ?? "Tab";
    const tab = tabFor(panes.length > 1 ? `${first} + ${panes.length - 1}` : first, cur.shape, this.ws.broadcasting, panes);
    const belongs = this.ws.tabs.get(cur.sids[0])?.space;
    let target: Space | null | "new" = this.spaces.find((x) => x.id === belongs) ?? null;
    if (!target || this.spaces.length > 1) {
      const options: [string, string][] = [...this.spaces.map((x): [string, string] => [String(x.id), x.name]), ["new", "+ New Space…"]];
      const choice = await new Promise<string | null>((resolve) => {
        let answered = false;
        openModal(
          (close) => {
            const sel = makeSelect(options, target && target !== "new" ? String(target.id) : this.spaces.length ? String(this.spaces[0].id) : "new", { "data-name": "space-choice" });
            const ok = el("button", { class: "btn primary", type: "button", "data-act": "space-choice-ok" }, "Save");
            const no = el("button", { class: "btn", type: "button" }, "Cancel");
            const finish = (v: string | null) => {
              answered = true;
              resolve(v);
              close();
            };
            ok.onclick = () => finish(sel.value);
            no.onclick = () => finish(null);
            queueMicrotask(() => sel.focus());
            return el("div", {}, el("h2", {}, "Save this tab in a Space"), el("div", { class: "hint", style: "margin:0 0 10px" }, `${panes.length} terminal${panes.length > 1 ? "s" : ""}, as they are arranged now. It opens again with the Space.`), field("Space", sel), el("div", { class: "actions" }, no, ok));
          },
          // closing the window any other way (Escape) is a no
          () => {
            if (!answered) resolve(null);
          },
        );
      });
      if (choice == null) return;
      target = choice === "new" ? "new" : (this.spaces.find((x) => String(x.id) === choice) ?? null);
    }
    if (target === "new") {
      const id = await newSpace(this.spacesCtx(), { name: first, servers: [...new Set(panes.map((p) => p.server).filter((s) => s !== 0))], tabs: [tab] });
      if (id == null) return;
      return void toast("Saved. Open the Space from the left to see it come back.");
    }
    if (!target) return;
    await this.be.spaceSave(withTab(target, tab));
    await this.refreshSpaces();
    toast(`Saved in “${target.name}”. It opens again with the Space.`);
  }

  /** Opens a Space: it becomes the one in use, then its tunnels and its saved tabs. One that is already open is only brought forward. */
  async openSpace(sp: Space, again = false) {
    const here = [...this.ws.tabs.values()].find((t) => t.space === sp.id && !t.dead);
    if (here && !again) return void this.focusSession(here.sid);
    if (this.tree.active_space !== sp.id && !(await this.selectSpace(sp.id))) return;
    for (const t of sp.tunnels) {
      if (this.tree.servers.some((s) => s.id === t.server && s.forwards.some((f) => f.id === t.forward))) await this.be.tunnelStart(t.server, t.forward, true).catch((e) => toast(`A tunnel of “${sp.name}” did not start: ${(e as Error).message}`, 6000));
    }
    void this.pollTunnels();
    if (!sp.tabs.length) return void toast(`“${sp.name}” has no saved tabs yet. Open a terminal from it (the terminal button on its row), or arrange a tab and save it in the Space.`, 6000);
    for (const tab of sp.tabs) await this.openLayout(tab, sp);
  }

  /** A new local terminal in a Space's first folder, with its secrets and its ssh key. */
  async newSpaceTerminal(sp: Space) {
    const info = await this.openIn(sp, LOCAL, sp.folders[0] ?? "");
    if (info) toast(`Terminal opened in “${sp.name}”`);
  }

  /** One session of a Space, put in a new tab. `null` when it could not be opened (the person was told). */
  private async openIn(sp: Space | null, server: number, cwd: string): Promise<SessionInfo | null> {
    this.measure();
    let useVault = true;
    for (;;) {
      try {
        const info = await this.be.open(server, this.guessRows(), this.guessCols(), useVault, { ...(sp ? { space: sp.id } : {}), ...(cwd ? { cwd } : {}) });
        this.addSession(info);
        return info;
      } catch (e) {
        if (e instanceof VaultLockedError) {
          const c = await this.unlockDialog(true);
          if (c === "cancel") return null;
          if (c === "without") useVault = false;
          continue;
        }
        toast(`Could not open it: ${(e as Error).message}`);
        return null;
      }
    }
  }

  /** Opens the terminals of one saved tab together, as one tab, arranged as it was saved. */
  async openLayout(g: SpaceTab, sp?: Space) {
    const gone = new Set<number>();
    g.panes.forEach((p, i) => {
      if (p.server !== LOCAL && !this.tree.servers.some((s) => s.id === p.server)) gone.add(i);
    });
    const use = gone.size ? withoutPanes(g, gone) : { shape: g.shape, panes: g.panes };
    if (!use) return toast(`The servers of “${g.name}” no longer exist`);
    if (gone.size) toast(`${gone.size} server${gone.size > 1 ? "s" : ""} of this tab no longer exist${gone.size > 1 ? "" : "s"}: ${[...gone].map((i) => g.panes[i].name).join(", ")}`, 6000);
    this.measure();
    const opened: SessionInfo[] = [];
    let useVault = true;
    for (let i = 0; i < use.panes.length; i++) {
      try {
        const pane = use.panes[i];
        opened.push(await this.be.open(pane.server, this.guessRows(), this.guessCols(), useVault, { ...(sp ? { space: sp.id } : {}), ...(pane.server === LOCAL && pane.cwd ? { cwd: pane.cwd } : {}) }));
      } catch (e) {
        if (e instanceof VaultLockedError) {
          const c = await this.unlockDialog(true);
          if (c === "cancel") {
            for (const o of opened) void this.be.close(o.sid).catch(() => {});
            return;
          }
          if (c === "without") useVault = false;
          i--;
          continue;
        }
        toast(`Could not open ${use.panes[i].name || "Local"}: ${(e as Error).message}`);
        opened.push(null as unknown as SessionInfo);
      }
    }
    // a terminal that failed to open leaves its place: the rest close up
    const failed = new Set(opened.map((o, i) => (o ? -1 : i)).filter((i) => i >= 0));
    const fit = failed.size ? withoutPanes(use, failed) : use;
    const live = opened.filter(Boolean);
    if (!fit || !live.length) return;
    live.forEach((o) => {
      this.since.set(o.sid, Date.now());
      this.opened.set(o.sid, Date.now());
    });
    this.ws.addGroup(
      live.map((o) => ({ sid: o.sid, title: o.title, serverId: o.server_id, ...(o.meta.space != null ? { space: Number(o.meta.space) } : {}), ...(o.meta.cwd ? { cwd: String(o.meta.cwd) } : {}) })),
      fit.shape,
      g.broadcast,
    );
    this.focusArea = "terminal";
    live.forEach((o, i) => {
      this.ensurePane(o.sid);
      const buffered = this.early.get(o.sid);
      this.early.delete(o.sid);
      buffered?.forEach((e) => this.onSession(e));
      const command = fit.panes[i].command.trim();
      if (command) void this.typeWhenReady(o.sid, command);
    });
  }

  /** Types a command in a terminal once it shows a prompt (never at a password question). Gives up after 30 s. */
  private async typeWhenReady(sid: Id, command: string) {
    const until = Date.now() + 30000;
    while (Date.now() < until) {
      await new Promise((r) => setTimeout(r, 300));
      const pane = this.panes.get(sid);
      const tab = this.ws.tabs.get(sid);
      if (!pane || !tab || tab.dead) return;
      if (pane.lastOutputAt && Date.now() - pane.lastOutputAt >= 600 && looksReady(pane.cursorLine())) {
        await this.be.write(sid, `${command}\r`);
        return;
      }
    }
    toast(`The command “${command}” was not typed: the terminal never showed a prompt`, 6000);
  }

  /** The saved tunnels of every host shown, each with a switch (name → port, how it is doing). */
  private renderTunnels() {
    const all = this.tree.servers.flatMap((s) => s.forwards.map((f) => ({ s, f })));
    const on = (id: string) => this.tunnelStatus.get(id)?.state === "up";
    const n = all.filter(({ s, f }) => on(`${s.id}:${f.id}`)).length;
    const addTunnel = () => {
      if (!this.tree.servers.length) return toast("Add a server first: a tunnel goes through one");
      tunnelForm(this.tunnelCtx(), null, null, () => void this.refreshTree());
    };
    const { head, open } = this.secHead("tunnels", "Tunnels", all.length ? (n ? `${n} on` : String(all.length)) : "", [["+ Add", "Add a tunnel", addTunnel], ["Manage", `Open the Tunnels window (${this.keyText("tunnels.open")})`, () => this.openTunnels()]]);
    this.treeEl.append(head);
    if (!open) return;
    if (!all.length) return void this.treeEl.append(this.emptyBox("A tunnel keeps a port open through a server, for example to reach a database that only the server can see.", [["Add a tunnel", addTunnel, true, "empty-add-tunnel"]]));
    for (const { s, f } of all) {
      const id = `${s.id}:${f.id}`;
      const st = this.tunnelStatus.get(id);
      const running = !!st && st.state !== "failed";
      const row = el("div", { class: `row tunnel${st?.state === "up" ? " up" : ""}${st?.state === "failed" ? " failed" : ""}`, "data-tunnel": id, title: st?.state === "failed" ? st.message : describeTunnel(f) });
      const dot = el("span", { class: "tdot" });
      const sw = this.ibtn(running ? "close" : "play", running ? "Switch off" : "Switch on", () => {});
      sw.style.cssText = "width:24px;height:24px";
      sw.onclick = async (e) => {
        e.stopPropagation();
        if (running) await this.be.tunnelStop(s.id, f.id);
        else await switchOn(this.tunnelCtx(), s.id, f.id);
        await this.pollTunnels();
      };
      row.append(dot, el("span", { class: "txt" }, el("b", {}, f.name.trim() || describeTunnel(f)), el("small", {}, st?.state === "failed" ? st.message : `${f.name.trim() ? describeTunnel(f) + " · " : ""}${s.name}`)), sw);
      this.treeEl.append(row);
    }
  }

  /** The title line of a section of the left panel: its name, how many, and its own buttons (with words, not just icons). */
  private secHead(key: string, title: string, count: string, actions: [string, string, () => void][]): { head: HTMLElement; open: boolean } {
    const head = el("div", { class: "sec", "data-sec": key });
    head.append(title, el("span", { class: "n" }, count));
    for (const [label, tip, run] of actions) {
      const b = el("button", { class: "sec-btn", type: "button", title: tip, "aria-label": tip, "data-act": `sec-${key}-${label.replace(/[^a-z]/gi, "").toLowerCase()}` }, label);
      b.onclick = (e) => {
        e.stopPropagation();
        run();
      };
      head.append(b);
    }
    return { head, open: true };
  }

  /** What a section says when it has nothing: one line about what it is for, and a button that does the first thing. */
  private emptyBox(text: string, buttons: [string, () => void, boolean, string][]): HTMLElement {
    const box = el("div", { class: "empty-box" }, el("div", {}, text));
    for (const [label, run, primary, act] of buttons) {
      const b = el("button", { class: `btn small${primary ? " primary" : ""}`, type: "button", "data-act": act }, label);
      b.onclick = run;
      box.append(b);
    }
    return box;
  }

  private secretsCtx() {
    return { be: this.be, spaces: () => this.spaces.map((x) => ({ id: x.id, name: x.name })), importSsh: () => void this.importSsh(), importEnv: () => void envImportDialog(this.importCtx(), () => void this.vaultDialog()), importProton: () => void protonImportDialog(this.importCtx(), () => void this.vaultDialog()), showCode: (id: number, name: string) => void showCode(this.be, id, name), lockVault: () => this.lockVault(), changed: () => void this.refreshSecrets() };
  }

  private importCtx() {
    return { be: this.be, unlock: async () => (await this.unlockDialog(false)) === "unlocked", refreshTree: () => this.refreshTree() };
  }

  async importSsh() {
    await importDialog(this.importCtx(), () => void this.refreshSecrets());
  }

  /** "Add" on the Secrets section: asks for the vault if it is locked, then the form. */
  async newSecret(preset: Partial<SecretMeta> = {}) {
    if ((await this.unlockDialog(false)) !== "unlocked") return;
    secretForm(this.secretsCtx(), null, () => void this.refreshSecrets(), preset);
  }

  async refreshSecrets() {
    this.secretsMeta = this.vault.unlocked ? await this.be.secretList().catch(() => null) : null;
    this.localSudoId = this.vault.unlocked ? await this.be.localSudoSecret().catch(() => null) : null;
    this.renderSidebar();
    this.render();
  }

  private renderSecrets() {
    const meta = this.secretsMeta;
    const { head, open } = this.secHead("secrets", "Secrets", meta?.length ? String(meta.length) : "", [["+ Add", "Add a secret", () => void this.newSecret()], ["Open", `Open the Secrets window (${this.keyText("secrets.open")})`, () => void this.vaultDialog()]]);
    this.treeEl.append(head);
    if (!open) return;
    if (!this.vault.exists) return void this.treeEl.append(this.emptyBox("API keys, passwords and SSH keys are kept in an encrypted vault. Create it to save your first one.", [["Create the vault", () => void this.vaultDialog(), true, "empty-create-vault"]]));
    if (!this.vault.unlocked) return void this.treeEl.append(this.emptyBox("The vault is locked.", [["Unlock the vault", () => void this.vaultDialog(), true, "empty-unlock"]]));
    if (!meta?.length) return void this.treeEl.append(this.emptyBox("You have no secrets yet. Add your first API key.", [["Add an API key", () => void this.newSecret({ kind: "api_key", agent_visible: true }), true, "empty-add-key"], ["Import…", () => void this.vaultDialog(), false, "empty-import"]]));
    for (const m of meta.slice(0, 6)) {
      const row = el("div", { class: "row secret-row", "data-secret-row": m.name, title: "Open the Secrets window" }, el("span", { class: "txt" }, el("b", {}, m.name), el("small", {}, `${KIND_LABEL[m.kind]}${m.env_var ? ` · $${m.env_var}` : ""}`)));
      row.onclick = () => void this.vaultDialog();
      this.treeEl.append(row);
    }
    if (meta.length > 6) this.treeEl.append(el("div", { class: "hint-row" }, `…and ${meta.length - 6} more in the Secrets window.`));
  }

  private renderAccounts() {
    const v = this.accountsView;
    const any = !!v && (v.github.installed || v.tailscale.installed);
    const { head, open } = this.secHead("accounts", "Accounts", "", [["Refresh", "Ask GitHub and Tailscale again", () => void this.refreshAccounts()]]);
    this.treeEl.append(head);
    if (!open) return;
    if (v && any) return void accountsRows(this.treeEl, v, this.be, () => this.refreshAccounts());
    this.treeEl.append(this.emptyBox("Switch your GitHub and Tailscale accounts here, and per Space. It needs `gh` and `tailscale` installed; oso never keeps their tokens.", [["Check again", () => void this.refreshAccounts(), false, "empty-check-accounts"]]));
  }

  // ------------------------------------------------------------ snippets

  private saveSnippets() {
    void this.be.writeConfig("snippets.json", JSON.stringify(this.snippets, null, 2));
  }

  /** Saved commands: click one to type it (and Enter) in the active terminal, or in every terminal with broadcast on. */
  snippetsDialog() {
    openModal((close) => {
      const list = el("div");
      const draw = () => {
        clear(list);
        if (!this.snippets.length) list.append(el("div", { class: "empty" }, "No snippets yet. A snippet is a command you run often: add one."));
        this.snippets.forEach((s, i) => {
          const run = this.ibtn("play", "Run in the active terminal", () => {
            close();
            this.runSnippet(s.command);
          });
          const edit = this.ibtn("edit", "Edit", () => void this.snippetForm(i).then(draw));
          const del = this.ibtn("trash", "Delete", () => {
            this.snippets.splice(i, 1);
            this.saveSnippets();
            draw();
          });
          list.append(el("div", { class: "snip" }, run, el("div", { class: "txt" }, el("b", {}, s.name), el("code", {}, s.command)), edit, del));
        });
      };
      draw();
      const add = el("button", { class: "btn", type: "button" }, "New snippet");
      add.onclick = () => void this.snippetForm(null).then(draw);
      const done = el("button", { class: "btn primary", type: "button" }, "Done");
      done.onclick = close;
      return el("div", {}, el("h2", {}, "Snippets"), list, el("div", { class: "actions" }, add, done));
    });
  }

  private snippetForm(index: number | null): Promise<void> {
    const cur = index == null ? { name: "", command: "" } : this.snippets[index];
    return new Promise((resolve) => {
      // a second dialog on top of the list: the list is shown again when this one closes
      openModal(
        (close) => {
          const name = el("input", { value: cur.name, autocomplete: "off" });
          const cmd = el("input", { value: cur.command, autocomplete: "off", spellcheck: "false", placeholder: "sudo systemctl restart nginx" });
          const err = el("div", { class: "err" });
          const form = el("form", {}, el("h2", {}, index == null ? "New snippet" : "Edit snippet"), field("Name", name), field("Command", cmd), err);
          const ok = el("button", { class: "btn primary", type: "submit" }, "Save");
          const no = el("button", { class: "btn", type: "button" }, "Cancel");
          no.onclick = close;
          form.append(el("div", { class: "actions" }, no, ok));
          form.onsubmit = (e) => {
            e.preventDefault();
            if (!cmd.value.trim()) return void (err.textContent = "Write the command");
            const s = { name: name.value.trim() || cmd.value.trim().slice(0, 30), command: cmd.value };
            if (index == null) this.snippets.push(s);
            else this.snippets[index] = s;
            this.saveSnippets();
            close();
          };
          return form;
        },
        () => {
          resolve();
          queueMicrotask(() => this.snippetsDialog());
        },
      );
    });
  }

  private runSnippet(command: string) {
    const t = this.ws.activeTab();
    if (!t || t.dead) return toast("Open a terminal first");
    const targets = this.ws.broadcasting ? this.ws.tabsHere().filter((x) => !x.dead) : [t];
    targets.forEach((x) => void this.be.write(x.sid, `${command}\r`));
    this.focusActive();
  }

  // ------------------------------------------------------------ dialogs

  async unlockDialog(offerWithout: boolean): Promise<"unlocked" | "without" | "cancel"> {
    this.vault = await this.be.vaultStatus();
    if (this.vault.unlocked) return "unlocked";
    return new Promise((resolve) => {
      let result: "unlocked" | "without" | "cancel" = "cancel";
      openModal(
        (close) => {
          const creating = !this.vault.exists;
          const pw = el("input", { type: "password", autocomplete: "off" });
          const confirm = el("input", { type: "password", autocomplete: "off" });
          const err = el("div", { class: "err" });
          const form = el("form", {}, el("h2", {}, creating ? "Create the vault" : "Unlock the vault"), el("div", { class: "hint" }, creating ? "Passwords are kept encrypted under this master password. Choose it carefully: it cannot be recovered." : "Enter the master password to use the saved passwords."), field("Master password", pw));
          if (creating) form.append(field("Repeat it", confirm));
          form.append(err);
          const ok = el("button", { class: "btn primary", type: "submit" }, creating ? "Create" : "Unlock");
          const no = el("button", { class: "btn", type: "button" }, "Cancel");
          no.onclick = close;
          const actions = el("div", { class: "actions" });
          if (offerWithout) {
            const w = el("button", { class: "btn", type: "button" }, "Connect without it");
            w.onclick = () => {
              result = "without";
              close();
            };
            actions.append(w);
          }
          actions.append(no, ok);
          form.append(actions);
          form.onsubmit = async (e) => {
            e.preventDefault();
            if (creating && pw.value !== confirm.value) return void (err.textContent = "The two passwords are not the same");
            if (!pw.value) return void (err.textContent = "Type the master password");
            try {
              await this.be.vaultUnlock(pw.value);
              this.vault = await this.be.vaultStatus();
              result = "unlocked";
              close();
            } catch (x) {
              err.textContent = (x as Error).message;
              pw.select();
            }
          };
          return form;
        },
        () => {
          this.renderRail();
          resolve(result);
        },
      );
    });
  }

  /** The Secrets window: every secret of the vault (asks for the master password first). */
  async vaultDialog() {
    if ((await this.unlockDialog(false)) !== "unlocked") return;
    secretsDialog(this.secretsCtx(), { space: this.tree.view_all || this.tree.active_space === null ? "" : String(this.tree.active_space) });
  }

  async lockVault() {
    await this.be.vaultLock();
    this.vault = await this.be.vaultStatus();
    this.renderRail();
    toast("Vault locked");
  }

  async newFolder() {
    const name = await promptBox("New folder", "");
    if (!name?.trim()) return;
    const parent = this.selected?.kind === "folder" ? this.selected.id : null;
    await this.be.addFolder(name.trim(), parent);
    await this.refreshTree();
  }

  editSelected() {
    const s = this.selected;
    if (!s) return toast("Select a server first");
    if (s.kind === "server") void this.serverForm(this.tree.servers.find((x) => x.id === s.id) ?? null);
    else void this.renameFolder(this.tree.folders.find((f) => f.id === s.id)!);
  }

  private renameFolder(f: Folder) {
    openModal((close) => {
      const name = el("input", { value: f.name, name: "name", autocomplete: "off" });
      const space = makeSelect(this.spaceOptions("General (every Space)"), this.spaceOf("folder", f.id), { "data-name": "folder-space" });
      const form = el("form", {}, el("h2", {}, "Folder"), field("Name", name), field("Space", space));
      const ok = el("button", { class: "btn primary", type: "submit" }, "Save");
      const no = el("button", { class: "btn", type: "button" }, "Cancel");
      no.onclick = close;
      form.append(el("div", { class: "actions" }, no, ok));
      form.onsubmit = async (e) => {
        e.preventDefault();
        if (name.value.trim() && name.value.trim() !== f.name) await this.be.renameFolder(f.id, name.value.trim());
        if (space.value !== this.spaceOf("folder", f.id)) await this.be.moveToSpace("folder", f.id, space.value ? Number(space.value) : null);
        await this.refreshSpaces();
        close();
        await this.refreshTree();
      };
      return form;
    });
  }

  async deleteSelected() {
    const s = this.selected;
    if (!s) return toast("Select a server or a folder first");
    const name = s.kind === "server" ? this.tree.servers.find((x) => x.id === s.id)?.name : this.tree.folders.find((f) => f.id === s.id)?.name;
    const ok = await confirmBox(`Delete “${name}”?`, s.kind === "folder" ? "The folder and every server inside it will be deleted, with their saved passwords." : "The server and its saved passwords will be deleted.", "Delete");
    if (!ok) return;
    await this.be.deleteNode(s.kind, s.id);
    this.selected = null;
    await this.refreshTree();
  }

  /** Add (`null`) or edit a server. Secrets go to the vault; blank leaves a saved one alone. */
  async serverForm(existing: Server | null) {
    const all = this.vault.unlocked ? await this.be.secretList().catch(() => []) : [];
    const keys = all.filter((k) => k.kind === "ssh_key");
    const seeds = all.filter((k) => k.kind === "totp");
    const s: Server = existing ?? { id: 0, name: "", host: "", port: 22, user: "", auth: "agent", key_path: "", parent: this.selected?.kind === "folder" ? this.selected.id : null, jump: null, has_secret: false, has_sudo: false, has_totp: false, forwards: [], notes: "", env_kind: null, proxy: null, network: null, ai_policy: "ask" };
    openModal((close) => {
      const name = el("input", { value: s.name, name: "name", autocomplete: "off" });
      const host = el("input", { value: s.host, name: "host", autocomplete: "off", spellcheck: "false" });
      const port = el("input", { value: String(s.port), name: "port", inputmode: "numeric" });
      const user = el("input", { value: s.user, name: "user", autocomplete: "off", spellcheck: "false" });
      const auth = makeSelect([["agent", "SSH agent"], ["key", "Private key"], ["password", "Password"], ["vaultkey", "Key from the vault"]], s.auth, { "data-name": "auth" });
      const key = el("input", { value: s.key_path, name: "key", placeholder: "~/.ssh/id_ed25519", spellcheck: "false" });
      const login = el("input", { type: "password", name: "login", autocomplete: "new-password", placeholder: s.has_secret ? "saved — leave empty to keep" : "" });
      const sudo = el("input", { type: "password", name: "sudo", autocomplete: "new-password", placeholder: s.has_sudo ? "saved — leave empty to keep" : "" });
      const jump = makeSelect([["", "Direct"], ...this.tree.servers.filter((x) => x.id !== s.id).map((x): [string, string] => [String(x.id), x.name])], s.jump == null ? "" : String(s.jump), { "data-name": "jump" });
      const space = makeSelect(this.spaceOptions("General (every Space)"), existing ? this.spaceOf("server", s.id) : this.tree.active_space === null ? "" : String(this.tree.active_space), { "data-name": "space" });
      const envKind = makeSelect([["", "Not said"], ["production", "Production"], ["staging", "Staging"], ["dev", "Development"]], s.env_kind ?? "", { "data-name": "env-kind" });
      const proxy = makeSelect([["", "Direct"], ...this.tree.proxies.map((p): [string, string] => [String(p.id), p.name])], s.proxy == null ? "" : String(s.proxy), { "data-name": "proxy" });
      const network = el("input", { value: s.network ?? "", name: "network", autocomplete: "off", spellcheck: "false", placeholder: "tailscale:work-tailnet (optional)" });
      const aiPolicy = makeSelect([["ask", "Ask me every time"], ["read_only_auto", "Let commands that only read go through"], ["blocked", "Never let an agent use it"]], s.ai_policy, { "data-name": "ai-policy" });
      const notes = el("textarea", { name: "notes", rows: "2", spellcheck: "false", placeholder: "nginx + API node, logs in /var/log/your-service" }, s.notes);
      const tunnelsBtn = el("button", { class: "btn", type: "button", "data-act": "server-tunnels" }, s.forwards.length ? `Tunnels (${s.forwards.length})…` : "Tunnels…");
      tunnelsBtn.onclick = () => {
        close();
        this.openTunnels();
      };
      const folder = makeSelect([["", "(top level)"], ...this.tree.folders.map((f): [string, string] => [String(f.id), f.name])], s.parent == null ? "" : String(s.parent), { "data-name": "folder" });
      const vaultKey = makeSelect([["", this.vault.unlocked ? (keys.length ? "Choose a key" : "No SSH key in the vault yet") : "Unlock the vault to choose"], ...keys.map((k): [string, string] => [String(k.id), k.name])], s.vault_key == null ? "" : String(s.vault_key), { "data-name": "vault-key" });
      const mfa = makeSelect([["", this.vault.unlocked ? "None" : "Unlock the vault to choose"], ...seeds.map((k): [string, string] => [String(k.id), k.name])], s.vault_totp == null ? "" : String(s.vault_totp), { "data-name": "mfa" });
      const vaultKeyRow = field("Key (kept in the vault, signed by oso's own ssh-agent)", vaultKey);
      const clearLogin = el("input", { type: "checkbox", id: "clr" });
      const keyRow = field("Key file", key);
      const loginLabel = el("label", {}, "");
      const loginRow = el("div", {}, loginLabel, login);
      const rmRow = el("div", { class: "hint" }, clearLogin, " remove the saved password");
      const sync = () => {
        keyRow.style.display = auth.value === "key" ? "" : "none";
        vaultKeyRow.style.display = auth.value === "vaultkey" ? "" : "none";
        loginRow.style.display = auth.value === "agent" || auth.value === "vaultkey" ? "none" : "";
        loginLabel.textContent = auth.value === "key" ? "Key passphrase (kept in the vault)" : "Password (kept in the vault)";
        rmRow.style.display = auth.value !== "agent" && auth.value !== "vaultkey" && s.has_secret ? "" : "none";
      };
      auth.onchange = sync;
      sync();
      const err = el("div", { class: "err" });
      const form = el("form", {}, el("h2", {}, existing ? `Edit ${s.name}` : "New server"), field("Name", name), el("div", { class: "grid2" }, field("Address", host), field("Port", port)), field("User", user), field("Authentication", auth), keyRow, vaultKeyRow, loginRow, rmRow, field("sudo password (kept in the vault)", sudo), field("One-time codes it asks for (MFA)", mfa), field("Reached through", jump), field("Folder", folder), field("Space", space), el("div", { class: "grid2" }, field("What it is for", envKind), field("Connects through", proxy)), field("Network", network), field("When an AI agent asks to use it", aiPolicy), el("div", { class: "hint" }, "A production machine always asks."), field("Notes (agents read them)", notes), existing ? el("div", {}, tunnelsBtn) : null, err);
      const ok = el("button", { class: "btn primary", type: "submit" }, "Save");
      const no = el("button", { class: "btn", type: "button" }, "Cancel");
      no.onclick = close;
      form.append(el("div", { class: "actions" }, no, ok));
      form.onsubmit = async (e) => {
        e.preventDefault();
        if (!host.value.trim()) return void (err.textContent = "The address is required");
        const portN = Number(port.value);
        if (!Number.isInteger(portN) || portN < 1 || portN > 65535) return void (err.textContent = "The port must be a number from 1 to 65535");
        const next: Server = { ...s, name: name.value.trim() || host.value.trim(), host: host.value.trim(), port: portN, user: user.value.trim(), auth: auth.value as Server["auth"], key_path: key.value.trim(), jump: jump.value ? Number(jump.value) : null, parent: folder.value ? Number(folder.value) : null, env_kind: (envKind.value || null) as Server["env_kind"], proxy: proxy.value ? Number(proxy.value) : null, notes: notes.value.trim(), network: network.value.trim() || null, ai_policy: aiPolicy.value as Server["ai_policy"] };
        const loginSecret = auth.value === "agent" || auth.value === "vaultkey" ? (s.has_secret ? "" : null) : clearLogin.checked ? "" : login.value || null;
        const sudoSecret = sudo.value || null;
        if ((loginSecret || sudoSecret) && !this.vault.unlocked) {
          const r = await this.unlockDialog(false);
          if (r !== "unlocked") return void (err.textContent = "Unlock the vault to save a password");
        }
        if (auth.value === "vaultkey") {
          if (!this.vault.unlocked) return void (err.textContent = "Unlock the vault (the lock, bottom left) and open this form again to choose its key");
          if (!vaultKey.value) return void (err.textContent = "Choose the key it logs in with");
        }
        try {
          const id = await this.be.saveServer(next, loginSecret, sudoSecret);
          if (space.value !== (existing ? this.spaceOf("server", id) : "") || (!existing && space.value)) await this.be.moveToSpace("server", id, space.value ? Number(space.value) : null);
          await this.refreshSpaces();
          if (auth.value === "vaultkey") await this.be.serverSetKey(id, Number(vaultKey.value));
          const wantTotp = mfa.value ? Number(mfa.value) : null;
          if (wantTotp !== (s.vault_totp ?? null)) await this.be.serverSetTotp(id, wantTotp);
          this.selected = { kind: "server", id };
          close();
          await this.refreshTree();
        } catch (x) {
          err.textContent = (x as Error).message;
        }
      };
      return form;
    });
  }

  async renameTab(sid: Id) {
    const t = this.ws.tabs.get(sid);
    if (!t) return;
    const name = await promptBox("Rename terminal", t.title);
    if (name?.trim()) this.ws.setTitle(sid, name.trim());
  }

  settingsDialog() {
    const edits: Record<string, unknown> = { ...(this.userKeys ?? {}) };
    delete edits.modifier;
    let mod = modifierOf(this.userKeys);
    openModal((close) => {
      const list = el("div", { class: "keylist" });
      const inputs = new Map<string, HTMLInputElement>();
      const warn = el("div", { class: "err" });
      const note = el("div", { class: "hint" });
      const draw = () => {
        clear(list);
        inputs.clear();
        note.textContent =
          mod === "Ctrl"
            ? "Ctrl is also what terminal programs use (Ctrl+C, Ctrl+M, Ctrl+H…): some shortcuts will clash with them."
            : mod === "Super"
              ? "Super is not used by terminal programs, but your desktop may keep some Super shortcuts for itself."
              : "";
        for (const action of Object.keys(DEFAULTS)) {
          const cur = action in edits ? edits[action] : DEFAULTS[action];
          const text = cur == null ? "" : (Array.isArray(cur) ? cur : [cur]).map((x) => resolveMod(String(x), mod)).join(" / ");
          const input = el("input", { value: text, readonly: true, placeholder: "none", "data-action": action });
          inputs.set(action, input);
          input.onfocus = () => input.classList.add("capture");
          input.onblur = () => input.classList.remove("capture");
          input.onkeydown = (e) => {
            e.preventDefault();
            e.stopPropagation();
            if (e.key === "Backspace" || e.key === "Delete") edits[action] = null;
            else if (["Alt", "Control", "Shift", "Meta"].includes(e.key)) return;
            else {
              const combo = comboFromEvent(e);
              if (!combo) return;
              edits[action] = combo;
            }
            draw();
            inputs.get(action)?.focus();
          };
          const reset = el("button", { class: "ib", type: "button", title: "Back to the default" });
          reset.append(icon("close", 14));
          reset.onclick = () => {
            delete edits[action];
            draw();
          };
          list.append(el("div", { class: "keyrow" }, el("span", {}, LABELS[action] ?? action), input, reset));
        }
      };
      const modSel = makeSelect(MODIFIERS.map((m): [string, string] => [m, m === "Alt" ? "Alt (default)" : m]), mod, { "data-role": "modifier" });
      modSel.onchange = () => {
        mod = modSel.value;
        draw();
      };
      draw();
      const activity = el("button", { class: "btn", type: "button", "data-act": "ai-activity" }, "What AI agents asked…");
      activity.onclick = () => {
        close();
        void auditDialog(this.be);
      };
      const soundsBtn = el("button", { class: "btn", type: "button", "data-act": "sounds" }, "Sounds…");
      soundsBtn.onclick = () => {
        close();
        soundsDialog(this.sound);
      };
      const agentsBtn = el("button", { class: "btn", type: "button", "data-act": "agent-kinds" }, "Agents…");
      agentsBtn.onclick = () => {
        close();
        void agentKindsDialog(this.be);
      };
      const save = el("button", { class: "btn primary", type: "button" }, "Save");
      const cancel = el("button", { class: "btn", type: "button" }, "Cancel");
      cancel.onclick = close;
      save.onclick = async () => {
        const clean: Record<string, unknown> = { modifier: mod };
        for (const [a, v] of Object.entries(edits)) if (JSON.stringify(v) !== JSON.stringify(DEFAULTS[a])) clean[a] = v;
        const { problems } = buildBindings(clean);
        if (problems.length) return void (warn.textContent = problems[0]);
        await this.be.writeConfig("hotkeys.json", JSON.stringify(clean, null, 2));
        this.userKeys = clean;
        this.bindings = buildBindings(clean).bindings;
        close();
        this.render();
        toast("Hotkeys saved");
      };
      return el(
        "div",
        { class: "wide" },
        el("h2", {}, "Settings"),
        el("h2", { style: "font-size:14px" }, "Hotkeys"),
        field("Main key: every shortcut that starts with it changes at once", modSel),
        note,
        el("div", { class: "hint", style: "margin:10px 0 6px" }, "Click a shortcut and press the new keys; the cross puts it back. Saved in hotkeys.json, which you can also edit by hand."),
        list,
        warn,
        el("div", { class: "actions" }, activity, agentsBtn, soundsBtn, cancel, save),
      );
    });
  }

  /** Alt+P: types the sudo password saved for this terminal's server. */
  async fillSudo(): Promise<void> {
    const t = this.ws.activeTab();
    if (!t || t.dead) return;
    try {
      await this.be.fillSudo(t.sid, t.serverId);
    } catch (e) {
      if (!this.vault.unlocked && (await this.unlockDialog(false)) === "unlocked") return this.fillSudo();
      toast((e as Error).message);
    }
  }

  /** Mod+B: what is typed in this tab goes to all of its terminals (and back). Needs two or more. */
  toggleBroadcast() {
    const ws = this.ws;
    if (ws.broadcasting) {
      ws.setBroadcast(false);
      return toast("Broadcast off");
    }
    if (!ws.screen.group) return;
    if (ws.screen.mode !== "mosaic") {
      if (ws.screen.groups.length < 2) return toast("Broadcast needs two or more terminals");
      ws.toggleMode();
    }
    ws.setBroadcast(true);
    toast("BROADCAST: what you type goes to every terminal of this tab");
  }

  // ------------------------------------------------------------ keyboard

  private onKey(e: KeyboardEvent) {
    if (modalOpen()) return;
    if (e.isComposing) return;
    const tgt = e.target as HTMLElement | null;
    const typing = tgt && (tgt.tagName === "INPUT" || tgt.tagName === "SELECT") && !e.altKey && !e.ctrlKey && !e.metaKey;
    if (typing) return;
    // "?" opens the sheet of shortcuts, but only when you are not typing (a terminal's own input counts as typing)
    if (e.key === "?" && !e.altKey && !e.ctrlKey && !e.metaKey && !(tgt && (tgt.tagName === "TEXTAREA" || tgt.isContentEditable || tgt.closest(".xterm")))) {
      e.preventDefault();
      return this.shortcutsSheet();
    }
    const action = matchAction(this.bindings, e);
    if (!action) return;
    e.preventDefault();
    e.stopPropagation();
    this.run(action);
  }

  run(action: string) {
    const ws = this.ws;
    const [kind, arg] = action.split(".");
    if (kind === "tab" && /^[1-9]$/.test(arg)) return ws.goTab(Number(arg));
    const dir = { left: [-1, 0], right: [1, 0], up: [0, -1], down: [0, 1] }[arg] as [number, number] | undefined;
    switch (action) {
      case "tab.prev":
        return ws.step(-1);
      case "tab.next":
        return ws.step(1);
      case "mode.toggle":
        return ws.toggleMode();
      case "split.right":
        ws.prepareSplit("right");
        return void this.newTerminal();
      case "split.down":
        ws.prepareSplit("bottom");
        return void this.newTerminal();
      case "tab.new":
        return void this.newTerminal();
      case "tab.close":
        return void (ws.screen.active != null && this.closeSession(ws.screen.active));
      case "tab.rename":
        return void (ws.screen.active != null && this.renameTab(ws.screen.active));
      case "broadcast.toggle":
        return this.toggleBroadcast();
      case "watch.toggle":
        return this.toggleWatch();
      case "files.open":
        return this.openFiles();
      case "tunnels.open":
        return this.openTunnels();
      case "group.save":
        return void this.saveTabToSpace();
      case "palette.open":
        return this.openPalette();
      case "search":
        return this.openPalette(["server"]);
      case "snippets.open":
        return this.snippetsDialog();
      case "space.new":
        return void newSpace(this.spacesCtx());
      case "server.add":
        return void this.serverForm(null);
      case "ssh.import":
        return void this.importSsh();
      case "secret.new":
        return void this.newSecret();
      case "vault.lock":
        return void this.lockVault();
      case "sounds.open":
        return soundsDialog(this.sound);
      case "agents.settings":
        return void agentKindsDialog(this.be);
      case "ai.connect":
        return void connectAiDialog(this.be, () => void this.refreshAi());
      case "audit.open":
        return void auditDialog(this.be);
      case "sudo.fill":
        return void this.fillSudo();
      case "rpanel.toggle":
        if (this.focusMode) this.setFocusMode(false);
        return this.rpanel.toggle();
      case "focus.toggle":
        return this.setFocusMode(!this.focusMode);
      case "attention.next":
        return this.jumpToAttention();
      case "agent.new":
        return this.newAgent(null);
      case "panel.spaces":
      case "panel.servers":
      case "panel.secrets":
      case "panel.tunnels":
      case "panel.accounts":
        return this.toggleSection(arg as Exclude<Section, "">);
      case "sidebar.toggle":
        if (this.focusArea === "terminal") {
          if (this.leftSection !== "servers" || this.side.hidden) {
            if (this.focusMode) this.setFocusMode(false);
            this.leftSection = "servers";
            storeSet("oso.ui.left", "servers");
            this.applyChrome();
            this.renderRail();
            this.renderSidebar();
          }
          this.focusArea = "sidebar";
          (document.activeElement as HTMLElement | null)?.blur();
        } else {
          this.focusArea = "terminal";
          this.focusActive();
        }
        return;
      case "settings":
        return this.settingsDialog();
      case "help.open":
        return this.shortcutsSheet();
      case "secrets.open":
        return void this.vaultDialog();
      case "local.new":
        return void this.openServer(LOCAL);
    }
    if (kind === "focus" && dir) {
      if (!ws.moveFocus(dir[0], dir[1]) && dir[0]) ws.step(dir[0]);
      return;
    }
    if (kind === "move" && dir) {
      if (ws.screen.mode !== "mosaic") return toast(`Moving terminals needs a mosaic (${this.keyText("mode.toggle")} joins every tab into one)`);
      if (!ws.moveActive(dir[0], dir[1])) toast("Already at that edge");
    }
  }

  // ------------------------------------------------------------ dragging

  private startResize(e: PointerEvent, handle: HTMLElement, path: boolean[], side: boolean, whole: Rect) {
    e.preventDefault();
    handle.classList.add("drag");
    const rect = this.stage.getBoundingClientRect();
    const move = (m: PointerEvent) => {
      const pos = side ? m.clientX - rect.left - whole.x : m.clientY - rect.top - whole.y;
      const len = side ? whole.w : whole.h;
      this.ws.resize(path, (pos / len) * 1000);
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  /** A host (or folder) of the left list picked up: dropped on the stage it opens there, on a folder it moves in. */
  private startHostDrag(e: PointerEvent, kind: "server" | "folder", id: number, row: HTMLElement) {
    if (e.button !== 0) return;
    const sx = e.clientX;
    const sy = e.clientY;
    let ghost: HTMLElement | null = null;
    let target: { sid: Id | null; side: Side | null } | null = null;
    let folder: HTMLElement | null = null;
    let toTop = false;
    const label = row.querySelector("b")?.textContent ?? "";
    const reset = () => {
      this.treeEl.querySelectorAll(".drop-target").forEach((x) => x.classList.remove("drop-target"));
      this.hint.style.display = "none";
    };
    const move = (m: PointerEvent) => {
      if (!ghost && Math.hypot(m.clientX - sx, m.clientY - sy) < 6) return;
      if (!ghost) {
        ghost = el("div", { class: "drag-ghost" }, kind === "server" ? el("span", { class: "dot", style: `background:${serverColor(id)}` }) : "", label);
        document.body.append(ghost);
        document.body.style.cursor = "grabbing";
      }
      ghost.style.left = `${m.clientX + 12}px`;
      ghost.style.top = `${m.clientY + 12}px`;
      reset();
      target = null;
      folder = null;
      toTop = false;
      const over = document.elementFromPoint(m.clientX, m.clientY) as HTMLElement | null;
      const fr = over?.closest(".row.folder") as HTMLElement | null;
      if (fr && Number(fr.dataset.id) !== id) {
        folder = fr;
        fr.classList.add("drop-target");
      } else if (over?.closest("#tree") && !over.closest(".row")) {
        toTop = true;
      } else if (kind === "server" && !over?.closest("#side")) {
        target = this.dropTarget(m.clientX, m.clientY, null);
        if (!target && this.ws.screen.order.length === 0 && over?.closest("#stage")) this.showHint({ x: 0, y: 0, w: this.stage.clientWidth, h: this.stage.clientHeight });
      }
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      document.body.style.cursor = "";
      const wasDragging = !!ghost;
      ghost?.remove();
      const drop = { target, folder, toTop };
      reset();
      if (!wasDragging) return;
      // the click that follows the drop must not open or select anything
      const swallow = (c: Event) => c.stopPropagation();
      window.addEventListener("click", swallow, { capture: true, once: true });
      setTimeout(() => window.removeEventListener("click", swallow, true), 0);
      if (drop.folder) void this.be.moveInto(kind, id, Number(drop.folder.dataset.id)).then(() => this.refreshTree());
      else if (drop.toTop) void this.be.moveInto(kind, id, null).then(() => this.refreshTree());
      else if (kind === "server") {
        const t = drop.target;
        const place: Place | undefined = t?.sid != null ? { target: t.sid, side: t.side ?? this.autoSideFor(t.sid) } : undefined;
        void this.openServer(id, true, place);
      }
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  private autoSideFor(sid: Id): Side {
    const b = this.ws.boxes().boxes.find((x) => x.id === sid);
    return b ? autoSide(b.rect) : "right";
  }

  /** A tab (from the bar) or a terminal (by its title) picked up to be placed somewhere else. */
  private startDrag(e: PointerEvent, sid: Id, fromBar: boolean) {
    if (e.button !== 0) return;
    this.drag = { sid, fromBar, startX: e.clientX, startY: e.clientY, active: false, target: null, toBar: false };
    const move = (m: PointerEvent) => this.dragMove(m);
    const up = (u: PointerEvent) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      this.dragEnd(u);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  private dragMove(m: PointerEvent) {
    const d = this.drag;
    if (!d) return;
    if (!d.active && Math.hypot(m.clientX - d.startX, m.clientY - d.startY) < 6) return;
    d.active = true;
    document.body.style.cursor = "grabbing";
    d.target = null;
    d.toBar = false;
    this.tabsEl.classList.remove("drop");
    this.hint.style.display = "none";
    const group = this.ws.screen.group;
    const inMosaic = !d.fromBar && !!group && leaves(group.tree).length > 1 && leaves(group.tree).includes(d.sid);
    const overBar = inMosaic && !!(document.elementFromPoint(m.clientX, m.clientY) as HTMLElement | null)?.closest(".tabs");
    if (overBar) {
      d.toBar = true;
      this.tabsEl.classList.add("drop");
    }
    if (!overBar) {
      d.target = this.dropTarget(m.clientX, m.clientY, d.sid);
    }
  }

  /** Where something dragged over the stage would dock (beside the terminal under it); shows the hint. */
  private dropTarget(cx: number, cy: number, sid: Id | null): { sid: Id | null; side: Side | null } | null {
    const sr = this.stage.getBoundingClientRect();
    const x = cx - sr.left;
    const y = cy - sr.top;
    this.hint.style.display = "none";
    if (x < 0 || y < 0 || x > sr.width || y > sr.height) return null;
    const box = this.ws.boxes().boxes.find((b) => x >= b.rect.x && x < b.rect.x + b.rect.w && y >= b.rect.y && y < b.rect.y + b.rect.h);
    if (!box || box.id === sid) return null;
    const side = dockSide(box.rect, x, y);
    this.showHint(dockZone(box.rect, side));
    return { sid: box.id, side };
  }

  private showHint(r: Rect) {
    Object.assign(this.hint.style, { display: "block", left: `${r.x}px`, top: `${r.y}px`, width: `${r.w}px`, height: `${r.h}px` });
  }

  private dragEnd(_u: PointerEvent) {
    const d = this.drag;
    this.drag = null;
    document.body.style.cursor = "";
    this.hint.style.display = "none";
    this.tabsEl.classList.remove("drop");
    if (!d?.active) return;
    this.justDragged = true;
    setTimeout(() => (this.justDragged = false), 0);
    if (d.toBar) return this.ws.dropIntoBar(d.sid);
    if (d.target?.sid == null) return;
    const same = this.ws.screen.indexOf(d.sid) === this.ws.screen.indexOf(d.target.sid);
    if (same) this.ws.dropOn(d.sid, d.target.sid, d.target.side);
    else this.ws.dropGroup(d.sid, d.target.sid, d.target.side);
  }
}

/** "Alt+Shift+2" from a key press, by key position. `null` for a key that cannot be a shortcut alone. */
export function comboFromEvent(e: KeyboardEvent): string | null {
  const mods = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Super"].filter(Boolean) as string[];
  let key = "";
  if (/^Digit\d$/.test(e.code)) key = e.code.slice(5);
  else if (/^Key[A-Z]$/.test(e.code)) key = e.code.slice(3);
  else if (/^F\d{1,2}$/.test(e.code)) key = e.code;
  else {
    const named: Record<string, string> = { ArrowLeft: "Left", ArrowRight: "Right", ArrowUp: "Up", ArrowDown: "Down", Comma: ",", Period: ".", Slash: "/", Backslash: "\\", Minus: "-", Equal: "=", Semicolon: ";", Quote: "'", BracketLeft: "[", BracketRight: "]", Backquote: "`", Enter: "Enter", Tab: "Tab", Space: "Space", Escape: "Escape", Home: "Home", End: "End", PageUp: "PageUp", PageDown: "PageDown" };
    key = named[e.code] ?? "";
  }
  if (!key) return null;
  const text = [...mods, key].join("+");
  return parseCombo(text) ? text : null;
}


/** Self-test only: a terminal that is a tab of its own (no split asked for). */
async function openTabAlone(app: App) {
  await app.openServer(LOCAL);
}
