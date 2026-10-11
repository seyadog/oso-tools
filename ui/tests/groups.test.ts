import { describe, expect, it } from "vitest";
import { looksReady, nextGroupId, parseGroups, summary, validShape, withoutPanes } from "../src/groups";
import { Tree, leaf, leaves } from "../src/tree";

const pair: Tree = { kind: "split", side: true, pm: 500, a: leaf(0), b: leaf(1) };
const three: Tree = { kind: "split", side: true, pm: 500, a: leaf(0), b: { kind: "split", side: false, pm: 500, a: leaf(1), b: leaf(2) } };

describe("saved groups", () => {
  it("reads a good file", () => {
    const g = parseGroups(JSON.stringify([{ id: 3, name: " web ", broadcast: true, shape: pair, panes: [{ server: 2, name: "web-1", command: "uptime" }, { server: 0, name: "Local" }] }]));
    expect(g).toHaveLength(1);
    expect(g[0]).toMatchObject({ id: 3, name: "web", broadcast: true });
    expect(g[0].panes[1]).toEqual({ server: 0, name: "Local", command: "" });
  });

  it("ignores broken files and broken groups without losing the good ones", () => {
    expect(parseGroups(null)).toEqual([]);
    expect(parseGroups("not json")).toEqual([]);
    expect(parseGroups("{}")).toEqual([]);
    const text = JSON.stringify([{ name: "bad", shape: pair, panes: [{ server: 1 }] }, { id: 1, name: "ok", shape: leaf(0), panes: [{ server: 1, name: "a" }] }, 7, null]);
    expect(parseGroups(text).map((g) => g.name)).toEqual(["ok"]);
  });

  it("gives every group its own id and never broadcasts a single terminal", () => {
    const text = JSON.stringify([
      { id: 1, name: "a", broadcast: true, shape: leaf(0), panes: [{ server: 1 }] },
      { id: 1, name: "b", shape: leaf(0), panes: [{ server: 1 }] },
    ]);
    const g = parseGroups(text);
    expect(new Set(g.map((x) => x.id)).size).toBe(2);
    expect(g[0].broadcast).toBe(false);
    expect(nextGroupId(g)).toBeGreaterThan(Math.max(...g.map((x) => x.id)));
  });

  it("checks that a shape holds exactly the terminals 0..n-1", () => {
    expect(validShape(three, 3)).toBe(true);
    expect(validShape(three, 2)).toBe(false);
    expect(validShape({ kind: "split", side: true, pm: 500, a: leaf(0), b: leaf(0) }, 2)).toBe(false);
    expect(validShape({ kind: "leaf" }, 1)).toBe(false);
  });

  it("closes up when a server is gone", () => {
    const g = { shape: three, panes: [{ server: 1, name: "a", command: "" }, { server: 2, name: "b", command: "x" }, { server: 3, name: "c", command: "" }] };
    const r = withoutPanes(g, new Set([1]))!;
    expect(r.panes.map((p) => p.name)).toEqual(["a", "c"]);
    expect(leaves(r.shape)).toEqual([0, 1]);
    expect(withoutPanes(g, new Set([0, 1, 2]))).toBeNull();
  });

  it("summarises what a group holds", () => {
    const g = parseGroups(JSON.stringify([{ id: 1, name: "g", shape: three, panes: [{ server: 1, name: "a" }, { server: 2, name: "b" }, { server: 3, name: "c" }] }]))[0];
    expect(summary(g)).toBe("a · b · c");
    expect(summary(g, 2)).toBe("a · b · +1");
  });
});

describe("when a command may be typed", () => {
  it("accepts prompts", () => {
    for (const l of ["root@web1:~# ", "user@h ~ $", "➜  ~ ", "host% ", "sh-5.2$ "]) expect(looksReady(l), l).toBe(true);
  });
  it("never types at a question", () => {
    for (const l of ["user@h's password: ", "Enter passphrase for key '/k':", "Are you sure (yes/no/[fingerprint])? ", "", "   ", "Last login: Mon"]) expect(looksReady(l), l).toBe(false);
  });
});
