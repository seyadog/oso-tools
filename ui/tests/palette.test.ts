import { describe, expect, it } from "vitest";
import { ACTIONS, ACTION_IDS, NO_CONTEXT, applicable, describe as describeAction } from "../src/actions";
import { DEFAULTS, LABELS } from "../src/hotkeys";
import { Item, flat, fuzzy, pushRecent, rank } from "../src/palette";

const item = (kind: Item["kind"], id: string, label: string, over: Partial<Item> = {}): Item => ({ id, kind, label, icon: "plus", run: () => {}, ...over });

describe("the fuzzy search", () => {
  it("finds what the letters spell in order, not only together", () => {
    expect(fuzzy("nwag", "New agent")).not.toBeNull();
    expect(fuzzy("nwag", "New agent")!.at).toEqual([0, 2, 4, 5]);
    expect(fuzzy("cfs", "Copy files to or from this server…")).not.toBeNull();
    expect(fuzzy("zzz", "New agent")).toBeNull();
    expect(fuzzy("tnew", "New agent")).toBeNull();
    expect(fuzzy("", "anything")).toEqual({ score: 0, at: [] });
    expect(fuzzy("NEW", "new agent")).not.toBeNull();
    expect(fuzzy("new agent", "New agent")).not.toBeNull();
  });
  it("prefers the whole word, the start of words and the start of the text", () => {
    const score = (q: string, t: string) => fuzzy(q, t)!.score;
    expect(score("agent", "New agent")).toBeGreaterThan(score("agent", "A great end to this"));
    expect(score("ag", "New agent")).toBeGreaterThan(score("ag", "Manage"));
    expect(score("set", "Settings")).toBeGreaterThan(score("set", "Reset the world"));
  });
});

describe("what the palette shows", () => {
  const items = [
    item("action", "agent.new", "New agent…"),
    item("action", "local.new", "New local terminal"),
    item("action", "settings", "Settings…", { keywords: "preferences" }),
    item("server", "1", "web-1", { sub: "root@web1.example.com" }),
    item("server", "2", "db-1", { sub: "admin@db1.example.com" }),
    item("space", "9", "Example API", { sub: "2 servers" }),
    item("secret", "5", "STRIPE_KEY"),
    item("tab", "7", "tab 1 · web-1 · root"),
  ];
  it("groups what matches, each with its title, actions first", () => {
    const g = rank(items, "web");
    expect(g.map((x) => [x.title, x.rows.map((r) => r.item.label)])).toEqual([["Open tabs", ["tab 1 · web-1 · root"]], ["Servers", ["web-1"]]]);
    expect(rank(items, "nwag")[0].rows[0].item.id).toBe("agent.new");
    expect(rank(items, "nothing like it")).toEqual([]);
  });
  it("finds a server by what is under its name, and an action by its other words", () => {
    expect(rank(items, "admin@db1")[0].rows[0].item.label).toBe("db-1");
    expect(rank(items, "preferences")[0].rows[0].item.id).toBe("settings");
  });
  it("shows secrets by name only and puts what was used lately on top, when nothing is typed", () => {
    const g = rank(items, "", { recent: ["settings", "gone", "agent.new"] });
    expect(g[0].title).toBe("Recent");
    expect(g[0].rows.map((r) => r.item.id)).toEqual(["settings", "agent.new"]);
    expect(g[1].rows.map((r) => r.item.id)).toEqual(["local.new"]);
    expect(g.map((x) => x.title)).toEqual(["Recent", "Actions", "Open tabs", "Spaces", "Servers", "Secrets"]);
  });
  it("can be limited to servers (Alt+K): the same palette, already filtered", () => {
    const g = rank(items, "", { only: ["server"], recent: ["settings"] });
    expect(g.map((x) => x.title)).toEqual(["Servers"]);
    expect(flat(g).map((r) => r.item.label)).toEqual(["web-1", "db-1"]);
    expect(flat(rank(items, "db", { only: ["server"] })).map((r) => r.item.label)).toEqual(["db-1"]);
  });
  it("lifts a recent action above one that only matches as well", () => {
    const two = [item("action", "a", "Open settings"), item("action", "b", "Open sounds")];
    expect(rank(two, "open", { recent: ["b"] })[0].rows[0].item.id).toBe("b");
  });
  it("remembers the last eight, newest first, no repeats", () => {
    let r: string[] = [];
    for (const id of ["a", "b", "c", "a", "d", "e", "f", "g", "h", "i"]) r = pushRecent(r, id);
    expect(r).toEqual(["i", "h", "g", "f", "e", "d", "a", "c"]);
  });
});

describe("the one list of actions", () => {
  it("has an entry for every key of the table, and ids are not repeated", () => {
    for (const id of Object.keys(DEFAULTS)) expect(ACTION_IDS.has(id), `${id} has a key but is not registered`).toBe(true);
    expect(new Set(ACTIONS.map((a) => a.id)).size).toBe(ACTIONS.length);
  });
  it("is where the sheet of shortcuts and Settings read their words", () => {
    for (const a of ACTIONS) expect(LABELS[a.id]).toBe(describeAction(a.id));
    expect(LABELS["palette.open"]).toContain("Every command");
    expect(DEFAULTS["palette.open"]).toBe("Mod+Shift+Space");
  });
  it("offers only what applies: files only for an ssh terminal, none of the terminal's actions without one", () => {
    const ids = (c: Partial<typeof NO_CONTEXT>) => applicable({ ...NO_CONTEXT, ...c }).map((a) => a.id);
    expect(ids({ terminal: true, ssh: false })).not.toContain("files.open");
    expect(ids({ terminal: true, ssh: true })).toContain("files.open");
    expect(ids({ terminal: false, ssh: true })).not.toContain("files.open");
    expect(ids({})).not.toContain("tab.close");
    expect(ids({})).toContain("agent.new");
    expect(ids({})).toContain("palette.open");
    expect(ids({ terminal: true, several: false })).not.toContain("broadcast.toggle");
    expect(ids({ terminal: true, several: true })).toContain("broadcast.toggle");
    expect(ids({ terminal: true, several: false, broadcasting: true })).toContain("broadcast.toggle");
    expect(ids({})).not.toContain("attention.next");
    expect(ids({ attention: true })).toContain("attention.next");
    expect(ids({ vaultUnlocked: false })).not.toContain("secret.new");
    expect(ids({ terminal: true, tabs: true })).not.toContain("tab.3");
  });
});
