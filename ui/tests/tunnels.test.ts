import { describe, expect, it } from "vitest";
import { Forward } from "../src/api";
import { describe as say, isPort, problem, sameTunnel } from "../src/tunnels";

const f = (kind: Forward["kind"], bind: number, host = "db.internal", port = 5432): Forward => ({ id: 1, kind, bind_port: bind, host, port });

describe("tunnels in words", () => {
  it("describes each kind", () => {
    expect(say(f("local", 5433))).toBe("localhost:5433 → db.internal:5432");
    expect(say(f("remote", 8080, "localhost", 3000))).toBe("on the server :8080 → localhost:3000 here");
    expect(say(f("dynamic", 1080))).toBe("SOCKS proxy on localhost:1080");
  });
  it("checks ports", () => {
    expect(["1", "80", "65535"].every(isPort)).toBe(true);
    expect(["", "0", "65536", "-1", "80a", " ", "8 0"].some(isPort)).toBe(false);
  });
  it("checks what is typed", () => {
    expect(problem("local", "5433", "db.internal", "5432")).toBeNull();
    expect(problem("dynamic", "1080", "", "")).toBeNull();
    expect(problem("local", "abc", "h", "1")).toMatch(/number/);
    expect(problem("local", "80", "h", "1")).toMatch(/1024/);
    expect(problem("remote", "80", "h", "1")).toBeNull();
    expect(problem("local", "5433", "", "5432")).toMatch(/host/);
    expect(problem("local", "5433", "a b", "5432")).toMatch(/host/);
    expect(problem("local", "5433", "-oProxyCommand=x", "5432")).toMatch(/host/);
    expect(problem("local", "5433", "db", "")).toMatch(/port/);
  });
  it("recognises a copy", () => {
    expect(sameTunnel(f("local", 1), { ...f("local", 1), id: 9 })).toBe(true);
    expect(sameTunnel(f("local", 1), f("remote", 1))).toBe(false);
  });
});
