import { describe, expect, it } from "vitest";
import { summary } from "../src/import_ui";

describe("the import summary", () => {
  it("counts in words", () => {
    expect(summary({ keys_added: ["a"], hosts_added: ["x", "y"], problems: [], originals: [] })).toBe("Imported 1 key and 2 hosts.");
    expect(summary({ keys_added: [], hosts_added: [], problems: [], originals: [] })).toBe("Imported 0 keys and 0 hosts.");
  });
});
