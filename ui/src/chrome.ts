// The frame around the terminals: a thin rail of icons, one panel on the left that shows one section, a panel on the
// right, and a focus mode that hides all of them. This file holds the parts that are decisions and not drawing, so they
// can be tested: which sections exist, what a click on the rail does, how wide the panel may be, what is remembered.

export type Section = "spaces" | "servers" | "secrets" | "tunnels" | "accounts" | "agents" | "";

/** The rail, top to bottom. `action` is the shortcut that opens it (see hotkeys.ts). */
export const SECTIONS: { key: Exclude<Section, "">; label: string; icon: string; action: string }[] = [
  { key: "spaces", label: "Spaces", icon: "layers", action: "panel.spaces" },
  { key: "servers", label: "Servers", icon: "server", action: "panel.servers" },
  { key: "secrets", label: "Secrets", icon: "key", action: "panel.secrets" },
  { key: "tunnels", label: "Tunnels", icon: "tunnel", action: "panel.tunnels" },
  { key: "accounts", label: "Accounts", icon: "user", action: "panel.accounts" },
  { key: "agents", label: "AI agents", icon: "agent", action: "panel.agents" },
];

export const RAIL_WIDTH = 48;
/** The quick look at the AI agents, folded (only an arrow) and open (names). */
export const MINI_FOLDED = 18;
export const MINI_OPEN = 128;
export const LEFT_MIN = 200;
export const LEFT_MAX = 480;
export const LEFT_DEFAULT = 264;
/** The terminals never get narrower than this because of the panels. */
const CENTER_MIN = 360;

/** What a click on a rail icon does: the open section closes, any other one opens (replacing the one that was). */
export function clickSection(current: Section, clicked: Exclude<Section, "">): Section {
  return current === clicked ? "" : clicked;
}

export function clampLeft(w: number, windowWidth = Infinity, rightWidth = 0): number {
  const room = Number.isFinite(windowWidth) ? windowWidth - RAIL_WIDTH - rightWidth - CENTER_MIN : LEFT_MAX;
  const max = Math.max(LEFT_MIN, Math.min(LEFT_MAX, room));
  const n = Number.isFinite(w) ? w : LEFT_DEFAULT;
  return Math.round(Math.min(max, Math.max(LEFT_MIN, n)));
}

export function store(k: string): string | null {
  try {
    return localStorage.getItem(k);
  } catch {
    return null;
  }
}

export function storeSet(k: string, v: string) {
  try {
    localStorage.setItem(k, v);
  } catch {
    /* a private window: it just will not be remembered */
  }
}

const KEYS = SECTIONS.map((s) => s.key as string);

/** The section the left panel was left on. The first time ever: Servers (so a new person sees something). */
export function readSection(): Section {
  const v = store("oso.ui.left");
  if (v == null) return "servers";
  return KEYS.includes(v) ? (v as Section) : "";
}
