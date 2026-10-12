import { afterEach, describe, expect, it } from "vitest";
import { Activity, ApprovalRequest } from "../src/api";
import { ApprovalQueue } from "../src/agents_ui";
import { DEFAULTS, buildBindings } from "../src/hotkeys";
import { sessionAge, PANEL_DEFAULT, PANEL_MAX, PANEL_MIN, agentName, ago, clampWidth, decisionLabel, latestUpdate, markFor, openFor, profileOf, resultLabel, whoLabel } from "../src/rpanel";

const act = (over: Partial<Activity> = {}): Activity => ({ id: 1, ts: 1000, updated: 1000, pane: 2, agent: "claude-code", action: "run", host: "web-1", command: "df -h", decision: "allowed", policy: "ask", reason: "", state: "finished", exit_code: 0, bytes_out: 10, preview: "", note: "", ...over });
const req = (id: number, over: Partial<ApprovalRequest> = {}): ApprovalRequest => ({ id, pane: 2, label: "Claude in acme, pane 2", agent: "claude", action: "run", host: "acme", command: "df -h", production: false, can_remember: true, ...over });

describe("the words of the panel", () => {
  it("names the agent and says how long ago", () => {
    expect(agentName("claude-code")).toBe("Claude");
    expect(agentName("opencode")).toBe("Opencode");
    expect(agentName("")).toBe("An AI agent");
    expect(ago(100, 110)).toBe("just now");
    expect(ago(100, 100 + 5 * 60)).toBe("5 min ago");
    expect(ago(100, 100 + 2 * 3600)).toBe("2 h ago");
    expect(ago(100, 100 + 3 * 86400)).toBe("3 d ago");
    expect(openFor(undefined, 5)).toBe("");
    expect(openFor(0, 30_000)).toBe("just opened");
    expect(openFor(0, 12 * 60_000)).toBe("12 min");
    expect(openFor(0, 130 * 60_000)).toBe("2 h 10 min");
    expect(openFor(0, 3 * 86400_000)).toBe("3 d");
    expect(sessionAge(0, 12 * 60_000)).toBe("open 12 min");
    expect(sessionAge(0, 5000)).toBe("just opened");
    expect(sessionAge(undefined, 5000)).toBe("");
  });

  it("says who decided: you, the policy, or the policy refusing", () => {
    expect(decisionLabel(act()).text).toBe("You allowed it");
    expect(decisionLabel(act({ decision: "allowed_without_asking", policy: "read_only_auto" }))).toEqual({ text: "Automatic: read-only policy", tone: "auto" });
    expect(decisionLabel(act({ decision: "allowed_without_asking", policy: "" })).text).toBe("Automatic");
    expect(decisionLabel(act({ decision: "denied" })).tone).toBe("no");
    expect(decisionLabel(act({ decision: "blocked" })).tone).toBe("blocked");
    expect(decisionLabel(act({ decision: "allowed_and_remembered_for_reads" })).text).toContain("reads");
  });

  it("says how it ended", () => {
    expect(resultLabel(act())).toBe("exit 0");
    expect(resultLabel(act({ exit_code: 3 }))).toBe("exit 3");
    expect(resultLabel(act({ exit_code: null }))).toBe("timed out");
    expect(resultLabel(act({ state: "running" }))).toBe("running…");
    expect(resultLabel(act({ state: "denied" }))).toBe("not run");
    expect(resultLabel(act({ state: "done" }))).toBe("");
    expect(whoLabel(act(), "web-1")).toBe("Claude · pane 2 (web-1)");
    expect(whoLabel(act({ agent: "" }))).toBe("An AI agent · pane 2");
  });
});

describe("the mark on the arrow", () => {
  it("is amber for a request waiting, a dot for news, nothing otherwise", () => {
    expect(markFor(1, 0, 0)).toBe("pending");
    expect(markFor(0, 20, 10)).toBe("new");
    expect(markFor(0, 10, 10)).toBe("none");
    expect(markFor(0, 0, 0)).toBe("none");
    expect(latestUpdate([act({ ts: 5, updated: 9 }), act({ ts: 12, updated: 12 })])).toBe(12);
    expect(latestUpdate([])).toBe(0);
  });
});

describe("the width", () => {
  it("stays between the limits and leaves the terminals room", () => {
    expect(clampWidth(10)).toBe(PANEL_MIN);
    expect(clampWidth(9999)).toBe(PANEL_MAX);
    expect(clampWidth(NaN)).toBe(PANEL_DEFAULT);
    expect(clampWidth(500, 1000)).toBe(376);
    expect(clampWidth(500, 800)).toBe(PANEL_MIN);
    expect(clampWidth(400, 1500)).toBe(400);
    expect(clampWidth(900, 1500)).toBe(PANEL_MAX);
  });
});

describe("the profile of a session", () => {
  const tree = {
    profiles: [{ id: "work", name: "Work" }, { id: "home", name: "Home" }] as never,
    folders: [{ id: 1, name: "a", parent: null, expanded: true, profile: "work" }, { id: 2, name: "b", parent: 1, expanded: true, profile: null }] as never,
    servers: [{ id: 10, profile: "home", parent: 2 }, { id: 11, profile: null, parent: 2 }, { id: 12, profile: null, parent: null }] as never,
  };
  it("is its own, then its folder's up the tree, then every profile", () => {
    expect(profileOf(tree, 10)).toBe("Home");
    expect(profileOf(tree, 11)).toBe("Work");
    expect(profileOf(tree, 12)).toBe("every profile");
    expect(profileOf(tree, 99)).toBe("");
  });
});

describe("the shortcut", () => {
  it("exists, and no other action uses the same keys", () => {
    expect(DEFAULTS["rpanel.toggle"]).toBe("Mod+J");
    expect(buildBindings(null).problems).toEqual([]);
    const keys = Object.values(DEFAULTS).flat();
    expect(keys.filter((k) => k === "Mod+J").length).toBe(1);
  });
});

describe("requests waiting for an answer", () => {
  afterEach(() => document.getElementById("approval-bg")?.remove());
  const make = () => {
    const answers: [number, string][] = [];
    const typed: number[] = [];
    const q = new ApprovalQueue({ be: { approvalAnswer: async (id: number, d: string) => (answers.push([id, d]), true) } as never, openInTerminal: async (r) => void typed.push(r.id) });
    return { q, answers, typed };
  };

  it("lists what is waiting, and an answer from the panel takes it off and tells the agent once", async () => {
    const { q, answers } = make();
    let changes = 0;
    q.onChange(() => changes++);
    const a = req(1);
    const b = req(2);
    q.push(a);
    q.push(b);
    expect(q.list().map((r) => r.id)).toEqual([1, 2]);
    expect(q.pending).toBe(2);
    await q.answer(b, "once");
    await q.answer(b, "deny"); // a second answer to the same request does nothing
    expect(answers).toEqual([[2, "once"]]);
    expect(q.list().map((r) => r.id)).toEqual([1]);
    expect(changes).toBe(3);
  });

  it("shows no window while the panel answers, and shows what is waiting when the panel closes", () => {
    const { q } = make();
    q.setInline(true);
    q.push(req(1));
    expect(document.getElementById("approval-bg")).toBeNull();
    q.setInline(false);
    expect(document.getElementById("approval-bg")).not.toBeNull();
    expect(document.getElementById("approval-bg")!.textContent).toContain("wants to run a command");
    q.setInline(true);
    expect(document.getElementById("approval-bg")).toBeNull();
  });

  it("\"in a terminal\" opens the terminal and answers terminal", async () => {
    const { q, answers, typed } = make();
    const r = req(5);
    q.push(r);
    await q.answer(r, "terminal");
    expect(typed).toEqual([5]);
    expect(answers).toEqual([[5, "terminal"]]);
  });
});
