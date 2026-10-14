import { ACTIONS, describe } from "./actions";

// Hotkeys: every action has a default and you can change any of them in hotkeys.json.
// A combo is written like "Alt+Shift+2" or "Alt+Left". Keys are matched by their physical place (the digit
// row, the letter keys), so the same combo works on every keyboard layout.

/** The main key: every default shortcut is "Mod+something", and Mod is whichever of these is chosen in Settings. */
export const MODIFIERS = ["Alt", "Ctrl", "Super", "Ctrl+Alt"];
export const DEFAULT_MODIFIER = "Alt";

/** "Mod+Shift+Left" with Mod = "Super" → "Super+Shift+Left". */
export function resolveMod(text: string, mod: string): string {
  return text.replace(/^Mod\+/, `${mod}+`);
}

export function modifierOf(user: Record<string, unknown> | null): string {
  const m = user?.modifier;
  return typeof m === "string" && MODIFIERS.includes(m) ? m : DEFAULT_MODIFIER;
}

export const DEFAULTS: Record<string, string | string[]> = {
  "palette.open": "Mod+Shift+Space",
  "tab.1": "Mod+1",
  "tab.2": "Mod+2",
  "tab.3": "Mod+3",
  "tab.4": "Mod+4",
  "tab.5": "Mod+5",
  "tab.6": "Mod+6",
  "tab.7": "Mod+7",
  "tab.8": "Mod+8",
  "tab.9": "Mod+9",
  "tab.prev": "Mod+,",
  "tab.next": "Mod+.",
  "mode.toggle": "Mod+M",
  "split.right": "Mod+V",
  "split.down": "Mod+H",
  "focus.left": "Mod+Left",
  "focus.right": "Mod+Right",
  "focus.up": "Mod+Up",
  "focus.down": "Mod+Down",
  "move.left": "Mod+Shift+Left",
  "move.right": "Mod+Shift+Right",
  "move.up": "Mod+Shift+Up",
  "move.down": "Mod+Shift+Down",
  "tab.new": "Ctrl+N",
  "tab.close": "Mod+W",
  "tab.rename": "F2",
  "broadcast.toggle": "Mod+B",
  "watch.toggle": "Mod+A",
  "files.open": "Mod+U",
  "tunnels.open": "Mod+T",
  "search": "Mod+K",
  "sudo.fill": "Mod+P",
  "sidebar.toggle": "Mod+Q",
  "settings": "Ctrl+,",
  "help.open": "Mod+/",
  "secrets.open": "Mod+S",
  "local.new": "Mod+Shift+L",
  "rpanel.toggle": "Mod+J",
  "focus.toggle": "Mod+Z",
  "attention.next": "Mod+N",
  "agent.new": "Mod+Shift+A",
  "panel.servers": "Mod+Shift+2",
  "panel.secrets": "Mod+Shift+3",
  "panel.tunnels": "Mod+Shift+4",
  "panel.agents": "Mod+Shift+6",
  "panel.accounts": "Mod+Shift+5",
  "panel.kubernetes": "Mod+Shift+7",
};

/** What each action does, for the settings screen and the sheet of shortcuts: it comes from the one list of actions. */
export const LABELS: Record<string, string> = Object.fromEntries(ACTIONS.map((a) => [a.id, describe(a.id)]));

interface Combo {
  alt: boolean;
  ctrl: boolean;
  shift: boolean;
  meta: boolean;
  /** KeyboardEvent.code */
  code: string;
}

const NAMED: Record<string, string> = {
  left: "ArrowLeft",
  right: "ArrowRight",
  up: "ArrowUp",
  down: "ArrowDown",
  enter: "Enter",
  return: "Enter",
  tab: "Tab",
  esc: "Escape",
  escape: "Escape",
  space: "Space",
  ",": "Comma",
  ".": "Period",
  "/": "Slash",
  "\\": "Backslash",
  "-": "Minus",
  "=": "Equal",
  ";": "Semicolon",
  "'": "Quote",
  "[": "BracketLeft",
  "]": "BracketRight",
  "`": "Backquote",
  backspace: "Backspace",
  delete: "Delete",
  home: "Home",
  end: "End",
  pageup: "PageUp",
  pagedown: "PageDown",
};

/** "Alt+Shift+2" → what a KeyboardEvent must look like. `null` when it is not a valid combo. */
export function parseCombo(text: string): Combo | null {
  const parts = text.split("+").map((p) => p.trim());
  // "Alt++" and "Ctrl+," : the key can be a plus or a comma, so split on the last separator only.
  if (text.endsWith("++")) parts.splice(parts.length - 2, 2, "+");
  const key = parts.pop();
  if (!key) return null;
  const c: Combo = { alt: false, ctrl: false, shift: false, meta: false, code: "" };
  for (const m of parts) {
    switch (m.toLowerCase()) {
      case "alt":
      case "option":
        c.alt = true;
        break;
      case "ctrl":
      case "control":
        c.ctrl = true;
        break;
      case "shift":
        c.shift = true;
        break;
      case "super":
      case "meta":
      case "cmd":
      case "win":
        c.meta = true;
        break;
      default:
        return null;
    }
  }
  const k = key.toLowerCase();
  if (/^[0-9]$/.test(k)) c.code = `Digit${k}`;
  else if (/^[a-z]$/.test(k)) c.code = `Key${k.toUpperCase()}`;
  else if (/^f([1-9]|1[0-2])$/.test(k)) c.code = k.toUpperCase();
  else if (k === "+") c.code = "Equal"; // shift+= on most layouts; rarely used
  else if (NAMED[k]) c.code = NAMED[k];
  else return null;
  return c;
}

export function formatCombo(text: string): string {
  return text;
}

export type Bindings = Map<string, Combo[]>;

/** Defaults with the user's changes on top. A `null` or empty value removes the shortcut. Bad entries are reported. */
export function buildBindings(user: Record<string, unknown> | null): { bindings: Bindings; problems: string[] } {
  const problems: string[] = [];
  const mod = modifierOf(user);
  if (user && "modifier" in user && user.modifier !== mod) problems.push(`"modifier" must be one of ${MODIFIERS.join(", ")}: using ${DEFAULT_MODIFIER}`);
  const merged: Record<string, string | string[] | null> = {};
  for (const [action, v] of Object.entries(DEFAULTS)) merged[action] = Array.isArray(v) ? v.map((x) => resolveMod(x, mod)) : resolveMod(v, mod);
  if (user && typeof user === "object") {
    for (const [action, v] of Object.entries(user)) {
      if (action === "modifier") continue;
      if (!(action in DEFAULTS)) {
        problems.push(`Unknown action "${action}"`);
        continue;
      }
      if (v === null || v === "" || (Array.isArray(v) && v.length === 0)) merged[action] = null;
      else if (typeof v === "string" || (Array.isArray(v) && v.every((x) => typeof x === "string"))) {
        merged[action] = Array.isArray(v) ? (v as string[]).map((x) => resolveMod(x, mod)) : resolveMod(v as string, mod);
      }
      else problems.push(`"${action}" must be a text like "Alt+M", a list of them, or null`);
    }
  }
  const bindings: Bindings = new Map();
  const taken = new Map<string, string>();
  for (const [action, v] of Object.entries(merged)) {
    if (v == null) continue;
    const combos: Combo[] = [];
    for (const text of Array.isArray(v) ? v : [v]) {
      const c = parseCombo(text);
      if (!c) {
        problems.push(`"${text}" (for ${action}) is not a valid shortcut`);
        continue;
      }
      const id = `${+c.alt}${+c.ctrl}${+c.shift}${+c.meta}${c.code}`;
      const other = taken.get(id);
      if (other && other !== action) problems.push(`"${text}" is used by both ${other} and ${action}: ${other} keeps it`);
      else {
        taken.set(id, action);
        combos.push(c);
      }
    }
    if (combos.length) bindings.set(action, combos);
  }
  return { bindings, problems };
}

/** The action a key press triggers, if any. */
export function matchAction(b: Bindings, e: Pick<KeyboardEvent, "altKey" | "ctrlKey" | "shiftKey" | "metaKey" | "code">): string | null {
  for (const [action, combos] of b) {
    for (const c of combos) {
      if (c.alt === e.altKey && c.ctrl === e.ctrlKey && c.shift === e.shiftKey && c.meta === e.metaKey && c.code === e.code) return action;
    }
  }
  return null;
}

/** The combo of an action for display ("Alt+M"), or "" when it has none. */
export function comboText(user: Record<string, unknown> | null, action: string): string {
  const mod = modifierOf(user);
  const v = user && action in user ? user[action] : DEFAULTS[action];
  if (v == null || v === "") return "";
  return (Array.isArray(v) ? v : [v]).map((x) => resolveMod(String(x), mod)).join(" / ");
}
