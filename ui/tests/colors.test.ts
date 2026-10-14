import { describe, expect, it } from "vitest";
import { PALETTE, projectColor, serverColor, setProjectColors, setServerColors } from "../src/colors";

describe("colours that stay", () => {
  it("a server wears the colour it was given, and the local terminal is grey", () => {
    setServerColors([{ id: 7, color: 2 }, { id: 9, color: 5 }]);
    expect(serverColor(7)).toBe(PALETTE[2]);
    expect(serverColor(9)).toBe(PALETTE[5]);
    expect(serverColor(0)).toBe("#6c7086");
  });

  it("the palette has twelve colours that all differ", () => {
    expect(new Set(PALETTE).size).toBe(12);
  });

  it("projects get free colours, the saved ones are kept, and new ones are saved", () => {
    const saved: Record<string, number>[] = [];
    setProjectColors({ "/a": 4 }, (m) => saved.push(m));
    expect(projectColor("/a")).toBe(PALETTE[4]);
    expect(saved.length).toBe(0);
    const got = ["/b", "/c", "/d", "/e", "/f", "/g", "/h", "/i", "/j"].map((k) => projectColor(k));
    expect(new Set([PALETTE[4], ...got]).size).toBe(10);
    expect(projectColor("/b")).toBe(got[0]);
    expect(saved.at(-1)).toMatchObject({ "/a": 4 });
    expect(Object.keys(saved.at(-1)!).length).toBe(10);
  });
});
