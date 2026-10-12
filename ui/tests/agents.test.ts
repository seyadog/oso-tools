import { describe, expect, it } from "vitest";
import { ApprovalRequest, AuditEntry } from "../src/api";
import { actionTitle, auditLine } from "../src/agents_ui";

const req = (over: Partial<ApprovalRequest> = {}): ApprovalRequest => ({ id: 1, pane: 2, label: "Claude in example-api, pane 2", agent: "claude-code", action: "run", host: "example", command: "df -h", production: false, can_remember: true, ...over });

describe("what an agent asks", () => {
  it("says who asks and what for", () => {
    expect(actionTitle(req())).toBe("Claude in example-api, pane 2 wants to run a command");
    expect(actionTitle(req({ action: "sudo" }))).toContain("use sudo on this computer");
    expect(actionTitle(req({ action: "run_with" }))).toContain("with secrets");
    expect(actionTitle(req({ action: "tunnel" }))).toContain("tunnel");
  });
  it("writes a line for the record", () => {
    const e: AuditEntry = { ts: 1, pane: 2, agent: "x", action: "run", host: "web-1", command: "df -h", decision: "allowed_without_asking", exit_code: 0, note: "" };
    expect(auditLine(e)).toBe("run: df -h on web-1 → allowed without asking (exit 0)");
    expect(auditLine({ ...e, command: "", host: "", exit_code: null, decision: "blocked", action: "context" })).toBe("context → blocked");
  });
});

import { MIN_WORK_MS, QUIET_MS, doneByQuiet, workStart } from "../src/agents";
describe("an agent without hooks", () => {
  it("is done when it worked and then stayed quiet", () => {
    expect(doneByQuiet(undefined, 1000, 100000)).toBe(false);
    expect(doneByQuiet(5, 100000 - QUIET_MS + 1, 100000)).toBe(false);
    expect(doneByQuiet(5, 100000 - QUIET_MS, 100000)).toBe(true);
    expect(doneByQuiet(5, 0, 100000)).toBe(false);
  });
});

describe("a blink of output is not work", () => {
  it("needs a long enough run before silence means done", () => {
    expect(doneByQuiet(5, 1, 100000, MIN_WORK_MS - 1)).toBe(false);
    expect(doneByQuiet(5, 1, 100000, MIN_WORK_MS)).toBe(true);
  });
  it("a silence longer than the quiet time starts a new run", () => {
    expect(workStart(undefined, undefined, 50)).toBe(50);
    expect(workStart(10, 40, 50)).toBe(10);
    expect(workStart(10, 40, 40 + QUIET_MS + 1)).toBe(40 + QUIET_MS + 1);
  });
});
