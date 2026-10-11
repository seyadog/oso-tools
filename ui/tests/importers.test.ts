import { describe, expect, it } from "vitest";
import { sentence } from "../src/importers_ui";

describe("the import sentence", () => {
  it("counts what came in and says what was left out", () => {
    expect(sentence({ added: ["a"], skipped: [], originals: [] })).toBe("Imported 1 secret.");
    expect(sentence({ added: ["a", "b"], skipped: ["app/X: already in the vault"], originals: [] })).toBe("Imported 2 secrets. 1 skipped (app/X: already in the vault).");
    expect(sentence({ added: [], skipped: ["1", "2", "3"], originals: [] })).toBe("Imported 0 secrets. 3 skipped (1; 2…).");
  });
});
