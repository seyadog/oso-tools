// Saved groups: a tab kept with everything in it (which server each terminal is, how they are arranged, a command
// to type in each one once it is ready, and whether typing goes to all of them). They live in groups.json.

import { Tree, leaves } from "./tree";

export interface GroupPane {
  /** The server of this terminal (0 is this computer). */
  server: number;
  /** Its name when it was saved, to say what is missing if the server was deleted. */
  name: string;
  /** Typed (and Enter) once the terminal shows its prompt. Empty: nothing. */
  command: string;
}

export interface SavedGroup {
  id: number;
  name: string;
  broadcast: boolean;
  /** The arrangement, with the terminals numbered 0, 1, 2… as in `panes`. */
  shape: Tree;
  panes: GroupPane[];
}

/** Reads groups.json defensively: a file edited by hand may be wrong, and one bad group must not hide the others. */
export function parseGroups(text: string | null): SavedGroup[] {
  if (!text) return [];
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch {
    return [];
  }
  if (!Array.isArray(raw)) return [];
  const out: SavedGroup[] = [];
  const used = new Set<number>();
  for (const g of raw as Record<string, unknown>[]) {
    if (!g || typeof g !== "object" || !Array.isArray(g.panes) || !g.panes.length || !validShape(g.shape, g.panes.length)) continue;
    const panes = (g.panes as Record<string, unknown>[]).map((p) => ({
      server: Number.isInteger(p?.server) ? (p.server as number) : 0,
      name: typeof p?.name === "string" ? p.name : "",
      command: typeof p?.command === "string" ? p.command : "",
    }));
    let id = Number.isInteger(g.id) ? (g.id as number) : 0;
    while (id <= 0 || used.has(id)) id++;
    used.add(id);
    out.push({ id, name: typeof g.name === "string" && g.name.trim() ? g.name.trim() : "Group", broadcast: g.broadcast === true && panes.length > 1, shape: g.shape as Tree, panes });
  }
  return out;
}

/** A shape is valid when it holds exactly the terminals 0..n-1, each once. */
export function validShape(t: unknown, n: number): boolean {
  const ok = (x: unknown): boolean => {
    const v = x as Tree | null;
    if (!v || typeof v !== "object") return false;
    if (v.kind === "leaf") return Number.isInteger(v.id);
    return v.kind === "split" && typeof v.side === "boolean" && typeof v.pm === "number" && ok(v.a) && ok(v.b);
  };
  if (!ok(t)) return false;
  const ids = leaves(t as Tree).sort((a, b) => a - b);
  return ids.length === n && ids.every((id, i) => id === i);
}

export function nextGroupId(groups: SavedGroup[]): number {
  return groups.reduce((m, g) => Math.max(m, g.id), 0) + 1;
}

/** "web-1 · db-1 · +2": what a group holds, for its row. */
export function summary(g: SavedGroup, max = 3): string {
  const names = g.panes.map((p) => p.name || "Local");
  const shown = names.slice(0, max).join(" · ");
  return names.length > max ? `${shown} · +${names.length - max}` : shown;
}

/**
 * Whether a startup command can be typed yet: the last line of the terminal looks like a shell prompt and not like a
 * question (a password, a passphrase, a host key), which would take the command as its answer.
 */
export function looksReady(lastLine: string): boolean {
  const t = lastLine.replace(/\s+$/, "");
  if (!t) return false;
  if (/(password|passphrase|\(yes\/no|\[y\/n\])/i.test(t)) return false;
  // a prompt ends with its sign, or (zsh themes) starts with an arrow and ends with the folder
  return /[$#%>❯➜»]$/.test(t) || /^[❯➜»]/.test(t);
}

/** A group without some of its terminals (their servers were deleted): the arrangement closes up and renumbers. */
export function withoutPanes(g: Pick<SavedGroup, "shape" | "panes">, drop: Set<number>): { shape: Tree; panes: GroupPane[] } | null {
  let shape: Tree | null = g.shape;
  for (const i of drop) shape = shape ? removeLeaf(shape, i) : null;
  if (!shape) return null;
  const kept = g.panes.map((_, i) => i).filter((i) => !drop.has(i));
  return { shape: renumber(shape, kept), panes: kept.map((i) => g.panes[i]) };
}

function removeLeaf(t: Tree, id: number): Tree | null {
  if (t.kind === "leaf") return t.id === id ? null : t;
  const a = removeLeaf(t.a, id);
  const b = removeLeaf(t.b, id);
  return a && b ? { ...t, a, b } : (a ?? b);
}

function renumber(t: Tree, kept: number[]): Tree {
  return t.kind === "leaf" ? { kind: "leaf", id: kept.indexOf(t.id) } : { ...t, a: renumber(t.a, kept), b: renumber(t.b, kept) };
}
