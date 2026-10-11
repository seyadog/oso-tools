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
  "search": "Mod+K",
  "sudo.fill": "Mod+P",
  "sidebar.toggle": "Mod+Q",
  "settings": "Ctrl+,",
};

/** What each action does, for the settings screen. */
export const LABELS: Record<string, string> = {
  "tab.prev": "Previous tab",
  "tab.next": "Next tab",
  "mode.toggle": "Join every tab in one mosaic, and set them apart again",
  "split.right": "New terminal to the right",
  "split.down": "New terminal below",
  "focus.left": "Focus the terminal on the left",
  "focus.right": "Focus the terminal on the right",
  "focus.up": "Focus the terminal above",
  "focus.down": "Focus the terminal below",
  "move.left": "Move this terminal left",
  "move.right": "Move this terminal right",
  "move.up": "Move this terminal up",
  "move.down": "Move this terminal down",
  "tab.new": "New terminal (same server)",
  "tab.close": "Close this terminal",
  "tab.rename": "Rename this terminal",
  "broadcast.toggle": "Type in every terminal of the screen",
  "search": "Open a host (quick search)",
  "sudo.fill": "Type the saved sudo password",
  "sidebar.toggle": "Servers panel ⇄ terminal",
  "settings": "Settings",
  ...Object.fromEntries(Array.from({ length: 9 }, (_, i) => [`tab.${i + 1}`, `Go to tab ${i + 1}  (9 goes to the last one)`])),
};

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
