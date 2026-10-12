import { describe, expect, it } from "vitest";
import { AgentState, AgentStatus, baseName, effectiveState, guessFromScreen, nextAttention, noticesFor, proposedBranch, tabDot } from "../src/agents";

const status = (over: Partial<AgentStatus> = {}): AgentStatus => ({ sid: 1, kind: "claude", name: "Claude Code", pid: 7, cwd: "/srv/example", folder: "example", branch: "main", state: "idle", since: 1000, source: "process", ...over });

describe("a guess from the screen", () => {
  it("says it needs you when the last lines ask a question, and that it works when output is arriving", () => {
    for (const l of ["Do you want to proceed? (y/n)", "Allow this command? [Y/n]", "  ❯ 1. Yes\n  Do you want to make this edit?", "Press Enter to continue", "Would you like to apply it?"]) {
      expect(guessFromScreen(["some output", ...l.split("\n"), ""], 9000), l).toBe("needs_you");
    }
    expect(guessFromScreen(["building…"], 500)).toBe("working");
    expect(guessFromScreen(["building…"], 5000)).toBeNull();
    expect(guessFromScreen(["building…"], -1)).toBeNull();
    // an old question far up the screen is not the last thing said
    expect(guessFromScreen(["Do you want to proceed? (y/n)", "y", "1", "2", "3", "4", "5", "6", "7"], 9000)).toBeNull();
  });
  it("is only used when nothing exact is known", () => {
    expect(effectiveState(status(), "working")).toEqual({ state: "working", guessed: true });
    expect(effectiveState(status(), null)).toEqual({ state: "idle", guessed: false });
    expect(effectiveState(status({ source: "hook", state: "idle" }), "working")).toEqual({ state: "idle", guessed: false });
    expect(effectiveState(status({ source: "hook", state: "finished" }), "needs_you")).toEqual({ state: "finished", guessed: false });
    expect(effectiveState(status({ source: "approval", state: "needs_you" }), "working")).toEqual({ state: "needs_you", guessed: false });
    expect(effectiveState(status({ source: "process", state: "finished" }), "working").guessed).toBe(false);
  });
});

describe("the attention queue", () => {
  const w = (sid: number, state: AgentState, since: number) => ({ sid, state, since });
  it("goes to who needs you, the one that has waited longest first, then the next, and starts again after the last", () => {
    const list = [w(5, "needs_you", 300), w(2, "needs_you", 100), w(9, "working", 50), w(7, "needs_you", 200), w(4, "idle", 10)];
    const walk: number[] = [];
    let at: number | null = 9; // on a terminal that is not waiting
    for (let i = 0; i < 5; i++) {
      at = nextAttention(list, at);
      walk.push(at!);
    }
    expect(walk).toEqual([2, 7, 5, 2, 7]);
  });
  it("takes the one that is not on screen yet when you are on a terminal that stopped waiting", () => {
    expect(nextAttention([w(2, "needs_you", 100), w(3, "needs_you", 200)], 3)).toBe(2);
    expect(nextAttention([w(2, "needs_you", 100)], 2)).toBe(2);
  });
  it("falls back to the ones that finished when nobody needs you, and to nobody", () => {
    expect(nextAttention([w(1, "working", 1), w(4, "finished", 40), w(3, "finished", 30)], null)).toBe(3);
    expect(nextAttention([w(1, "working", 1), w(2, "idle", 2)], null)).toBeNull();
    expect(nextAttention([], 1)).toBeNull();
  });
  it("breaks a tie by the terminal", () => {
    expect(nextAttention([w(8, "needs_you", 100), w(3, "needs_you", 100)], null)).toBe(3);
  });
});

describe("what is said from outside the window", () => {
  const now = (state: AgentState, sid = 1) => [{ sid, name: "Claude Code", state, where: "tab 1 · Local · seya" }];
  it("is said when an agent starts to need you, and when one that was working finishes, once", () => {
    expect(noticesFor(new Map([[1, "working"]]), now("needs_you"))).toEqual([{ event: "needs", sid: 1, title: "Claude Code needs you", body: "tab 1 · Local · seya" }]);
    expect(noticesFor(new Map([[1, "working"]]), now("finished"))).toEqual([{ event: "finished", sid: 1, title: "Claude Code finished", body: "tab 1 · Local · seya" }]);
    expect(noticesFor(new Map([[1, "needs_you"]]), now("needs_you"))).toEqual([]);
  });
  it("says nothing the first time an agent is seen, nor for a finish that was not after work, nor for work or rest", () => {
    expect(noticesFor(new Map(), now("needs_you"))).toEqual([]);
    expect(noticesFor(new Map([[1, "idle"]]), now("finished"))).toEqual([]);
    expect(noticesFor(new Map([[1, "idle"]]), now("working"))).toEqual([]);
    expect(noticesFor(new Map([[1, "needs_you"]]), now("idle"))).toEqual([]);
  });
});

describe("the small things", () => {
  it("a tab shows the most urgent dot of its agents", () => {
    expect(tabDot(["idle", "working", "needs_you"])).toBe("needs_you");
    expect(tabDot(["idle", "finished"])).toBe("finished");
    expect(tabDot([])).toBeNull();
  });
  it("proposes a branch from the agent and the time, and names a folder by its last part", () => {
    expect(proposedBranch("claude", new Date(2026, 9, 11, 14, 30))).toBe("oso/claude-261011-1430");
    expect(proposedBranch("My Agent!", new Date(2026, 0, 2, 3, 4))).toBe("oso/MyAgent-260102-0304");
    expect(proposedBranch("", new Date(2026, 0, 2, 3, 4))).toBe("oso/agent-260102-0304");
    expect(baseName("/home/u/code/example/")).toBe("example");
    expect(baseName("example")).toBe("example");
  });
});
