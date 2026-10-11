import { describe, expect, it } from "vitest";
import { SecretMeta } from "../src/api";
import { daysOld, isStale, matches } from "../src/secrets_ui";

const m = (over: Partial<SecretMeta> = {}): SecretMeta => ({ id: 1, name: "anthropic-work", kind: "api_key", profile: "work", env_var: "ANTHROPIC_API_KEY", agent_visible: true, notes: "", updated: 0, ...over });
const DAY = 86400;

describe("the secrets list", () => {
  it("filters by text, type and profile", () => {
    expect(matches(m(), "anthro", "", "")).toBe(true);
    expect(matches(m(), "ANTHROPIC_API", "", "")).toBe(true);
    expect(matches(m(), "nothing", "", "")).toBe(false);
    expect(matches(m(), "", "token", "")).toBe(false);
    expect(matches(m(), "", "api_key", "")).toBe(true);
  });
  it("shows the secrets of a profile and the ones shared by all", () => {
    expect(matches(m({ profile: "work" }), "", "", "work")).toBe(true);
    expect(matches(m({ profile: "home" }), "", "", "work")).toBe(false);
    expect(matches(m({ profile: null }), "", "", "work")).toBe(true);
    expect(matches(m({ profile: null }), "", "", "-")).toBe(true);
    expect(matches(m({ profile: "work" }), "", "", "-")).toBe(false);
  });
  it("warns after 90 days", () => {
    const now = 1_800_000_000_000;
    expect(isStale(m({ updated: now / 1000 - 89 * DAY }), now)).toBe(false);
    expect(isStale(m({ updated: now / 1000 - 91 * DAY }), now)).toBe(true);
    expect(daysOld(now / 1000 - 91 * DAY, now)).toBe(91);
    expect(isStale(m({ updated: 0 }), now)).toBe(false);
  });
});
