// The layout of a screen in mosaic mode: a tree of divisions. Every division is side by side or stacked and
// holds two parts, each a terminal or another division, so any arrangement can be built
// (and you can drag things around).

export type Id = number;
export type Side = "left" | "right" | "top" | "bottom";
export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export type Tree =
  | { kind: "leaf"; id: Id }
  | { kind: "split"; side: boolean; pm: number; a: Tree; b: Tree };
// `side`: the parts are side by side (else one over the other). `pm`: size of the first one, in thousandths.

export interface SplitInfo {
  /** Which way to go from the root to reach it (false: first part, true: second). */
  path: boolean[];
  /** The area both parts share. */
  whole: Rect;
  side: boolean;
  /** Where the second part starts: an x (side by side) or a y (stacked). */
  at: number;
}

export const leaf = (id: Id): Tree => ({ kind: "leaf", id });

export function leaves(t: Tree): Id[] {
  return t.kind === "leaf" ? [t.id] : [...leaves(t.a), ...leaves(t.b)];
}

export function contains(t: Tree, id: Id): boolean {
  return t.kind === "leaf" ? t.id === id : contains(t.a, id) || contains(t.b, id);
}

/** Takes a terminal out; its sibling fills the place of the division. `null` when nothing is left. */
export function remove(t: Tree, id: Id): Tree | null {
  if (t.kind === "leaf") return t.id === id ? null : t;
  const a = remove(t.a, id);
  const b = remove(t.b, id);
  if (a && b) return a === t.a && b === t.b ? t : { ...t, a, b };
  return a ?? b;
}

/** The same arrangement with every terminal renamed by `f` (a saved layout uses 0, 1, 2… for its terminals). */
export function mapLeaves(t: Tree, f: (id: Id) => Id): Tree {
  return t.kind === "leaf" ? leaf(f(t.id)) : { ...t, a: mapLeaves(t.a, f), b: mapLeaves(t.b, f) };
}

/** Puts `to` where `from` is. */
export function replace(t: Tree, from: Id, to: Id): Tree {
  if (t.kind === "leaf") return t.id === from ? leaf(to) : t;
  return { ...t, a: replace(t.a, from, to), b: replace(t.b, from, to) };
}

/** Two terminals trade places. */
export function swap(t: Tree, p: Id, q: Id): Tree {
  if (t.kind === "leaf") return t.id === p ? leaf(q) : t.id === q ? leaf(p) : t;
  return { ...t, a: swap(t.a, p, q), b: swap(t.b, p, q) };
}

/** `id` (not in the tree yet) takes half of the terminal `target`, on that side of it. */
export function dock(t: Tree, target: Id, id: Id, side: Side): Tree {
  if (t.kind === "leaf") {
    if (t.id !== target) return t;
    const sideBySide = side === "left" || side === "right";
    const first = side === "left" || side === "top";
    const [a, b] = first ? [leaf(id), t] : [t, leaf(id)];
    return { kind: "split", side: sideBySide, pm: 500, a, b };
  }
  const a = dock(t.a, target, id, side);
  if (a !== t.a) return { ...t, a };
  const b = dock(t.b, target, id, side);
  return b !== t.b ? { ...t, b } : t;
}

const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v));

/** The boxes of the terminals and the divisions inside `area`. */
export function layout(t: Tree, area: Rect): { boxes: { id: Id; rect: Rect }[]; splits: SplitInfo[] } {
  const boxes: { id: Id; rect: Rect }[] = [];
  const splits: SplitInfo[] = [];
  const go = (n: Tree, r: Rect, path: boolean[]) => {
    if (n.kind === "leaf") return void boxes.push({ id: n.id, rect: r });
    if (n.side) {
      const w = clamp((r.w * n.pm) / 1000, 0, r.w);
      splits.push({ path, whole: r, side: true, at: r.x + w });
      go(n.a, { x: r.x, y: r.y, w, h: r.h }, [...path, false]);
      go(n.b, { x: r.x + w, y: r.y, w: r.w - w, h: r.h }, [...path, true]);
    } else {
      const h = clamp((r.h * n.pm) / 1000, 0, r.h);
      splits.push({ path, whole: r, side: false, at: r.y + h });
      go(n.a, { x: r.x, y: r.y, w: r.w, h }, [...path, false]);
      go(n.b, { x: r.x, y: r.y + h, w: r.w, h: r.h - h }, [...path, true]);
    }
  };
  go(t, area, []);
  return { boxes, splits };
}

/** Sets the size of the division at `path`. */
export function setPm(t: Tree, path: boolean[], pm: number): Tree {
  if (t.kind === "leaf") return t;
  if (path.length === 0) return { ...t, pm };
  const [head, ...rest] = path;
  return head ? { ...t, b: setPm(t.b, rest, pm) } : { ...t, a: setPm(t.a, rest, pm) };
}

export const MIN_W = 320;
export const MIN_H = 180;

/**
 * Where a new terminal goes when it splits the box `r` by itself: beside it when the box is wide, below it when
 * it is tall, and never leaving a box too small to use if the other way fits.
 */
export function autoSide(r: Rect): Side {
  const wide = r.w >= r.h;
  const fitsBeside = r.w / 2 >= MIN_W;
  const fitsBelow = r.h / 2 >= MIN_H;
  if ((wide && (fitsBeside || !fitsBelow)) || (!wide && !fitsBelow && fitsBeside)) return "right";
  return "bottom";
}

/** Automatic layout: each terminal splits the one before it, beside or below by the shape of its box. */
export function dwindle(ids: Id[], area: Rect): Tree | null {
  if (ids.length === 0) return null;
  let t = leaf(ids[0]);
  for (let i = 1; i < ids.length; i++) {
    const prev = layout(t, area).boxes.find((b) => b.id === ids[i - 1])?.rect ?? area;
    t = dock(t, ids[i - 1], ids[i], autoSide(prev));
  }
  return t;
}

/** The side of `r` a pointer at (x, y) is closest to, when it is near the edge; `null` in the middle. */
export function dockSide(r: Rect, x: number, y: number): Side | null {
  const fx = (x - r.x) / Math.max(1, r.w);
  const fy = (y - r.y) / Math.max(1, r.h);
  const edges: [number, Side][] = [
    [fx, "left"],
    [1 - fx, "right"],
    [fy, "top"],
    [1 - fy, "bottom"],
  ];
  const [d, side] = edges.reduce((best, e) => (e[0] < best[0] ? e : best));
  return d < 0.28 ? side : null;
}

/** The half of `r` a terminal would take when docked on `side`; the whole box when `null`. */
export function dockZone(r: Rect, side: Side | null): Rect {
  const hw = r.w / 2;
  const hh = r.h / 2;
  switch (side) {
    case "left":
      return { x: r.x, y: r.y, w: hw, h: r.h };
    case "right":
      return { x: r.x + hw, y: r.y, w: hw, h: r.h };
    case "top":
      return { x: r.x, y: r.y, w: r.w, h: hh };
    case "bottom":
      return { x: r.x, y: r.y + hh, w: r.w, h: hh };
    default:
      return r;
  }
}

/** The box next to box `s` in direction (dx, dy), by where the boxes really are. */
export function neighbour(rects: Rect[], s: number, dx: number, dy: number): number | null {
  const cur = rects[s];
  if (!cur) return null;
  const overlap = (a0: number, a1: number, b0: number, b1: number) => Math.max(0, Math.min(a1, b1) - Math.max(a0, b0));
  let best: { i: number; gap: number; side: number } | null = null;
  rects.forEach((r, i) => {
    if (i === s) return;
    let gap: number;
    let side: number;
    if (dx < 0) {
      gap = cur.x - (r.x + r.w);
      side = overlap(cur.y, cur.y + cur.h, r.y, r.y + r.h);
    } else if (dx > 0) {
      gap = r.x - (cur.x + cur.w);
      side = overlap(cur.y, cur.y + cur.h, r.y, r.y + r.h);
    } else if (dy < 0) {
      gap = cur.y - (r.y + r.h);
      side = overlap(cur.x, cur.x + cur.w, r.x, r.x + r.w);
    } else {
      gap = r.y - (cur.y + cur.h);
      side = overlap(cur.x, cur.x + cur.w, r.x, r.x + r.w);
    }
    if (gap < -0.5 || side <= 0) return;
    if (!best || gap < best.gap - 0.5 || (Math.abs(gap - best.gap) <= 0.5 && side > best.side)) best = { i, gap, side };
  });
  return best ? (best as { i: number }).i : null;
}
