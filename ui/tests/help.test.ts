import { describe, expect, it } from "vitest";
import { DEFAULTS, LABELS, buildBindings, comboText } from "../src/hotkeys";
import { shortcutGroups } from "../src/shortcuts_ui";
import { progress, steps } from "../src/start_ui";
import { versionLabel } from "../src/version";

const text = (user: Record<string, unknown> | null) => (a: string) => comboText(user, a).split(" / ")[0];

describe("the sheet of shortcuts", () => {
  it("has every action of the real table, so it cannot go stale", () => {
    const rows = shortcutGroups(text(null)).flatMap((g) => g.rows);
    const shown = rows.map((r) => r.keys).join("\n");
    for (const action of Object.keys(DEFAULTS)) {
      // the families (tab 1…9, the arrows) are one row: their first and last key stand for all
      const key = text(null)(action);
      const family = ["tab.2", "tab.3", "tab.4", "tab.5", "tab.6", "tab.7", "tab.8", "focus.right", "focus.up", "focus.down", "move.right", "move.up", "move.down", "panel.servers", "panel.secrets", "panel.tunnels", "panel.accounts"].includes(action);
      if (!family) expect(shown, action).toContain(key);
    }
    expect(Object.keys(LABELS).length).toBeGreaterThan(30);
  });

  it("follows what is set in hotkeys.json", () => {
    const user = { modifier: "Super", search: "Ctrl+Shift+P" };
    const rows = shortcutGroups(text(user)).flatMap((g) => g.rows);
    expect(rows.find((r) => r.label.startsWith("Open a server"))?.keys).toBe("Ctrl+Shift+P");
    expect(rows.find((r) => r.label.startsWith("Previous"))?.keys).toContain("Super+,");
    expect(rows.map((r) => r.keys).join(" ")).not.toContain("Alt+");
  });

  it("groups them by what they are for, and has the four groups asked for", () => {
    const titles = shortcutGroups(text(null)).map((g) => g.title);
    expect(titles).toEqual(["Open", "Tabs", "Mosaic", "Files and tunnels", "Secrets and agents"]);
  });

  it("puts a new action in More instead of losing it", () => {
    (DEFAULTS as Record<string, string>)["zzz.new"] = "Mod+9999";
    try {
      const more = shortcutGroups(text(null)).find((g) => g.title === "More");
      expect(more?.rows.map((r) => r.label)).toContain("zzz.new");
    } finally {
      delete (DEFAULTS as Record<string, string>)["zzz.new"];
    }
  });

  it("has no two actions on the same keys", () => {
    const { problems } = buildBindings(null);
    expect(problems).toEqual([]);
  });
});

describe("getting started", () => {
  const base = { profiles: 1, apiKeys: null, openedLocal: false, aiConnected: false };
  it("has the four steps in order, each with a button", () => {
    const s = steps(base);
    expect(s.map((x) => x.id)).toEqual(["profile", "key", "terminal", "ai"]);
    expect(s.every((x) => x.button.length > 3 && x.text.length > 20)).toBe(true);
    expect(progress(base)).toEqual({ done: 0, total: 4 });
  });
  it("marks what is done", () => {
    const done = progress({ profiles: 2, apiKeys: 1, openedLocal: true, aiConnected: true });
    expect(done).toEqual({ done: 4, total: 4 });
    expect(steps({ ...base, profiles: 3 })[0].done).toBe(true);
    expect(steps({ ...base, apiKeys: 0 })[1].done).toBe(false);
  });
});

describe("the version", () => {
  it("says the version and, when known, the commit", () => {
    expect(versionLabel("0.3.0", "a1b2c3d")).toBe("v0.3.0 · a1b2c3d");
    expect(versionLabel("0.3.0", "")).toBe("v0.3.0");
  });
});
