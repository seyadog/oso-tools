// The colours of the window: one per server and one per project (the folder an agent works in). Each keeps its colour
// for good: a server's is saved with it, a project's in the settings. Nothing is picked by chance, so two of them only
// share a colour once all twelve are taken.

export const PALETTE = ["#6ea8ff", "#ff6e91", "#82e682", "#ffa05a", "#be82ff", "#5ad7f5", "#ffd764", "#50e1b9", "#c4e85a", "#f27fe0", "#ff8f78", "#9aa5ff"];
export const LOCAL_COLOR = "#6c7086";

let servers = new Map<number, number>();
let projects = new Map<string, number>();
let save: (m: Record<string, number>) => void = () => {};

/** The colours the servers were given (their `color` index). */
export function setServerColors(list: { id: number; color?: number | null }[]) {
  servers = new Map(list.filter((s) => typeof s.color === "number").map((s) => [s.id, s.color as number]));
}

export function serverColor(id: number): string {
  if (id === 0) return LOCAL_COLOR;
  return PALETTE[(servers.get(id) ?? id) % PALETTE.length];
}

/** Loads the colours of the projects and says where to save them when a new project gets one. */
export function setProjectColors(saved: Record<string, number>, onNew: (m: Record<string, number>) => void) {
  projects = new Map(Object.entries(saved).filter(([, v]) => Number.isInteger(v)));
  save = onNew;
}

/** The colour of a project; the first time it is seen it gets the one that fewest projects wear. */
export function projectColor(key: string): string {
  let i = projects.get(key);
  if (i === undefined) {
    const used = new Array(PALETTE.length).fill(0);
    for (const v of projects.values()) used[v % PALETTE.length]++;
    i = used.indexOf(Math.min(...used));
    projects.set(key, i);
    save(Object.fromEntries(projects));
  }
  return PALETTE[i % PALETTE.length];
}
