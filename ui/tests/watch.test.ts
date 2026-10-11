import { describe, expect, it } from "vitest";
import { ECHO_MS, QUIET_ANY_MS, QUIET_AT_PROMPT_MS, noteInput, noteOutput, startWatch, verdict } from "../src/watch";

describe("telling you when a terminal is done", () => {
  it("says nothing before anything happened (a quiet terminal is not a finished one)", () => {
    const w = startWatch(0);
    expect(verdict(w, 60_000, "user@h:~$ ")).toBeNull();
  });

  it("speaks when output stops and the prompt is back", () => {
    const w = startWatch(0);
    noteOutput(w, 1000);
    noteOutput(w, 5000);
    expect(verdict(w, 5000 + QUIET_AT_PROMPT_MS - 1, "user@h:~$ ")).toBeNull();
    expect(verdict(w, 5000 + QUIET_AT_PROMPT_MS, "user@h:~$ ")).toBe("prompt");
  });

  it("waits while the command is still printing", () => {
    const w = startWatch(0);
    for (let t = 1000; t < 30000; t += 1000) {
      noteOutput(w, t);
      expect(verdict(w, t + 500, "Compiling foo v0.1")).toBeNull();
    }
  });

  it("without a prompt it speaks only after a long silence", () => {
    const w = startWatch(0);
    noteOutput(w, 1000);
    expect(verdict(w, 1000 + QUIET_AT_PROMPT_MS + 1, "Do you want to continue? [Y/n]")).toBeNull();
    expect(verdict(w, 1000 + QUIET_ANY_MS, "Do you want to continue? [Y/n]")).toBe("quiet");
  });

  it("does not count the echo of your own typing as the command working", () => {
    const w = startWatch(0);
    noteInput(w, 10_000);
    noteOutput(w, 10_000 + ECHO_MS - 1);
    expect(w.saw).toBe(false);
    expect(verdict(w, 60_000, "user@h:~$ ")).toBeNull();
    noteOutput(w, 10_000 + ECHO_MS + 100);
    expect(w.saw).toBe(true);
  });
});
