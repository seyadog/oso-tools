import { describe, expect, it } from "vitest";
import { ToolAccounts } from "../src/api";
import { accountChoices } from "../src/accounts_ui";

const tool: ToolAccounts = { installed: true, error: null, accounts: [{ id: "github.com/me", name: "me", detail: "github.com", active: true }] };

describe("account choices", () => {
  it("start with leaving the tool alone", () => {
    expect(accountChoices(tool, null)[0]).toEqual(["", "Leave it alone"]);
    expect(accountChoices(tool, null)[1]).toEqual(["github.com/me", "me · github.com"]);
  });
  it("keep a saved account the tool no longer lists", () => {
    expect(accountChoices(tool, "old-user").at(-1)).toEqual(["old-user", "old-user"]);
    expect(accountChoices(tool, "github.com/me")).toHaveLength(2);
    expect(accountChoices(undefined, null)).toHaveLength(1);
  });
});
