// The sheet of shortcuts. It is built from the same table the keys are read from (defaults plus hotkeys.json), so it
// can never say something that is not true, and a shortcut added later shows up in it without anyone remembering to.

import { DEFAULTS, LABELS } from "./hotkeys";
import { el, openModal } from "./dom";

export interface Row {
  keys: string;
  label: string;
}
export interface Group {
  title: string;
  rows: Row[];
}

/** Which actions belong together. Anything not listed lands in "More", so a new action is never lost. */
const GROUPS: { title: string; actions: string[] }[] = [
  { title: "Open", actions: ["search", "local.new", "tab.new", "sidebar.toggle", "rpanel.toggle", "settings", "help.open"] },
  { title: "Tabs", actions: ["tab.1", "tab.prev", "tab.next", "tab.close", "tab.rename", "group.save", "watch.toggle"] },
  { title: "Mosaic", actions: ["mode.toggle", "split.right", "split.down", "focus.left", "move.left", "broadcast.toggle"] },
  { title: "Files and tunnels", actions: ["files.open", "tunnels.open"] },
  { title: "Secrets and agents", actions: ["secrets.open", "sudo.fill"] },
];

/** Actions shown as one row with the others of their kind: `Alt+1 … Alt+9`, `Alt+Left …`. */
const FAMILY: Record<string, { members: string[]; label: string }> = {
  "tab.1": { members: ["tab.1", "tab.2", "tab.3", "tab.4", "tab.5", "tab.6", "tab.7", "tab.8", "tab.9"], label: "Go to tab 1 … 9 (9 is the last one)" },
  "focus.left": { members: ["focus.left", "focus.right", "focus.up", "focus.down"], label: "Move between terminals" },
  "move.left": { members: ["move.left", "move.right", "move.up", "move.down"], label: "Move this terminal (needs a mosaic)" },
};

/** Things that are not keys of the table but that you need to know, next to the group they belong to. */
const EXTRA: Record<string, Row[]> = {
  Tabs: [{ keys: "Enter", label: "On a terminal that ended: connect again" }],
  "Secrets and agents": [
    { keys: "Esc", label: "Deny what an AI agent asks, in its window" },
    { keys: "(no shortcut)", label: "An agent never gets a key press: it asks in a window and you answer" },
  ],
};

export function shortcutGroups(text: (action: string) => string): Group[] {
  const used = new Set<string>();
  const label = (a: string) => LABELS[a] ?? a;
  const groups: Group[] = GROUPS.map((g) => ({
    title: g.title,
    rows: [
      ...g.actions.map((a): Row => {
        const fam = FAMILY[a];
        (fam?.members ?? [a]).forEach((m) => used.add(m));
        if (fam) return { keys: `${text(fam.members[0])} … ${text(fam.members[fam.members.length - 1])}`, label: fam.label };
        return { keys: text(a) || "—", label: label(a) };
      }),
      ...(EXTRA[g.title] ?? []),
    ],
  }));
  // tab.prev / tab.next show as a pair
  const tabs = groups.find((g) => g.title === "Tabs")!;
  const at = tabs.rows.findIndex((r) => r.label === label("tab.prev"));
  if (at >= 0) tabs.rows.splice(at, 2, { keys: `${text("tab.prev")}  ${text("tab.next")}`, label: "Previous / next tab" });
  const more = Object.keys(DEFAULTS).filter((a) => !used.has(a) && !GROUPS.some((g) => g.actions.includes(a)));
  if (more.length) groups.push({ title: "More", rows: more.map((a) => ({ keys: text(a) || "—", label: label(a) })) });
  return groups;
}

export function shortcutsSheet(text: (action: string) => string, onSettings: () => void) {
  openModal((close) => {
    const body = el("div", { class: "sheet" });
    for (const g of shortcutGroups(text)) {
      const sec = el("section", {}, el("h3", {}, g.title));
      for (const r of g.rows) sec.append(el("div", { class: "g-row" }, el("span", { class: "g-keys" }, el("kbd", {}, r.keys)), el("span", {}, r.label)));
      body.append(sec);
    }
    const settings = el("button", { class: "btn", type: "button", "data-act": "shortcuts-settings" }, "Change shortcuts…");
    settings.onclick = () => {
      close();
      onSettings();
    };
    const done = el("button", { class: "btn primary", type: "button" }, "Done");
    done.onclick = close;
    return el("div", { class: "wide sheet-box", "data-sheet": "shortcuts" }, el("h2", {}, "Shortcuts"), el("div", { class: "hint", style: "margin:0 0 10px" }, "Press ? any time you are not typing (or the Help shortcut) to open this. The main key can be changed in Settings."), body, el("div", { class: "actions" }, settings, done));
  });
}
