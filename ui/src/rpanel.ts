// The right panel: the open sessions, and what AI agents asked of oso. It is a view over things oso already keeps
// (the tabs, and `audit.jsonl`), so nothing here decides anything; the only thing it can do is answer a request that
// is waiting, the same answer the window gives. Nothing secret reaches it: the log is blanked of the vault's values
// before it is written.

import { Activity, ApprovalDecision, ApprovalRequest, Backend, TreeData } from "./api";
import { ApprovalQueue, actionTitle } from "./agents_ui";
import { clear, el } from "./dom";
import { icon, serverColor } from "./icons";

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

/** "open 12 min", or "just opened". */
export function sessionAge(openedMs: number | undefined, nowMs: number): string {
  const t = openFor(openedMs, nowMs);
  return !t || t === "just opened" ? t : `open ${t}`;
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
export function whoLabel(a: Activity, paneTitle?: string): string {
  return `${agentName(a.agent)} · pane ${a.pane}${paneTitle ? ` (${paneTitle})` : ""}`;
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

// ------------------------------------------------------------------ sessions

export interface SessionRow {
  sid: number;
  name: string;
  serverId: number;
  local: boolean;
  prod: boolean;
  dead: boolean;
  profile: string;
  /** When the window opened it (ms). */
  opened?: number;
}

/** The profile a server answers to: its own, else the one of the nearest folder up the tree, else "every profile". */
export function profileOf(tree: Pick<TreeData, "servers" | "folders" | "profiles">, serverId: number): string {
  const name = (id: string | null) => (id ? (tree.profiles.find((p) => p.id === id)?.name ?? id) : null);
  const s = tree.servers.find((x) => x.id === serverId);
  if (!s) return "";
  if (s.profile) return name(s.profile) ?? "";
  let f = s.parent != null ? tree.folders.find((x) => x.id === s.parent) : undefined;
  for (let hops = 0; f && hops < 50; hops++) {
    if (f.profile) return name(f.profile) ?? "";
    f = f.parent != null ? tree.folders.find((x) => x.id === f!.parent) : undefined;
  }
  return "every profile";
}

// ------------------------------------------------------------------ the panel

export interface PanelCtx {
  be: Backend;
  queue: ApprovalQueue;
  rows(): SessionRow[];
  focus(sid: number): void;
  close(sid: number): void;
  /** The title of the terminal an agent runs in, if it is still open. */
  paneTitle(pane: number): string | undefined;
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
  readonly el = el("aside", { id: "rpanel", "aria-label": "Sessions and AI activity", hidden: true });
  readonly arrow = el("button", { id: "rtoggle", type: "button", "data-act": "rpanel-toggle" });
  private grip = el("div", { id: "rgrip", title: "Drag to change the width" });
  private body = el("div", { class: "rp-inner" });
  open = store.get("oso.rpanel") === "open";
  width = clampWidth(Number(store.get("oso.rpanel.w") ?? PANEL_DEFAULT), window.innerWidth);
  private activity: Activity[] = [];
  private loaded = false;
  /** The newest thing the person has seen in the panel. */
  private seen = Number(store.get("oso.rpanel.seen") ?? "0") || 0;
  private expanded = new Set<string>();
  private sig = "";
  private scroll = { s: 0, a: 0 };
  private openListeners: ((open: boolean) => void)[] = [];

  constructor(private ctx: PanelCtx) {
    this.el.append(this.grip, this.body);
    this.arrow.onclick = () => this.toggle();
    this.grip.addEventListener("pointerdown", (e) => this.startResize(e));
    ctx.queue.onChange(() => {
      this.render();
      void this.refresh();
    });
    window.addEventListener("resize", () => this.apply());
  }

  /** Puts the panel and its arrow inside the window's grid. */
  attach(app: HTMLElement) {
    app.append(this.el, this.arrow);
    this.apply();
    this.render(true);
  }

  onOpenChange(f: (open: boolean) => void) {
    this.openListeners.push(f);
    f(this.open);
  }

  setOpen(on: boolean) {
    if (on === this.open) return;
    this.open = on;
    store.set("oso.rpanel", on ? "open" : "closed");
    if (on) this.markSeen();
    this.apply();
    this.openListeners.forEach((f) => f(on));
    this.render(true);
    if (on) void this.refresh();
  }

  toggle() {
    this.setOpen(!this.open);
  }

  /** Puts the width and the open/closed state on the page. */
  private apply() {
    this.width = clampWidth(this.width, window.innerWidth);
    const app = this.el.parentElement;
    this.el.hidden = !this.open;
    this.el.style.width = `${this.width}px`;
    app?.style.setProperty("--rw", this.open ? `${this.width}px` : "0px");
    this.arrow.classList.toggle("open", this.open);
    clear(this.arrow);
    this.arrow.append(icon(this.open ? "chevron-right" : "chevron-left", 14), el("span", { class: "mark" }));
    const tip = this.open ? "Close the side panel" : "Open the side panel: sessions and AI activity";
    const key = this.ctx.keyText("rpanel.toggle");
    this.arrow.title = key ? `${tip}  (${key})` : tip;
    this.arrow.setAttribute("aria-expanded", String(this.open));
    this.updateMark();
  }

  private startResize(e: PointerEvent) {
    e.preventDefault();
    const target = this.grip;
    target.setPointerCapture(e.pointerId);
    document.body.classList.add("resizing-rpanel");
    const move = (m: PointerEvent) => {
      this.width = clampWidth(window.innerWidth - m.clientX, window.innerWidth);
      this.apply();
    };
    const up = () => {
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", up);
      target.removeEventListener("pointercancel", up);
      document.body.classList.remove("resizing-rpanel");
      store.set("oso.rpanel.w", String(this.width));
    };
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", up);
    target.addEventListener("pointercancel", up);
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

  private updateMark() {
    const m = markFor(this.ctx.queue.pending, latestUpdate(this.activity), this.seen);
    const shown = this.open ? "none" : m;
    this.arrow.dataset.mark = shown;
    const dot = this.arrow.querySelector(".mark") as HTMLElement | null;
    if (dot) dot.title = shown === "pending" ? "An AI agent is waiting for your answer" : shown === "new" ? "An AI agent did something since you last looked" : "";
  }

  // -------------------------------------------------------------- drawing

  /** Draws it again when something it shows changed (so a selection or a scroll is not lost every second). */
  render(force = false) {
    this.updateMark();
    if (!this.open) return;
    const now = Date.now();
    const rows = this.ctx.rows();
    const pending = this.ctx.queue.list();
    const nowS = Math.floor(now / 1000);
    const sig = JSON.stringify([rows.map((r) => [r.sid, r.name, r.prod, r.dead, r.profile, openFor(r.opened, now)]), pending.map((r) => r.id), this.activity.map((a) => [a.id, a.ts, a.updated, a.state, ago(a.ts, nowS), this.expanded.has(keyOf(a))]), [...this.expanded]]);
    if (!force && sig === this.sig) return;
    this.sig = sig;
    const s = this.body.querySelector(".rp-sessions .rp-scroll") as HTMLElement | null;
    const a = this.body.querySelector(".rp-activity .rp-scroll") as HTMLElement | null;
    if (s) this.scroll.s = s.scrollTop;
    if (a) this.scroll.a = a.scrollTop;
    clear(this.body);
    this.body.append(this.sessionsSection(rows, now), this.activitySection(pending, nowS));
    const s2 = this.body.querySelector(".rp-sessions .rp-scroll") as HTMLElement | null;
    const a2 = this.body.querySelector(".rp-activity .rp-scroll") as HTMLElement | null;
    if (s2) s2.scrollTop = this.scroll.s;
    if (a2) a2.scrollTop = this.scroll.a;
  }

  private sessionsSection(rows: SessionRow[], now: number): HTMLElement {
    const list = el("div", { class: "rp-scroll" });
    if (!rows.length) list.append(el("div", { class: "rp-empty" }, "No open sessions. Double-click a server on the left to open one."));
    for (const r of rows) {
      const x = el("button", { class: "x", type: "button", title: "Close this session", "data-act": "session-close", "aria-label": `Close ${r.name}` });
      x.append(icon("close", 13));
      x.onclick = (e) => {
        e.stopPropagation();
        this.ctx.close(r.sid);
      };
      const where = r.local ? "this computer" : r.profile;
      const sub = [where, r.dead ? "ended" : sessionAge(r.opened, now)].filter(Boolean).join(" · ");
      const row = el(
        "div",
        { class: `rp-session${r.prod ? " prod" : ""}${r.dead ? " dead" : ""}`, "data-session": String(r.sid), role: "button", tabindex: "0", title: "Go to this terminal" },
        el("span", { class: "dot", style: `background:${serverColor(r.serverId)}` }),
        el("div", { class: "txt" }, el("b", {}, r.name, r.prod ? el("span", { class: "badge-prod" }, "PRODUCTION") : null), el("small", {}, sub)),
        x,
      );
      row.onclick = () => this.ctx.focus(r.sid);
      row.onkeydown = (e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          this.ctx.focus(r.sid);
        }
      };
      list.append(row);
    }
    return el("section", { class: "rp-sec rp-sessions", "data-section": "sessions" }, el("h3", {}, "Sessions", el("span", { class: "n" }, rows.length ? String(rows.length) : "")), list);
  }

  private activitySection(pending: ApprovalRequest[], nowS: number): HTMLElement {
    const list = el("div", { class: "rp-scroll" });
    for (const r of pending) list.append(this.pendingCard(r));
    const shown = this.activity.filter((a) => a.action !== "context" && a.action !== "hosts");
    for (const a of [...shown].reverse()) list.append(this.activityCard(a, nowS));
    if (!pending.length && !shown.length) list.append(el("div", { class: "rp-empty" }, "Nothing yet. When an AI agent in one of your terminals asks oso to do something, it shows up here: what, where, and how it was decided."));
    const badge = pending.length ? el("span", { class: "pending-n", title: "Waiting for your answer" }, String(pending.length)) : el("span", { class: "n" }, shown.length ? String(shown.length) : "");
    return el("section", { class: "rp-sec rp-activity", "data-section": "activity" }, el("h3", {}, "Agent activity", badge), list);
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
      el("div", { class: "rp-where" }, r.host, r.production ? el("span", { class: "badge-prod" }, "PRODUCTION") : null),
      el("pre", { class: "cmd" }, r.command),
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
    const hasMore = !!(a.reason || a.preview || a.note);
    const head = el(
      "div",
      { class: "rp-who" },
      el("b", {}, whoLabel(a, this.ctx.paneTitle(a.pane))),
      el("span", { class: "when", title: new Date(a.ts * 1000).toLocaleString() }, ago(a.ts, nowS)),
    );
    const where = el("div", { class: `rp-where${prod ? " prod" : ""}` }, a.host || "", actionWord(a) ? el("span", { class: "kind" }, actionWord(a)) : null, prod ? el("span", { class: "badge-prod" }, "PRODUCTION") : null);
    const chips = el("div", { class: "rp-chips" }, el("span", { class: `chip ${d.tone}`, "data-decision": a.decision }, d.text), res ? el("span", { class: `chip res ${a.state}${a.state === "finished" && a.exit_code !== 0 ? " bad" : ""}` }, res) : null);
    const card = el("div", { class: `rp-card${open ? " open" : ""}`, "data-activity": String(a.id || a.ts), "data-state": a.state }, head, where, el("pre", { class: "cmd" }, a.command), chips);
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
        if (a.preview) detail.append(el("pre", { class: "out", "data-role": "preview" }, a.preview));
        card.append(detail);
      }
    }
    return card;
  }
}

const keyOf = (a: Activity) => `${a.id || `${a.ts}:${a.pane}`}:${a.command}`;
