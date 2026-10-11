import { describe, expect, it } from "vitest";
import { MAX_TRIES, STABLE_MS, fatalReason, retryDelay, worthRetrying } from "../src/reconnect";

describe("reconnecting by itself", () => {
  it("retries a connection that was up and dropped", () => {
    expect(worthRetrying(255, 5 * 60 * 1000, 0, false)).toEqual({ retry: true, attempt: 0 });
  });

  it("does not retry a first failure right after connecting (it would only fail again)", () => {
    expect(worthRetrying(255, 1500, 0, false).retry).toBe(false);
  });

  it("keeps trying while the network is still down, then gives up", () => {
    for (let a = 1; a < MAX_TRIES; a++) expect(worthRetrying(255, 800, a, false)).toEqual({ retry: true, attempt: a });
    expect(worthRetrying(255, 800, MAX_TRIES, false).retry).toBe(false);
  });

  it("a connection that held on starts a new chain", () => {
    expect(worthRetrying(255, STABLE_MS, 5, false)).toEqual({ retry: true, attempt: 0 });
  });

  it("never retries what you ended yourself, or this computer's shell", () => {
    expect(worthRetrying(0, 999999, 0, false).retry).toBe(false);
    expect(worthRetrying(1, 999999, 0, false).retry).toBe(false);
    expect(worthRetrying(130, 999999, 0, false).retry).toBe(false);
    expect(worthRetrying(255, 999999, 0, true).retry).toBe(false);
  });

  it("waits longer each time, up to a minute", () => {
    const d = Array.from({ length: MAX_TRIES + 2 }, (_, i) => retryDelay(i));
    expect(d[0]).toBe(2);
    expect([...d].sort((a, b) => a - b)).toEqual(d);
    expect(Math.max(...d)).toBe(60);
  });
});

describe("what trying again cannot fix", () => {
  it("stops at a refused login or a changed key", () => {
    expect(fatalReason("root@h: Permission denied (publickey,password).")).not.toBeNull();
    expect(fatalReason("@@@ WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED! @@@")).not.toBeNull();
    expect(fatalReason("Host key verification failed.")).not.toBeNull();
  });
  it("goes on when it looks like the network", () => {
    expect(fatalReason("ssh: connect to host h port 22: Network is unreachable")).toBeNull();
    expect(fatalReason("Connection to h closed by remote host.")).toBeNull();
    expect(fatalReason("ssh: Could not resolve hostname h")).toBeNull();
  });
});
