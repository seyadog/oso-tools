// The one list of what oso can do. The command palette, the menus (the "..." menu, the one of a terminal, the one of a
// tab) and the sheet of shortcuts are all made from it, so an action that is added here is in all of them, and an action
// that is not here is nowhere. Its keys are in `hotkeys.ts` (`DEFAULTS`), by the same id; a test fails if a key has no entry.

/** What the window knows when it decides which actions apply now. */
export interface ActionCtx {
  /** There is a terminal on show (not a dead one). */
  terminal: boolean;
  /** It is an ssh session (not this computer). */
  ssh: boolean;
  /** The tab on show has two or more terminals. */
  several: boolean;
  /** There are tabs at all. */
  tabs: boolean;
  /** More than one tab. */
  manyTabs: boolean;
  /** Typing goes to every terminal of the tab. */
  broadcasting: boolean;
  /** The terminal on show is being watched (the bell). */
  watching: boolean;
  /** Some agent needs you or finished. */
  attention: boolean;
  /** The vault exists / is open. */
  vaultExists: boolean;
  vaultUnlocked: boolean;
  focusMode: boolean;
  rightOpen: boolean;
}

export const NO_CONTEXT: ActionCtx = { terminal: false, ssh: false, several: false, tabs: false, manyTabs: false, broadcasting: false, watching: false, attention: false, vaultExists: false, vaultUnlocked: false, focusMode: false, rightOpen: false };

export interface ActionDef {
  /** The id: the same as in the table of keys when it has one. */
  id: string;
  /** What the palette and the menus say. */
  label: string;
  /** What the sheet of shortcuts and Settings say, when it needs more words than `label`. */
  describe?: string;
  icon: string;
  /** Only when this is true is it offered (the palette and the menus leave it out otherwise). */
  when?: (c: ActionCtx) => boolean;
  /** Registered, but not listed in the palette (the members of a family such as "go to tab 4"). */
  quiet?: boolean;
  /** Other words that find it. */
  keywords?: string;
}

const t = (c: ActionCtx) => c.terminal;

export const ACTIONS: ActionDef[] = [
  // open things
  { id: "palette.open", label: "Command palette", describe: "Every command, server, Space, secret and tab: type to find it", icon: "search", keywords: "commands search find" },
  { id: "search", label: "Open a server…", describe: "Open a server (quick search)", icon: "server", keywords: "connect ssh host" },
  { id: "local.new", label: "New local terminal", describe: "New local terminal (in the active Space)", icon: "terminal", keywords: "shell" },
  { id: "tab.new", label: "New terminal on the same server", describe: "New terminal (same server)", icon: "plus", when: t },
  { id: "agent.new", label: "New agent…", describe: "New agent (folder, agent, optional worktree)", icon: "agent", keywords: "claude opencode codex ai worktree" },
  { id: "space.new", label: "New Space…", icon: "layers", keywords: "workspace project" },
  { id: "server.add", label: "Add a server…", icon: "plus", keywords: "new host" },
  { id: "ssh.import", label: "Import servers from ~/.ssh", icon: "download", keywords: "config hosts keys" },
  { id: "secret.new", label: "New secret…", icon: "key", when: (c) => c.vaultUnlocked, keywords: "api key token password" },
  // tabs and terminals
  { id: "tab.rename", label: "Rename this tab", describe: "Rename this terminal", icon: "edit", when: (c) => c.tabs },
  { id: "tab.close", label: "Close this terminal", icon: "close", when: t },
  { id: "split.right", label: "New terminal to the right", icon: "split-right", when: t },
  { id: "split.down", label: "New terminal below", icon: "split-down", when: t },
  { id: "broadcast.toggle", label: "Type in every terminal of this tab", describe: "Type in every terminal of the tab (broadcast), on or off", icon: "broadcast", when: (c) => c.several || c.broadcasting, keywords: "broadcast all" },
  { id: "watch.toggle", label: "Tell me when this terminal is done", describe: "Tell me when this terminal is done, on or off", icon: "bell", when: t, keywords: "watch notify bell" },
  { id: "files.open", label: "Copy files to or from this server…", describe: "Copy files to or from this terminal's server", icon: "files", when: (c) => c.terminal && c.ssh, keywords: "scp upload download transfer" },
  { id: "group.save", label: "Save this tab in a Space…", describe: "Save this tab in a Space", icon: "save", when: (c) => c.tabs },
  { id: "sudo.fill", label: "Type the saved sudo password", icon: "lock", when: t },
  { id: "mode.toggle", label: "Join every tab in one mosaic (or set them apart)", describe: "Join every tab in one mosaic, and set them apart again", icon: "grid", when: (c) => c.manyTabs },
  { id: "tab.prev", label: "Previous tab", icon: "chevron-left", when: (c) => c.manyTabs },
  { id: "tab.next", label: "Next tab", icon: "chevron-right", when: (c) => c.manyTabs },
  { id: "focus.left", label: "Focus the terminal on the left", icon: "chevron-left", when: (c) => c.several, quiet: true },
  { id: "focus.right", label: "Focus the terminal on the right", icon: "chevron-right", when: (c) => c.several, quiet: true },
  { id: "focus.up", label: "Focus the terminal above", icon: "chevron-down", when: (c) => c.several, quiet: true },
  { id: "focus.down", label: "Focus the terminal below", icon: "chevron-down", when: (c) => c.several, quiet: true },
  { id: "move.left", label: "Move this terminal left", icon: "chevron-left", when: (c) => c.several, quiet: true },
  { id: "move.right", label: "Move this terminal right", icon: "chevron-right", when: (c) => c.several, quiet: true },
  { id: "move.up", label: "Move this terminal up", icon: "chevron-down", when: (c) => c.several, quiet: true },
  { id: "move.down", label: "Move this terminal down", icon: "chevron-down", when: (c) => c.several, quiet: true },
  ...Array.from({ length: 9 }, (_, i): ActionDef => ({ id: `tab.${i + 1}`, label: `Go to tab ${i + 1}`, describe: `Go to tab ${i + 1}  (9 goes to the last one)`, icon: "tabs", when: (c) => c.tabs, quiet: true })),
  // agents
  { id: "attention.next", label: "Next agent that needs you", describe: "Jump to the next agent that needs you", icon: "bell", when: (c) => c.attention, keywords: "agent queue waiting" },
  // look of the window
  { id: "focus.toggle", label: "Focus mode: only the terminals", describe: "Focus mode: hide every panel, only the terminals", icon: "focus", keywords: "zen hide panels" },
  { id: "rpanel.toggle", label: "Left panel: AI agents and activity", describe: "Left panel: AI agents and their activity, open or closed", icon: "agent", keywords: "agents activity" },
  { id: "sidebar.toggle", label: "Servers list ⇄ terminal", describe: "Servers list ⇄ terminal (opens the Servers panel if it is closed)", icon: "server" },
  { id: "panel.spaces", label: "Left panel: Spaces", describe: "Left panel: Spaces (again to close it)", icon: "layers" },
  { id: "panel.servers", label: "Left panel: Servers", describe: "Left panel: Servers", icon: "server" },
  { id: "panel.secrets", label: "Left panel: Secrets", describe: "Left panel: Secrets", icon: "key" },
  { id: "panel.agents", label: "Left panel: AI agents", describe: "Left panel: AI agents and activity", icon: "agent" },
  { id: "panel.tunnels", label: "Left panel: Tunnels", describe: "Left panel: Tunnels", icon: "tunnel" },
  { id: "panel.accounts", label: "Left panel: Accounts", describe: "Left panel: Accounts", icon: "user" },
  // tools and settings
  { id: "snippets.open", label: "Snippets…", describe: "Snippets: saved commands you run often", icon: "snippet", keywords: "saved commands" },
  { id: "tunnels.open", label: "Tunnels (port forwards)…", describe: "Tunnels (port forwards)", icon: "tunnel", keywords: "ssh -L port" },
  { id: "secrets.open", label: "Secrets…", describe: "Secrets", icon: "key", keywords: "vault passwords keys" },
  { id: "vault.lock", label: "Lock the vault", icon: "lock", when: (c) => c.vaultUnlocked },
  { id: "settings", label: "Settings…", describe: "Settings", icon: "settings", keywords: "shortcuts keys preferences" },
  { id: "sounds.open", label: "Sounds…", icon: "bell", keywords: "volume notification" },
  { id: "agents.settings", label: "Agents list…", icon: "agent", keywords: "claude opencode codex settings" },
  { id: "ai.connect", label: "Connect your AI…", icon: "agent", keywords: "mcp claude opencode codex install hooks" },
  { id: "audit.open", label: "What AI agents asked…", icon: "files", keywords: "audit log activity history" },
  { id: "help.open", label: "Shortcuts and help", describe: "Shortcuts and help (also ? when you are not typing)", icon: "bell", keywords: "keys cheat sheet" },
];

/** Every id that has an entry. */
export const ACTION_IDS = new Set(ACTIONS.map((a) => a.id));

export function actionDef(id: string): ActionDef | undefined {
  return ACTIONS.find((a) => a.id === id);
}

/** What the sheet of shortcuts and Settings say for an action. */
export function describe(id: string): string {
  const a = actionDef(id);
  return a ? (a.describe ?? a.label) : id;
}

/** The actions that apply now and are listed (the quiet ones are left out). */
export function applicable(c: ActionCtx): ActionDef[] {
  return ACTIONS.filter((a) => !a.quiet && (!a.when || a.when(c)));
}
