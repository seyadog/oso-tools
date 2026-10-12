import { describe, expect, it } from "vitest";
import { looksReady, validShape, withoutPanes } from "../src/layout";
import { Tree, leaf, leaves } from "../src/tree";

const three: Tree = { kind: "split", side: true, pm: 500, a: leaf(0), b: { kind: "split", side: false, pm: 500, a: leaf(1), b: leaf(2) } };

describe("the saved arrangement of a tab", () => {
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
});

describe("when a command may be typed", () => {
  it("accepts prompts", () => {
    for (const l of ["root@web1:~# ", "user@h ~ $", "➜  ~ ", "host% ", "sh-5.2$ "]) expect(looksReady(l), l).toBe(true);
  });
  it("never types at a question", () => {
    for (const l of ["user@h's password: ", "Enter passphrase for key '/k':", "Are you sure (yes/no/[fingerprint])? ", "", "   ", "Last login: Mon"]) expect(looksReady(l), l).toBe(false);
  });
});
