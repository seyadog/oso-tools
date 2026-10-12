import { describe, expect, it } from "vitest";
import { Space, TreeData } from "../src/api";
import { coveredServers, emptySpace, missingOf, openSpaces, spaceSummary, tabFor, withTab } from "../src/spaces";
import { leaf } from "../src/tree";

const tree = {
  folders: [
    { id: 10, name: "prod", parent: null, expanded: true },
    { id: 11, name: "eu", parent: 10, expanded: true },
    { id: 12, name: "other", parent: null, expanded: true },
  ],
  servers: [1, 2, 3, 4, 5].map((id) => ({ id, name: `s${id}`, parent: ({ 2: 10, 3: 11, 4: 12 } as Record<number, number>)[id] ?? null })),
} as unknown as Pick<TreeData, "servers" | "folders">;

const space = (over: Partial<Space> = {}): Space => ({ ...emptySpace(), name: "Example", ...over });

describe("what a Space covers", () => {
  it("is its servers and everything under its folders, any depth, without what was deleted", () => {
    expect(coveredServers(space({ servers: [1, 99], server_folders: [10, 77] }), tree)).toEqual([1, 2, 3]);
    expect(coveredServers(space(), tree)).toEqual([]);
    expect(coveredServers(space({ server_folders: [12] }), tree)).toEqual([4]);
  });
  it("is described in a row, and says what no longer exists", () => {
    const sp = space({ servers: [1], server_folders: [10], folders: ["/srv/a", "/srv/b"], tabs: [tabFor("t", leaf(0), false, [{ server: 1, name: "s1", cwd: "" }])], secrets: [5, 6] });
    expect(spaceSummary(sp, tree)).toBe("3 servers · 2 folders · 1 tab · 2 secrets");
    expect(spaceSummary(space(), tree)).toBe("0 servers · 0 folders · 0 tabs");
    expect(missingOf(space({ servers: [1, 99], server_folders: [10, 77], secrets: [5, 6] }), tree, [5])).toEqual({ servers: 1, folders: 1, secrets: 1 });
    expect(missingOf(space({ secrets: [5, 6] }), tree, null).secrets).toBe(0);
  });
});

describe("a tab saved in a Space", () => {
  it("keeps the terminals and covers their servers, never counting this computer as one", () => {
    const tab = tabFor("ops", leaf(0), true, [{ server: 3, name: "s3", cwd: "" }, { server: 0, name: "Local", cwd: "/srv/a" }]);
    expect(tab.broadcast).toBe(true);
    expect(tab.panes[1]).toEqual({ server: 0, name: "Local", command: "", cwd: "/srv/a" });
    const next = withTab(space({ servers: [1] }), tab);
    expect(next.servers).toEqual([1, 3]);
    expect(next.tabs).toHaveLength(1);
    expect(tabFor("one", leaf(0), true, [{ server: 1, name: "s1", cwd: "" }]).broadcast).toBe(false);
  });
  it("counts the open terminals of each Space", () => {
    const m = openSpaces([{ space: 1, dead: false }, { space: 1, dead: false }, { space: 2, dead: true }, { dead: false }]);
    expect([...m]).toEqual([[1, 2]]);
  });
});
