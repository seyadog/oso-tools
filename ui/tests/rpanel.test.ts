import { afterEach, describe, expect, it } from "vitest";
import { Activity, ApprovalRequest } from "../src/api";
import { ApprovalQueue } from "../src/agents_ui";
import { DEFAULTS, buildBindings } from "../src/hotkeys";
import { AgentRow, agentColor, PANEL_DEFAULT, PANEL_MAX, PANEL_MIN, agentName, ago, clampWidth, decisionLabel, latestUpdate, markFor, openFor, resultLabel, sortAgents, whoLabel } from "../src/rpanel";
import { LEFT_DEFAULT, LEFT_MAX, LEFT_MIN, RAIL_WIDTH, SECTIONS, clampLeft, clickSection, readSection } from "../src/chrome";

const act = (over: Partial<Activity> = {}): Activity => ({ id: 1, ts: 1000, updated: 1000, pane: 2, agent: "claude-code", action: "run", host: "web-1", command: "df -h", decision: "allowed", policy: "ask", reason: "", state: "finished", exit_code: 0, bytes_out: 10, preview: "", has_output: false, note: "", ...over });
const req = (id: number, over: Partial<ApprovalRequest> = {}): ApprovalRequest => ({ id, pane: 2, label: "Claude in example, pane 2", agent: "claude", action: "run", host: "example", command: "df -h", production: false, can_remember: true, ...over });

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
    expect(whoLabel(act(), "tab 1 · Local · me")).toBe("Claude · tab 1 · Local · me");
    expect(whoLabel(act({ agent: "" }))).toBe("An AI agent · terminal closed");
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

describe("the rail and the left panel", () => {
  it("has the six sections in the order asked for, each with its own shortcut", () => {
    expect(SECTIONS.map((x) => x.key)).toEqual(["servers", "secrets", "tunnels", "kubernetes", "accounts", "agents"]);
    for (const sec of SECTIONS) expect(DEFAULTS[sec.action], sec.key).toMatch(/^Mod\+Shift\+[1-7]$/);
  });
  it("a click opens only that section, a second click closes it, another one swaps it", () => {
    expect(clickSection("", "secrets")).toBe("secrets");
    expect(clickSection("secrets", "secrets")).toBe("");
    expect(clickSection("secrets", "tunnels")).toBe("tunnels");
  });
  it("keeps the panel between its limits and leaves the terminals room", () => {
    expect(clampLeft(10)).toBe(LEFT_MIN);
    expect(clampLeft(9999)).toBe(LEFT_MAX);
    expect(clampLeft(NaN)).toBe(LEFT_DEFAULT);
    expect(clampLeft(400, 1000, 320)).toBe(272);
    expect(clampLeft(400, 800, 320)).toBe(LEFT_MIN);
    expect(clampLeft(400, 1500, 0)).toBe(400);
    expect(RAIL_WIDTH).toBe(48);
  });
  it("starts on Servers the first time, then on what was left", () => {
    localStorage.removeItem("oso.ui.left");
    expect(readSection()).toBe("servers");
    localStorage.setItem("oso.ui.left", "");
    expect(readSection()).toBe("");
    localStorage.setItem("oso.ui.left", "tunnels");
    expect(readSection()).toBe("tunnels");
    localStorage.setItem("oso.ui.left", "nonsense");
    expect(readSection()).toBe("");
    localStorage.removeItem("oso.ui.left");
  });
});

describe("the Agents tab", () => {
  const row = (over: Partial<AgentRow>): AgentRow => ({ sid: 1, name: "Claude", where: "tab 1", folder: "", branch: "", state: "idle", since: 1000, space: "", source: "process", kind: "claude", cwd: "", spaceId: null, ...over });
  it("puts who needs you first, then who is working, then finished, then idle; the longest wait first", () => {
    const sorted = sortAgents([row({ sid: 1, state: "idle" }), row({ sid: 2, state: "needs_you", since: 5000 }), row({ sid: 3, state: "working" }), row({ sid: 4, state: "needs_you", since: 2000 }), row({ sid: 5, state: "finished" })]);
    expect(sorted.map((r) => r.sid)).toEqual([4, 2, 3, 5, 1]);
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

  it("Allow all answers what comes without asking, but a production host still asks", () => {
    const { q, answers } = make();
    q.push(req(1));
    q.push(req(2, { production: true }));
    q.setAllowAll(true);
    q.push(req(3));
    q.push(req(4, { production: true }));
    expect(answers).toEqual([[1, "once"], [3, "once"]]);
    expect(q.list().map((r) => r.id)).toEqual([2, 4]);
    q.setAllowAll(false);
    q.push(req(5));
    expect(q.list().map((r) => r.id)).toEqual([2, 4, 5]);
  });

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

  it("tells the panel about a new request, but not about an answer", async () => {
    const { q } = make();
    const seen: number[] = [];
    q.onPush((r) => seen.push(r.id));
    const r = req(8);
    q.push(r);
    await q.answer(r, "deny");
    expect(seen).toEqual([8]);
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

describe("agentColor", () => {
  it("gives the first ten projects ten different colours, and the same one every time", () => {
    const colours = Array.from({ length: 10 }, (_, i) => agentColor(`/work/project-${i}`));
    expect(new Set(colours).size).toBe(10);
    expect(agentColor("/work/project-3")).toBe(colours[3]);
  });
});
