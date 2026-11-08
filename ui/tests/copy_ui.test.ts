import { describe, expect, it } from "vitest";
import { copyRows } from "../src/copy_ui";

describe("copy card rows", () => {
  it("an IP server gives name, IPv4, user, user@address and the ssh line", () => {
    const rows = copyRows({ name: "web", host: "10.0.0.5", user: "me", port: 22 });
    expect(rows).toEqual([["Name", "web"], ["IPv4", "10.0.0.5"], ["User", "me"], ["User@address", "me@10.0.0.5"], ["ssh command", "ssh me@10.0.0.5"]]);
  });
  it("a name, another port and a jump host show up", () => {
    const rows = copyRows({ name: "db", host: "db.example.com", user: "", port: 2222 }, "bastion");
    expect(rows).toContainEqual(["Address", "db.example.com"]);
    expect(rows).toContainEqual(["Port", "2222"]);
    expect(rows).toContainEqual(["ssh command", "ssh -p 2222 db.example.com"]);
    expect(rows).toContainEqual(["Reached via", "bastion"]);
  });
  it("an IPv6 address is labelled as such", () => {
    expect(copyRows({ name: "n", host: "fd7a:115c::1", user: "me", port: 22 })[1]).toEqual(["IPv6", "fd7a:115c::1"]);
  });
});
