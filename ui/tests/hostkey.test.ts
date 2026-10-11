import { describe, expect, it } from "vitest";
import { parseHostKeyWarning } from "../src/hostkey";

const REAL = `@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
@    WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!     @
@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
IT IS POSSIBLE THAT SOMEONE IS DOING SOMETHING NASTY!
Someone could be eavesdropping on you right now (man-in-the-middle attack)!
It is also possible that a host key has just been changed.
The fingerprint for the ED25519 key sent by the remote host is
SHA256:Zx3fKq1o0Yg4mYfX9lq1nDq0vN7q1wQmP8tGk3m1v2A.
Please contact your system administrator.
Add correct host key in /home/u/.ssh/known_hosts to get rid of this message.
Offending ECDSA key in /home/u/.ssh/known_hosts:3
Host key for web1.example.com has changed and you have requested strict checking.
Host key verification failed.`;

describe("the changed-key warning of ssh", () => {
  it("is found with the host and the new fingerprint", () => {
    const w = parseHostKeyWarning(REAL)!;
    expect(w.host).toBe("web1.example.com");
    expect(w.fingerprint).toBe("SHA256:Zx3fKq1o0Yg4mYfX9lq1nDq0vN7q1wQmP8tGk3m1v2A.");
  });
  it("copes with a port in the name", () => {
    expect(parseHostKeyWarning(REAL.replace("web1.example.com has", "[web1.example.com]:2222 has"))!.host).toBe("[web1.example.com]:2222");
  });
  it("is found even when ssh did not print every line", () => {
    expect(parseHostKeyWarning("WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!")).toEqual({ host: null, fingerprint: null });
  });
  it("is not found in ordinary output", () => {
    expect(parseHostKeyWarning("Welcome to web-1\nuser@web-1:~$ ")).toBeNull();
    expect(parseHostKeyWarning("Permission denied (publickey).")).toBeNull();
  });
});
