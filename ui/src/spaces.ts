// Spaces, the parts that are not drawing: what a Space covers, how it is described in a row, what a saved tab becomes,
// and what is left of it when things it names were deleted. (The engine enforces the same rules for agents.)

import { Space, SpaceTab, TreeData } from "./api";
import { NO_SETTINGS } from "./space_bar";
import { Tree } from "./tree";

export function emptySpace(): Space {
  return { id: 0, name: "", folders: [], servers: [], server_folders: [], secrets: [], tunnels: [], notes: "", tabs: [], ...NO_SETTINGS };
}

/** Every server a Space covers: its single servers, and everything under its folders at any depth. Deleted ones are not there. */
export function coveredServers(space: Pick<Space, "servers" | "server_folders">, tree: Pick<TreeData, "servers" | "folders">): number[] {
  const out = new Set(space.servers.filter((id) => tree.servers.some((s) => s.id === id)));
  for (const root of space.server_folders) {
    if (!tree.folders.some((f) => f.id === root)) continue;
    const inside = new Set([root]);
    for (let grew = true; grew; ) {
      grew = false;
      for (const f of tree.folders) if (f.parent != null && inside.has(f.parent) && !inside.has(f.id)) (inside.add(f.id), (grew = true));
    }
    for (const s of tree.servers) if (s.parent != null && inside.has(s.parent)) out.add(s.id);
  }
  return [...out].sort((a, b) => a - b);
}

const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

/** "3 servers · 2 folders · 1 tab": what a row says about a Space. */
export function spaceSummary(space: Space, tree: Pick<TreeData, "servers" | "folders">): string {
  const parts = [plural(coveredServers(space, tree).length, "server"), plural(space.folders.length, "folder"), plural(space.tabs.length, "tab")];
  if (space.secrets.length) parts.push(plural(space.secrets.length, "secret"));
  return parts.join(" · ");
}

/** What a Space names that no longer exists, so that the person can be told (and it is dropped when the Space is saved). */
export function missingOf(space: Space, tree: Pick<TreeData, "servers" | "folders">, secretIds: number[] | null): { servers: number; folders: number; secrets: number } {
  return {
    servers: space.servers.filter((id) => !tree.servers.some((s) => s.id === id)).length,
    folders: space.server_folders.filter((id) => !tree.folders.some((f) => f.id === id)).length,
    // with the vault locked the list of secrets is not known: nothing is said to be missing
    secrets: secretIds ? space.secrets.filter((id) => !secretIds.includes(id)).length : 0,
  };
}

/** A tab on show, as a saved tab of a Space. */
export function tabFor(name: string, shape: Tree, broadcast: boolean, panes: { server: number; name: string; cwd: string }[]): SpaceTab {
  return { name, broadcast: broadcast && panes.length > 1, shape, panes: panes.map((p) => ({ server: p.server, name: p.name, command: "", cwd: p.cwd })) };
}

/** The Space with a tab added, and the servers of its terminals covered too (this computer needs no server). */
export function withTab(space: Space, tab: SpaceTab): Space {
  const servers = [...new Set([...space.servers, ...tab.panes.map((p) => p.server).filter((s) => s !== 0)])];
  return { ...space, servers, tabs: [...space.tabs, tab] };
}

/** The Space a terminal of this window belongs to, by what its tab says. */
export function openSpaces(tabs: Iterable<{ space?: number; dead: boolean }>): Map<number, number> {
  const out = new Map<number, number>();
  for (const t of tabs) if (t.space != null && !t.dead) out.set(t.space, (out.get(t.space) ?? 0) + 1);
  return out;
}
