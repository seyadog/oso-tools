// Helpers for the saved arrangement of a tab (what a Space keeps): when a command may be typed in a new terminal, whether
// a shape holds exactly its terminals, and closing a shape up when some of its terminals cannot be opened.

import { Tree, leaves } from "./tree";

export interface LayoutPane {
  /** The server of this terminal (0 is this computer). */
  server: number;
  name: string;
  command: string;
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

/** A saved tab without some of its terminals (their servers were deleted): the arrangement closes up and renumbers. */
export function withoutPanes<P>(g: { shape: Tree; panes: P[] }, drop: Set<number>): { shape: Tree; panes: P[] } | null {
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
