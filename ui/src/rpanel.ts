// The right panel: the open sessions, and what AI agents asked of oso. It is a view over things oso already keeps
// (the tabs, and `audit.jsonl`), so nothing here decides anything; the only thing it can do is answer a request that
// is waiting, the same answer the window gives. Nothing secret reaches it: the log is blanked of the vault's values
// before it is written.

import { Activity, ActivityDetail, ApprovalDecision, ApprovalRequest, Backend } from "./api";
import { AgentState, SavedAgent } from "./agents";
import { hasSudo, writeCommand } from "./sudo";
import { ApprovalQueue, actionTitle } from "./agents_ui";
import { clear, el } from "./dom";
import { icon } from "./icons";

export const PANEL_MIN = 240;
export const PANEL_MAX = 560;
export const PANEL_DEFAULT = 320;
/** The terminals in the middle never get narrower than this because of the panel. */
const CENTER_MIN = 360;
const SIDE_WIDTH = 264;

export function clampWidth(w: number, windowWidth = Infinity): number {
  const room = Number.isFinite(windowWidth) ? windowWidth - SIDE_WIDTH - CENTER_MIN : PANEL_MAX;
  const max = Math.max(PANEL_MIN, Math.min(PANEL_MAX, room));
  const n = Number.isFinite(w) ? w : PANEL_DEFAULT;
  return Math.round(Math.min(max, Math.max(PANEL_MIN, n)));
}

// ------------------------------------------------------------------ words

/** "claude-code" → "Claude"; nothing → "An AI agent". */
export function agentName(agent: string): string {
  const first = agent.split(/[^a-zA-Z0-9]/)[0]?.toLowerCase() ?? "";
  if (!first) return "An AI agent";
  return first[0].toUpperCase() + first.slice(1);
}

/** "just now", "5 min ago", "2 h ago", "3 d ago". */
export function ago(ts: number, now: number): string {
  const s = Math.max(0, Math.round(now - ts));
  if (s < 45) return "just now";
  if (s < 3600) return `${Math.max(1, Math.round(s / 60))} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  return `${Math.round(s / 86400)} d ago`;
}

/** How long a session has been open: "just now", "12 min", "2 h 10 min", "3 d". */
export function openFor(openedMs: number | undefined, nowMs: number): string {
  if (openedMs == null) return "";
  const m = Math.max(0, Math.floor((nowMs - openedMs) / 60000));
  if (m < 1) return "just opened";
  if (m < 60) return `${m} min`;
  if (m < 1440) return `${Math.floor(m / 60)} h${m % 60 ? ` ${m % 60} min` : ""}`;
  return `${Math.floor(m / 1440)} d`;
}

export type Tone = "you" | "auto" | "no" | "blocked";

/** Who decided: you, the server's policy, or the policy refusing. */
export function decisionLabel(a: Activity): { text: string; tone: Tone } {
  switch (a.decision) {
    case "allowed":
      return { text: "You allowed it", tone: "you" };
    case "allowed_and_remembered_for_reads":
      return { text: "You allowed it, and reads from now on", tone: "you" };
    case "allowed_without_asking":
      return { text: a.policy === "read_only_auto" ? "Automatic: read-only policy" : "Automatic", tone: "auto" };
    case "denied":
      return { text: "You said no", tone: "no" };
    case "blocked":
      return { text: "Blocked by the server's policy", tone: "blocked" };
    default:
      return { text: a.decision.replace(/_/g, " "), tone: "auto" };
  }
}

/** How it ended, in a few words: "exit 0", "running…", "failed". */
export function resultLabel(a: Activity): string {
  switch (a.state) {
    case "running":
      return "running…";
    case "finished":
      return a.exit_code == null ? "timed out" : `exit ${a.exit_code}`;
    case "failed":
      return "could not run";
    case "denied":
      return "not run";
    case "blocked":
      return "not run";
    case "unknown":
      return "no result";
    default:
      return "";
  }
}

/** Where a request came from: "Claude · pane 2 (web-1)". */
export function whoLabel(a: Activity, tab?: string): string {
  return `${agentName(a.agent)} · ${tab ?? "terminal closed"}`;
}

/** What the action is, for the line under the name. Commands show as themselves. */
export function actionWord(a: Activity): string {
  switch (a.action) {
    case "sudo":
      return "sudo";
    case "run_with":
      return "with secrets";
    case "tunnel":
      return "tunnel";
    default:
      return "";
  }
}

/** The newest `updated` of any request: the panel's "something happened" mark compares it to what was last seen. */
export function latestUpdate(list: Activity[]): number {
  return list.reduce((m, a) => Math.max(m, a.updated, a.ts), 0);
}

export type Mark = "pending" | "new" | "none";

/** What the small mark on the arrow says while the panel is closed: amber for a request waiting, a dot for news. */
export function markFor(pending: number, latest: number, seen: number): Mark {
  if (pending > 0) return "pending";
  return latest > seen ? "new" : "none";
}

// ------------------------------------------------------------------ agents

/** One line of the Agents tab. */
export interface AgentRow {
  /** The terminal it runs in (its session id). */
  sid: number;
  /** "Claude", "Opencode"… */
  name: string;
  /** The tab as the person sees it: "tab 1 · Local · me". */
  where: string;
  folder: string;
  branch: string;
  state: AgentState;
  /** When it got into this state (ms). */
  since: number;
  /** The Space it belongs to ("" when none). */
  space: string;
  /** How the state was learned: `approval`, `hook`, `process`, or `screen` (guessed). */
  source: string;
  /** What kind of agent (its id in Settings → Agents) and the full folder: what is remembered of it. */
  kind: string;
  cwd: string;
  spaceId: number | null;
}

/** Each agent has its own colour, so that two of them are told apart at a glance: neighbours in the list are far apart on the colour wheel. */
export function agentColor(sid: number): string {
  const hues = [210, 30, 150, 330, 55, 270, 180, 0, 100, 300];
  return `hsl(${hues[Math.abs(sid) % hues.length]} 70% 62%)`;
}

export const STATE_WORD: Record<AgentState, string> = { needs_you: "needs you", working: "working", finished: "finished", idle: "idle" };
const STATE_ORDER: AgentState[] = ["needs_you", "working", "finished", "idle"];

/** Needs you first, then working, finished, idle; inside a state the one that has waited longest first. */
export function sortAgents(rows: AgentRow[]): AgentRow[] {
  return [...rows].sort((a, b) => STATE_ORDER.indexOf(a.state) - STATE_ORDER.indexOf(b.state) || a.since - b.since || a.sid - b.sid);
}

// ------------------------------------------------------------------ the panel

export interface PanelCtx {
  be: Backend;
  queue: ApprovalQueue;
  /** The agents to list (what the engine knows, with the screen's say where it knows nothing else). */
  agents(): AgentRow[];
  /** The agents remembered from other runs that are not running now. */
  savedAgents(): SavedAgent[];
  /** Opens a terminal in the folder of a remembered agent and starts it again. */
  resumeAgent(s: SavedAgent): void;
  /** Takes a remembered agent off the list. */
  forgetAgent(s: SavedAgent): void;
  /** Brings the Agents section of the left panel forward (a request came in). */
  reveal(): void;
  /** The mark of the rail button may have changed. */
  markChanged(): void;
  /** "+ New agent". */
  newAgent(): void;
  /** Goes to the tab of this terminal. */
  focus(sid: number): void;
  /** The tab as the person sees it ("tab 1 · Local · me"), or `undefined` when that terminal is closed. */
  tabLabel(pane: number): string | undefined;
  /** In focus mode no panel is shown and nothing opens one by itself. */
  focusMode(): boolean;
  isProdHost(name: string): boolean;
  /** The key written for an action, for the arrow's tooltip. */
  keyText(action: string): string;
}

const store = {
  get(k: string): string | null {
    try {
      return localStorage.getItem(k);
    } catch {
      return null;
    }
  },
  set(k: string, v: string) {
    try {
      localStorage.setItem(k, v);
    } catch {
      /* private window: it just will not be remembered */
    }
  },
};

export class RightPanel {
  /** The panel itself, and the arrow on its edge (both go inside `#app`). */
  readonly el = el("aside", { id: "rpanel", "aria-label": "Agents and AI activity", hidden: true });
  private body = el("div", { class: "rp-inner" });
  /** Whether the Agents section of the left panel is the one on show (the app sets it). */
  open = false;
  /** Which of its two tabs is on show. */
  tab: "agents" | "activity" = store.get("oso.rpanel.tab") === "activity" ? "activity" : "agents";
  private activity: Activity[] = [];
  private loaded = false;
  /** The newest thing the person has seen in the panel. */
  private seen = Number(store.get("oso.rpanel.seen") ?? "0") || 0;
  private expanded = new Set<string>();
  /** The whole command and output of the cards that were opened (`null`: the log has no more than the preview). */
  private details = new Map<number, ActivityDetail | null | undefined>();
  private sig = "";
  private scroll = { s: 0, a: 0 };
  private agentRows: AgentRow[] = [];
  private openListeners: ((open: boolean) => void)[] = [];

  constructor(private ctx: PanelCtx) {
    this.el.append(this.body);
    ctx.queue.onChange(() => {
      this.render();
      void this.refresh();
    });
    // a request that comes in opens the panel on Activity, where it can be answered (not in focus mode: that hides it)
    ctx.queue.onPush(() => {
      if (ctx.focusMode()) return;
      this.tab = "activity";
      store.set("oso.rpanel.tab", "activity");
      ctx.reveal();
      this.render(true);
    });
  }

  /** Puts it inside the left panel (it is only shown while the AI agents section is open). */
  attach(parent: HTMLElement) {
    parent.append(this.el);
    this.render(true);
  }

  onOpenChange(f: (open: boolean) => void) {
    this.openListeners.push(f);
    f(this.open);
  }

  setOpen(on: boolean) {
    if (on === this.open) return;
    this.open = on;
    this.el.hidden = !on;
    if (on) this.markSeen();
    this.openListeners.forEach((f) => f(on));
    this.render(true);
    if (on) void this.refresh();
  }

  // -------------------------------------------------------------- data

  /** Reads the log again (cheap: only the end of the file). */
  async refresh() {
    const list = await this.ctx.be.agentActivity(60).catch(() => null);
    if (!list) return;
    this.activity = list;
    const latest = latestUpdate(list);
    if (!this.loaded) {
      this.loaded = true;
      // the first time ever there is nothing "new": what is already there is old news
      if (store.get("oso.rpanel.seen") == null) this.markSeen(latest);
    }
    if (this.open) this.markSeen(latest);
    this.updateMark();
    this.render();
  }

  private markSeen(latest = latestUpdate(this.activity)) {
    if (latest > this.seen) {
      this.seen = latest;
      store.set("oso.rpanel.seen", String(latest));
    }
  }

  /** The mark on the rail button: amber for a request waiting, a dot for news; the number of agents is the app's. */
  mark(): Mark {
    return this.open ? "none" : markFor(this.ctx.queue.pending, latestUpdate(this.activity), this.seen);
  }

  private updateMark() {
    this.ctx.markChanged();
  }

  // -------------------------------------------------------------- drawing

  /** What the log has of one request; `undefined` while it is being read. */
  private detail(id: number): ActivityDetail | null | undefined {
    if (!this.details.has(id)) {
      this.details.set(id, undefined);
      void this.ctx.be.activityDetail(id).then(
        (d) => {
          this.details.set(id, d);
          this.render(true);
        },
        () => {
          this.details.set(id, null);
          this.render(true);
        },
      );
    }
    return this.details.get(id);
  }

  /** Shows one of the two tabs. */
  showTab(tab: "agents" | "activity") {
    this.tab = tab;
    store.set("oso.rpanel.tab", tab);
    this.render(true);
  }

  /** Draws it again when something it shows changed (so a selection or a scroll is not lost every second). */
  render(force = false) {
    this.updateMark();
    if (!this.open) return;
    const now = Date.now();
    const nowS = Math.floor(now / 1000);
    const pending = this.ctx.queue.list();
    this.agentRows = sortAgents(this.ctx.agents());
    const sig = JSON.stringify([this.tab, this.agentRows.map((r) => [r.sid, r.name, r.where, r.folder, r.branch, r.state, openFor(r.since, now), r.space]), this.ctx.savedAgents().map((s) => [s.cwd, s.kind, openFor(s.last, now)]), pending.map((r) => r.id), this.ctx.queue.allowAll, this.activity.map((a) => [a.id, a.ts, a.updated, a.state, ago(a.ts, nowS)]), [...this.expanded]]);
    if (!force && sig === this.sig) return;
    this.sig = sig;
    const cur = this.body.querySelector(".rp-scroll") as HTMLElement | null;
    if (cur) this.scroll[this.tab === "agents" ? "s" : "a"] = cur.scrollTop;
    clear(this.body);
    const needs = this.agentRows.filter((r) => r.state === "needs_you").length;
    const tabBtn = (id: "agents" | "activity", label: string, badge: number) => {
      const b = el("button", { class: `rp-tab${this.tab === id ? " on" : ""}`, type: "button", role: "tab", "data-rtab": id, "aria-selected": String(this.tab === id) }, label);
      if (badge) b.append(el("span", { class: "pending-n" }, String(badge)));
      b.onclick = () => this.showTab(id);
      return b;
    };
    this.body.append(el("div", { class: "rp-tabs", role: "tablist" }, tabBtn("agents", "Agents", needs), tabBtn("activity", "Activity", pending.length)));
    this.body.append(this.tab === "agents" ? this.agentsSection(now) : this.activitySection(pending, nowS));
    const next = this.body.querySelector(".rp-scroll") as HTMLElement | null;
    if (next) next.scrollTop = this.scroll[this.tab === "agents" ? "s" : "a"];
  }

  private agentsSection(now: number): HTMLElement {
    const list = el("div", { class: "rp-scroll" });
    const add = el("button", { class: "btn small", type: "button", "data-act": "agents-new", title: `New agent (${this.ctx.keyText("agent.new")})` }, "+ New agent");
    add.onclick = () => this.ctx.newAgent();
    list.append(el("div", { class: "rp-bar" }, add));
    if (!this.agentRows.length) list.append(el("div", { class: "rp-empty" }, "No agent is running. Start one from a Space (New agent), or run claude, opencode or codex in a terminal of oso: it shows up here with what it is doing."));
    // two groups: the ones doing something (or asking for you) and the ones standing there
    const busy = (r: AgentRow) => r.state === "needs_you" || r.state === "working";
    const groups: [string, string, AgentRow[]][] = [
      ["parked", "Standing by", this.agentRows.filter((r) => !busy(r))],
      ["busy", "Working", this.agentRows.filter(busy)],
    ];
    for (const [id, title, rows] of groups) {
      if (!rows.length) continue;
      list.append(el("h4", { class: "rp-space", "data-group": id }, title, el("span", { class: "n" }, String(rows.length))));
      for (const r of rows) list.append(this.agentRow(r, now));
    }
    const saved = this.ctx.savedAgents();
    if (saved.length) {
      list.append(el("h4", { class: "rp-space", "data-group": "saved", title: "Agents from before: click to open their folder and start them again" }, "Not running", el("span", { class: "n" }, String(saved.length))));
      for (const s of saved) list.append(this.savedRow(s, now));
    }
    return el("section", { class: "rp-sec rp-agents", "data-section": "agents" }, list);
  }

  private savedRow(s: SavedAgent, now: number): HTMLElement {
    const forget = el("button", { class: "rp-forget", type: "button", title: "Take it off the list", "aria-label": "Forget this agent", "data-act": "agent-forget" }, "×");
    const row = el(
      "div",
      { class: "rp-agent saved", style: `--ag: ${agentColor(s.cwd.length)}`, "data-saved": s.cwd, role: "button", tabindex: "0", title: `${s.cwd} · click to open it and start ${s.name}` },
      el("span", { class: "sdot" }),
      el("div", { class: "txt" }, el("b", { class: "repo" }, s.folder || s.cwd), el("small", { class: "who" }, s.name), s.branch ? el("small", {}, `⎇ ${s.branch}`) : null, el("small", {}, `last seen ${openFor(s.last, now) === "just opened" ? "just now" : `${openFor(s.last, now)} ago`}`)),
      forget,
    );
    forget.onclick = (e) => {
      e.stopPropagation();
      this.ctx.forgetAgent(s);
    };
    row.onclick = () => this.ctx.resumeAgent(s);
    row.onkeydown = (e) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        this.ctx.resumeAgent(s);
      }
    };
    return row;
  }

  private agentRow(r: AgentRow, now: number): HTMLElement {
    const repo = r.folder || r.where;
    const meta = [r.branch ? `⎇ ${r.branch}` : "", openFor(r.since, now)].filter(Boolean).join(" · ");
    const row = el(
      "div",
      { class: `rp-agent ${r.state}`, style: `--ag: ${agentColor(r.sid)}`, "data-agent-row": String(r.sid), "data-state": r.state, role: "button", tabindex: "0", title: `${r.where}${r.source === "screen" ? " · guessed from the screen" : r.source === "process" ? " · only known to be running" : ""}` },
      el("span", { class: "sdot" }),
      el(
        "div",
        { class: "txt" },
        el("b", { class: "repo" }, repo, el("span", { class: "st" }, STATE_WORD[r.state])),
        el("small", { class: "who" }, r.name, r.space ? ` · ${r.space}` : ""),
        el("small", {}, r.where),
        meta ? el("small", {}, meta) : null,
      ),
    );
    row.onclick = () => this.ctx.focus(r.sid);
    row.onkeydown = (e) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        this.ctx.focus(r.sid);
      }
    };
    return row;
  }

  private activitySection(pending: ApprovalRequest[], nowS: number): HTMLElement {
    const list = el("div", { class: "rp-scroll" });
    const q = this.ctx.queue;
    const mode = (on: boolean, text: string, tip: string) => {
      const b = el("button", { class: `seg${q.allowAll === on ? " on" : ""}`, type: "button", title: tip, "aria-pressed": String(q.allowAll === on), "data-act": on ? "allow-all" : "ask" }, text);
      b.onclick = () => {
        q.setAllowAll(on);
        this.render();
      };
      return b;
    };
    list.append(
      el(
        "div",
        { class: `rp-mode${q.allowAll ? " all" : ""}` },
        el("div", { class: "segs", role: "group", "aria-label": "What oso does when an agent asks" }, mode(false, "Ask", "Every request waits for your answer"), mode(true, "Allow all", "Allow everything without asking, except production hosts. Until you close oso.")),
        q.allowAll ? el("small", {}, "Everything is allowed without asking, except hosts marked production. It goes back to Ask when you close oso.") : null,
      ),
    );
    for (const r of pending) list.append(this.pendingCard(r));
    const shown = this.activity.filter((a) => a.action !== "context" && a.action !== "hosts");
    for (const a of [...shown].reverse()) list.append(this.activityCard(a, nowS));
    if (!pending.length && !shown.length) list.append(el("div", { class: "rp-empty" }, "Nothing yet. When an AI agent in one of your terminals asks oso to do something, it shows up here: what, where, and how it was decided."));
    return el("section", { class: "rp-sec rp-activity", "data-section": "activity" }, list);
  }

  private pendingCard(r: ApprovalRequest): HTMLElement {
    const btn = (cls: string, text: string, d: ApprovalDecision, act: string) => {
      const b = el("button", { class: `btn ${cls}`.trim(), type: "button", "data-act": act }, text);
      b.onclick = () => void this.ctx.queue.answer(r, d);
      return b;
    };
    const buttons = el("div", { class: "rp-actions" }, btn("", "Deny", "deny", "panel-deny"), btn("", "In a terminal", "terminal", "panel-terminal"), btn("primary", "Allow once", "once", "panel-once"));
    const extra = r.can_remember ? btn("link", "Always for read-only commands on this host", "always_read", "panel-always") : null;
    return el(
      "div",
      { class: `rp-card pending${r.production ? " prod" : ""}`, "data-approval": String(r.id), role: "group", "aria-label": actionTitle(r) },
      el("div", { class: "rp-who" }, el("b", {}, actionTitle(r))),
      el("div", { class: "rp-where" }, r.host, hasSudo(r.command) ? sudoTag() : null, r.production ? el("span", { class: "badge-prod" }, "PRODUCTION") : null),
      cmdPre(r.command),
      buttons,
      extra,
    );
  }

  private activityCard(a: Activity, nowS: number): HTMLElement {
    const key = keyOf(a);
    const open = this.expanded.has(key);
    const d = decisionLabel(a);
    const res = resultLabel(a);
    const prod = a.host && this.ctx.isProdHost(a.host);
    const hasMore = !!(a.reason || a.preview || a.note || a.has_output || a.command.length > 120);
    const head = el(
      "div",
      { class: "rp-who" },
      el("b", { title: `pane ${a.pane}` }, whoLabel(a, this.ctx.tabLabel(a.pane))),
      el("span", { class: "when", title: new Date(a.ts * 1000).toLocaleString() }, ago(a.ts, nowS)),
    );
    const where = el("div", { class: `rp-where${prod ? " prod" : ""}` }, a.host || "", actionWord(a) ? el("span", { class: "kind" }, actionWord(a)) : null, hasSudo(a.command) ? sudoTag() : null, prod ? el("span", { class: "badge-prod" }, "PRODUCTION") : null);
    const chips = el("div", { class: "rp-chips" }, el("span", { class: `chip ${d.tone}`, "data-decision": a.decision }, d.text), res ? el("span", { class: `chip res ${a.state}${a.state === "finished" && a.exit_code !== 0 ? " bad" : ""}` }, res) : null);
    const card = el("div", { class: `rp-card${open ? " open" : ""}`, "data-activity": String(a.id || a.ts), "data-state": a.state }, head, where, cmdPre(a.command), chips);
    if (hasMore) {
      const more = el("button", { class: "more", type: "button", "data-act": "activity-toggle", "aria-expanded": String(open) }, open ? "Hide details" : "Details");
      more.prepend(icon(open ? "chevron-down" : "chevron-right", 12));
      more.onclick = () => {
        if (open) this.expanded.delete(key);
        else this.expanded.add(key);
        this.render(true);
      };
      card.append(more);
      if (open) {
        const detail = el("div", { class: "rp-detail" });
        if (a.reason) detail.append(el("div", { class: "why" }, a.reason));
        const facts = [a.policy ? `server policy: ${a.policy.replace(/_/g, " ")}` : "", a.exit_code != null ? `exit code ${a.exit_code}` : "", a.bytes_out != null ? `${a.bytes_out} bytes of output` : "", a.note].filter(Boolean).join(" · ");
        if (facts) detail.append(el("div", { class: "facts" }, facts));
        // the whole command and output are read from the log when a card is opened, not on every refresh
        const d = a.id && a.has_output ? this.detail(a.id) : null;
        if (d === undefined) detail.append(el("div", { class: "facts" }, "Loading…"));
        else {
          detail.append(el("div", { class: "lbl" }, "Command"), cmdPre(d?.command ?? a.command, "full"));
          const out = d ? d.output : a.preview;
          if (out) {
            detail.append(el("div", { class: "lbl" }, "Output"), el("pre", { class: "out", "data-role": "output" }, out));
            if (!d && a.preview) detail.append(el("div", { class: "facts" }, "Only the first lines were kept for this one."));
          } else if (a.state === "finished") detail.append(el("div", { class: "facts" }, "It printed nothing."));
        }
        card.append(detail);
      }
    }
    return card;
  }
}

/** A command as text, with its `sudo` words in amber. */
function cmdPre(command: string, extra = ""): HTMLElement {
  const pre = el("pre", { class: `cmd${extra ? ` ${extra}` : ""}`, "data-role": "command" });
  writeCommand(pre, command);
  return pre;
}

function sudoTag(): HTMLElement {
  return el("span", { class: "sudo-tag", title: "This command uses sudo" }, "sudo");
}

const keyOf = (a: Activity) => `${a.id || `${a.ts}:${a.pane}`}:${a.command}`;
