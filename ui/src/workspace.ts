// The workspace: one row of tabs. A tab is one terminal, or several tiled together
// (a mosaic); every terminal is born as a tab and drag and drop joins them or sets them apart. This is the whole
// state of the window and every way to change it, with no DOM in it, so it is tested on its own.

import { Id, Rect, Side, Tree, autoSide, contains, dock, dwindle, layout, leaf, leaves, mapLeaves, neighbour, remove, setPm, swap } from "./tree";

/** There is one row of tabs, like a browser's. (Older versions had nine screens; their tabs are merged on load.) */
export const SCREENS = 1;
export type Mode = "tabs" | "mosaic";

/** Where a terminal that is being opened goes: beside `target` (or the active one), on this side. */
export interface Place {
  target: Id | null;
  side: Side | null;
}

export interface Tab {
  sid: Id;
  title: string;
  serverId: number;
  screen: number;
  /** The session ended (shows "Enter: reconnect"). */
  dead: boolean;
  /** The Space it was opened in. */
  space?: number;
  /** The folder it was opened in (a local terminal). */
  cwd?: string;
}

/** One tab of a screen: a single terminal, or several tiled together. */
export interface Group {
  tree: Tree;
  /** The terminal of this tab that has (or last had) the keyboard. */
  active: Id;
  /** What is typed in one terminal of this tab goes to all of them. */
  broadcast?: boolean;
}

export class Screen {
  /** The tabs of this screen, left to right. */
  groups: Group[] = [];
  /** Which tab is on show. */
  cur = 0;

  get group(): Group | undefined {
    return this.groups[this.cur];
  }
  /** Every terminal of the screen, tab by tab. */
  get order(): Id[] {
    return this.groups.flatMap((g) => leaves(g.tree));
  }
  /** The terminal with the keyboard. */
  get active(): Id | null {
    return this.group?.active ?? null;
  }
  /** How the terminals of the tab on show are arranged. */
  get tree(): Tree | null {
    return this.group?.tree ?? null;
  }
  /** "mosaic" while the tab on show holds more than one terminal. */
  get mode(): Mode {
    return this.group && leaves(this.group.tree).length > 1 ? "mosaic" : "tabs";
  }
  indexOf(sid: Id): number {
    return this.groups.findIndex((g) => contains(g.tree, sid));
  }
}

export interface Saved {
  current: number;
  screens: {
    groups?: { tree: Tree; active: Id; broadcast?: boolean }[];
    cur?: number;
    /** Older layouts: a mode, the terminals and (mosaic) one tree for all of them. */
    mode?: Mode;
    order?: Id[];
    tree?: Tree | null;
    active?: Id | null;
  }[];
}

const emptyScreen = () => new Screen();

export class Workspace {
  tabs = new Map<Id, Tab>();
  screens: Screen[] = Array.from({ length: SCREENS }, emptyScreen);
  current = 0;
  /** The area the terminals share, in pixels (set by the view). */
  size: Rect = { x: 0, y: 0, w: 1200, h: 800 };
  /** Alt+V / Alt+H: how the next terminal opened joins the mosaic. */
  pending: Side | null = null;
  private listeners = new Set<() => void>();

  onChange(f: () => void): () => void {
    this.listeners.add(f);
    return () => this.listeners.delete(f);
  }
  private changed() {
    this.listeners.forEach((f) => f());
  }

  get screen(): Screen {
    return this.screens[this.current];
  }
  /** The terminals of the tab on show. */
  tabsHere(): Tab[] {
    const g = this.screen.group;
    return g ? leaves(g.tree).map((id) => this.tabs.get(id)!).filter(Boolean) : [];
  }
  activeTab(): Tab | undefined {
    return this.screen.active == null ? undefined : this.tabs.get(this.screen.active);
  }
  /** The box of each terminal of the tab on show. */
  boxes() {
    const g = this.screen.group;
    return g ? layout(g.tree, this.size) : { boxes: [], splits: [] };
  }
  // ------------------------------------------------------------ tabs

  /**
   * A new terminal on the screen you are on. By itself it is a new tab. After Alt+V / Alt+H, or when it is dropped
   * on a terminal (`place`), it joins the tab on show instead and the two are tiled.
   */
  addTab(tab: Omit<Tab, "screen" | "dead"> & Partial<Pick<Tab, "dead">>, place?: Place) {
    const s = this.screen;
    this.tabs.set(tab.sid, { ...tab, screen: this.current, dead: tab.dead ?? false });
    const g = s.group;
    const side = place?.side ?? this.pending;
    if (g && side) {
      const target = place?.target != null && contains(g.tree, place.target) ? place.target : g.active;
      g.tree = dock(g.tree, target, tab.sid, side);
      g.active = tab.sid;
    } else {
      s.groups.push({ tree: leaf(tab.sid), active: tab.sid });
      s.cur = s.groups.length - 1;
    }
    this.pending = null;
    this.changed();
  }

  /**
   * A whole tab at once (a saved group being opened): `shape` is the arrangement with the terminals numbered
   * 0, 1, 2… in the order of `tabs`.
   */
  addGroup(tabs: (Omit<Tab, "screen" | "dead"> & Partial<Pick<Tab, "dead">>)[], shape: Tree, broadcast = false) {
    if (tabs.length === 0) return;
    const s = this.screen;
    for (const t of tabs) this.tabs.set(t.sid, { ...t, screen: this.current, dead: t.dead ?? false });
    const tree = mapLeaves(shape, (i) => tabs[i]?.sid ?? tabs[0].sid);
    const ids = leaves(tree);
    // a shape that does not match the terminals (an old file edited by hand) falls back to an automatic layout
    const ok = ids.length === tabs.length && new Set(ids).size === ids.length;
    s.groups.push({ tree: ok ? tree : dwindle(tabs.map((t) => t.sid), this.size)!, active: tabs[0].sid, broadcast: broadcast && tabs.length > 1 });
    s.cur = s.groups.length - 1;
    this.pending = null;
    this.changed();
  }

  /** The arrangement of the tab on show with its terminals numbered 0, 1, 2… (to save it as a group). */
  shape(): { shape: Tree; sids: Id[] } | null {
    const g = this.screen.group;
    if (!g) return null;
    const sids = leaves(g.tree);
    return { shape: mapLeaves(g.tree, (id) => sids.indexOf(id)), sids };
  }

  /** Whether what is typed goes to every terminal of the tab on show. */
  get broadcasting(): boolean {
    const g = this.screen.group;
    return !!g?.broadcast && leaves(g.tree).length > 1;
  }

  /** The terminals that get what is typed in `sid`: all of its tab if that tab is in broadcast, else nobody. */
  broadcastTeam(sid: Id): Id[] | null {
    const g = this.screen.groups[this.screen.indexOf(sid)];
    return g?.broadcast && leaves(g.tree).length > 1 ? leaves(g.tree) : null;
  }

  setBroadcast(on: boolean) {
    const g = this.screen.group;
    if (!g) return;
    g.broadcast = on && leaves(g.tree).length > 1;
    this.changed();
  }

  /** Takes a terminal out of its tab (the tab goes if it was the only one). It stays registered. */
  private detach(sid: Id) {
    const tab = this.tabs.get(sid);
    if (!tab) return;
    const s = this.screens[tab.screen];
    const gi = s.indexOf(sid);
    if (gi < 0) return;
    const g = s.groups[gi];
    const rest = remove(g.tree, sid);
    if (rest) {
      g.tree = rest;
      if (leaves(rest).length < 2) g.broadcast = false;
      if (g.active === sid) g.active = leaves(rest).at(-1)!;
      return;
    }
    s.groups.splice(gi, 1);
    if (s.cur > gi) s.cur--;
    else if (s.cur === gi) s.cur = Math.max(0, gi - 1);
    s.cur = Math.min(s.cur, Math.max(0, s.groups.length - 1));
  }

  closeTab(sid: Id) {
    if (!this.tabs.has(sid)) return;
    this.detach(sid);
    this.tabs.delete(sid);
    this.changed();
  }

  /** Gives a terminal the keyboard; goes to its screen and its tab first if it is somewhere else. */
  focus(sid: Id) {
    const tab = this.tabs.get(sid);
    if (!tab) return;
    if (tab.screen !== this.current) this.current = tab.screen;
    const s = this.screen;
    const gi = s.indexOf(sid);
    if (gi < 0) return;
    s.cur = gi;
    s.groups[gi].active = sid;
    this.changed();
  }

  /** Alt+←/→ when there is nothing at that side: the previous or next tab of this screen. */
  step(d: number) {
    const s = this.screen;
    if (s.groups.length === 0) return;
    s.cur = (s.cur + d + s.groups.length) % s.groups.length;
    this.changed();
  }

  setTitle(sid: Id, title: string) {
    const t = this.tabs.get(sid);
    if (t) {
      t.title = title;
      this.changed();
    }
  }
  setDead(sid: Id, dead: boolean) {
    const t = this.tabs.get(sid);
    if (t && t.dead !== dead) {
      t.dead = dead;
      this.changed();
    }
  }

  /** A session was restarted in place: the tab keeps its box and takes the new session. */
  rekey(oldSid: Id, newSid: Id) {
    const t = this.tabs.get(oldSid);
    if (!t) return;
    const s = this.screens[t.screen];
    this.tabs.delete(oldSid);
    this.tabs.set(newSid, { ...t, sid: newSid, dead: false });
    for (const g of s.groups) {
      if (contains(g.tree, oldSid)) g.tree = swapIn(g.tree, oldSid, newSid);
      if (g.active === oldSid) g.active = newSid;
    }
    this.changed();
  }

  /** Alt+N: tab N (Alt+9 the last one), like a browser. */
  goTab(n: number) {
    const s = this.screen;
    if (s.groups.length === 0) return;
    const i = n >= 9 ? s.groups.length - 1 : n - 1;
    if (i < 0 || i >= s.groups.length || i === s.cur) return;
    s.cur = i;
    this.pending = null;
    this.changed();
  }

  // ------------------------------------------------------------ joining and splitting

  /** Alt+M: every tab of the screen into one mosaic; if it already is one, back into separate tabs. */
  toggleMode() {
    const s = this.screen;
    if (s.groups.length > 1) {
      const active = s.active;
      const ids = s.order;
      s.groups = [{ tree: dwindle(ids, this.size)!, active: active ?? ids[0] }];
      s.cur = 0;
    } else if (s.group && leaves(s.group.tree).length > 1) {
      const active = s.active;
      s.groups = leaves(s.group.tree).map((id) => ({ tree: leaf(id), active: id }));
      s.cur = Math.max(0, s.groups.findIndex((g) => g.active === active));
    } else return;
    this.changed();
  }

  // ------------------------------------------------------------ mosaic

  /** Alt+V / Alt+H: the next terminal opened joins this tab, to the right of / below the active one. */
  prepareSplit(side: "right" | "bottom") {
    this.pending = side;
  }

  /** Alt+arrows: the keyboard goes to the terminal on that side. False if there is none. */
  moveFocus(dx: number, dy: number): boolean {
    const { boxes } = this.boxes();
    const at = boxes.findIndex((b) => b.id === this.screen.active);
    const to = at < 0 ? null : neighbour(boxes.map((b) => b.rect), at, dx, dy);
    if (to == null) return false;
    this.focus(boxes[to].id);
    return true;
  }

  /** Alt+Shift+arrows: the active terminal trades places with the one on that side. */
  moveActive(dx: number, dy: number): boolean {
    const g = this.screen.group;
    const { boxes } = this.boxes();
    const at = boxes.findIndex((b) => b.id === g?.active);
    const to = at < 0 ? null : neighbour(boxes.map((b) => b.rect), at, dx, dy);
    if (to == null || !g) return false;
    g.tree = swap(g.tree, boxes[at].id, boxes[to].id);
    this.changed();
    return true;
  }

  private sideFor(g: Group, target: Id): Side {
    const rect = layout(g.tree, this.size).boxes.find((b) => b.id === target)?.rect ?? this.size;
    return autoSide(rect);
  }

  /**
   * A terminal dropped on a box: near the edge it docks there taking half of it; in the middle it trades places
   * with that box (if they are in the same tab). One that comes from another tab joins this one.
   */
  dropOn(sid: Id, target: Id, side: Side | null) {
    const s = this.screen;
    const tab = this.tabs.get(sid);
    if (!tab || tab.screen !== this.current || sid === target || s.indexOf(target) < 0) return;
    if (s.indexOf(sid) === s.indexOf(target)) {
      const g = s.group!;
      if (side == null) g.tree = swap(g.tree, sid, target);
      else {
        const rest = remove(g.tree, sid);
        if (!rest) return;
        g.tree = dock(rest, target, sid, side);
      }
      g.active = sid;
    } else {
      this.detach(sid);
      const gi = s.indexOf(target);
      const g = s.groups[gi];
      g.tree = dock(g.tree, target, sid, side ?? this.sideFor(g, target));
      g.active = sid;
      s.cur = gi;
    }
    this.changed();
  }

  /** A tab dragged from the bar onto a terminal: all of its terminals join that tab. */
  dropGroup(sid: Id, target: Id, side: Side | null) {
    const s = this.screen;
    const si = s.indexOf(sid);
    const ti = s.indexOf(target);
    if (si < 0 || ti < 0 || si === ti) return;
    const src = s.groups[si];
    const ids = leaves(src.tree);
    s.groups.splice(si, 1);
    const gi = s.indexOf(target);
    const g = s.groups[gi];
    let beside = target;
    let where: Side = side ?? this.sideFor(g, target);
    for (const id of ids) {
      g.tree = dock(g.tree, beside, id, where);
      beside = id;
      where = this.sideFor(g, id);
    }
    g.active = src.active;
    s.cur = gi;
    this.changed();
  }

  /** A terminal dragged up to the tab bar: it leaves the mosaic and becomes a tab of its own. */
  dropIntoBar(sid: Id) {
    const s = this.screen;
    const gi = s.indexOf(sid);
    if (gi < 0 || leaves(s.groups[gi].tree).length < 2) return;
    this.detach(sid);
    s.groups.splice(gi + 1, 0, { tree: leaf(sid), active: sid });
    s.cur = gi + 1;
    this.changed();
  }

  /** Drag of the line between two terminals. */
  resize(path: boolean[], pm: number) {
    const g = this.screen.group;
    if (!g) return;
    g.tree = setPm(g.tree, path, Math.max(100, Math.min(900, Math.round(pm))));
    this.changed();
  }

  // ------------------------------------------------------------ saving

  save(): Saved {
    return {
      current: this.current,
      screens: this.screens.map((s) => ({ groups: s.groups.map((g) => ({ tree: g.tree, active: g.active, broadcast: g.broadcast })), cur: s.cur })),
    };
  }

  /**
   * Puts the layout saved last time over the sessions that are still alive: what is gone leaves, and a session
   * the layout did not know becomes a tab of the screen its `screen` says.
   */
  restore(saved: Saved | null, live: (Omit<Tab, "dead"> & { screen: number })[]) {
    this.tabs.clear();
    this.screens = Array.from({ length: SCREENS }, emptyScreen);
    const alive = new Set(live.map((t) => t.sid));
    live.forEach((t) => this.tabs.set(t.sid, { ...t, screen: 0, dead: false }));
    const placed = new Set<Id>();
    // every saved screen's tabs go, in order, into the one row
    const take = (tree: Tree | null | undefined, active: Id | null | undefined, broadcast?: boolean) => {
      const keep = new Set([...alive].filter((id) => !placed.has(id)));
      const pruned = pruneTree(tree, keep);
      if (!pruned) return;
      const ids = leaves(pruned);
      ids.forEach((id) => placed.add(id));
      this.screens[0].groups.push({ tree: pruned, active: active != null && ids.includes(active) ? active : ids.at(-1)!, broadcast: !!broadcast && ids.length > 1 });
    };
    let cur = 0;
    saved?.screens?.forEach((ss, i) => {
      const start = this.screens[0].groups.length;
      if (ss.groups) {
        ss.groups.forEach((g) => take(g.tree, g.active, g.broadcast));
        if (i === (saved.current ?? 0)) cur = start + Math.max(0, ss.cur ?? 0);
      } else {
        if (ss.mode === "mosaic" && ss.tree) take(ss.tree, ss.active);
        (ss.order ?? []).forEach((id) => take(leaf(id), id));
        if (i === (saved.current ?? 0)) cur = start + Math.max(0, this.screens[0].groups.slice(start).findIndex((g) => g.active === ss.active));
      }
    });
    const known = [...live].sort((a, b) => a.screen - b.screen);
    for (const t of known) if (!placed.has(t.sid)) take(leaf(t.sid), t.sid);
    const s0 = this.screens[0];
    s0.cur = Math.min(cur, Math.max(0, s0.groups.length - 1));
    this.current = 0;
    this.changed();
  }
}

function swapIn(t: Tree, from: Id, to: Id): Tree {
  return t.kind === "leaf" ? (t.id === from ? leaf(to) : t) : { ...t, a: swapIn(t.a, from, to), b: swapIn(t.b, from, to) };
}

/** The saved tree without the terminals that are no longer there. */
function pruneTree(t: Tree | null | undefined, keep: Set<Id>): Tree | null {
  if (!t) return null;
  let out: Tree | null = t;
  for (const id of leaves(t)) if (!keep.has(id)) out = out ? remove(out, id) : null;
  return out;
}
