// The command palette, the parts that are not drawing: the fuzzy search ("nwag" finds "New agent"), what goes in which
// group, what is shown first, and the recents. The window (`palette_ui.ts`) only shows what this decides.

export type Kind = "action" | "server" | "space" | "secret" | "tab";

export interface Item {
  /** Unique inside its kind (an action's id, a server's id…). */
  id: string;
  kind: Kind;
  label: string;
  /** A second line: where, what. */
  sub?: string;
  icon: string;
  /** The keys written for it, if it has any. */
  hotkey?: string;
  /** Other words that find it (they do not show). */
  keywords?: string;
  run: () => void;
}

export const TITLES: Record<Kind | "recent", string> = { recent: "Recent", action: "Actions", tab: "Open tabs", space: "Spaces", server: "Servers", secret: "Secrets" };
export const GROUP_ICONS: Record<Kind | "recent", string> = { recent: "play", action: "snippet", tab: "tabs", space: "layers", server: "server", secret: "key" };
/** The order of the groups when you search. */
const ORDER: Kind[] = ["action", "tab", "space", "server", "secret"];

export interface Match {
  score: number;
  /** The positions of the text that matched, to draw them in bold. */
  at: number[];
}

const isStart = (text: string, i: number) => i === 0 || /[\s\-_./·:@(]/.test(text[i - 1]);

/**
 * Does `query` match `text`, letters in order but not necessarily together? The better the match, the higher the score:
 * the whole query as one piece is best, then matches that begin words and run on, then spread-out ones. `null` when the
 * letters are not all there in order.
 */
export function fuzzy(query: string, text: string): Match | null {
  const q = query.toLowerCase().replace(/\s+/g, "");
  const t = text.toLowerCase();
  if (!q) return { score: 0, at: [] };
  const whole = t.indexOf(q);
  if (whole >= 0) {
    const bonus = (isStart(t, whole) ? 25 : 12) + (whole === 0 ? 10 : 0);
    return { score: 40 + q.length * 6 + bonus - t.length * 0.1, at: Array.from({ length: q.length }, (_, i) => whole + i) };
  }
  // the best way of laying the letters out: prefer starting words and running on
  let best: Match | null = null;
  const lay = (qi: number, from: number, at: number[], score: number) => {
    if (qi === q.length) {
      const s = score - t.length * 0.1;
      if (!best || s > best.score) best = { score: s, at: [...at] };
      return;
    }
    for (let i = from; i < t.length; i++) {
      if (t[i] !== q[qi]) continue;
      const run = at.length && at[at.length - 1] === i - 1;
      const gain = 3 + (isStart(t, i) ? 8 : 0) + (run ? 6 : 0) - (at.length && !run ? Math.min(4, i - at[at.length - 1] - 1) * 0.5 : 0);
      at.push(i);
      lay(qi + 1, i + 1, at, score + gain);
      at.pop();
      // do not try every later place for the same letter once a word start was used (keeps it quick)
      if (isStart(t, i)) break;
    }
  };
  lay(0, 0, [], 0);
  return best;
}

export interface Row {
  item: Item;
  at: number[];
  score: number;
}

export interface GroupOut {
  kind: Kind | "recent";
  title: string;
  icon: string;
  rows: Row[];
}

export interface RankOptions {
  /** Ids of actions used lately, newest first. */
  recent?: string[];
  /** Only these kinds (the quick search of servers). */
  only?: Kind[];
  /** How many a group shows. */
  perGroup?: number;
}

/** How the palette is filled: with no query, the recents on top and a few of each group; with one, what matches, best first. */
export function rank(items: Item[], query: string, opts: RankOptions = {}): GroupOut[] {
  const only = opts.only ?? ORDER;
  const per = opts.perGroup ?? 8;
  const recent = opts.recent ?? [];
  const pool = items.filter((i) => only.includes(i.kind));
  const q = query.trim();
  const out: GroupOut[] = [];
  const group = (kind: Kind | "recent", rows: Row[]) => rows.length && out.push({ kind, title: TITLES[kind], icon: GROUP_ICONS[kind], rows });
  if (!q) {
    const recents = recent.map((id) => pool.find((i) => i.kind === "action" && i.id === id)).filter((i): i is Item => !!i).slice(0, 5);
    group("recent", recents.map((item) => ({ item, at: [], score: 0 })));
    for (const kind of only) {
      const rows = pool.filter((i) => i.kind === kind && !(kind === "action" && recents.includes(i))).map((item) => ({ item, at: [], score: 0 }));
      group(kind, only.length === 1 ? rows : rows.slice(0, kind === "action" ? 8 : 5));
    }
    return out;
  }
  for (const kind of only) {
    const rows: Row[] = [];
    for (const item of pool.filter((i) => i.kind === kind)) {
      const own = fuzzy(q, item.label);
      const other = own ? null : fuzzy(q, `${item.sub ?? ""} ${item.keywords ?? ""}`);
      if (!own && !other) continue;
      const lift = kind === "action" && recent.includes(item.id) ? 6 : 0;
      rows.push(own ? { item, at: own.at, score: own.score + lift } : { item, at: [], score: other!.score * 0.5 + lift });
    }
    rows.sort((a, b) => b.score - a.score || a.item.label.localeCompare(b.item.label));
    group(kind, only.length === 1 ? rows : rows.slice(0, per));
  }
  return out;
}

/** The rows of every group in the order they are shown (what the arrow keys walk). */
export function flat(groups: GroupOut[]): Row[] {
  return groups.flatMap((g) => g.rows);
}

const KEY = "oso.palette.recent";
const MAX_RECENT = 8;

/** `id` first, the rest as they were, no repeats, at most 8. */
export function pushRecent(list: string[], id: string): string[] {
  return [id, ...list.filter((x) => x !== id)].slice(0, MAX_RECENT);
}

export function loadRecent(): string[] {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? "[]");
    return Array.isArray(v) ? v.filter((x): x is string => typeof x === "string").slice(0, MAX_RECENT) : [];
  } catch {
    return [];
  }
}

export function saveRecent(list: string[]) {
  try {
    localStorage.setItem(KEY, JSON.stringify(list));
  } catch {
    /* it just will not be remembered */
  }
}
