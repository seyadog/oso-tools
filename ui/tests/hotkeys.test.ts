import { describe, expect, it } from "vitest";
import { DEFAULTS, buildBindings, comboText, matchAction, parseCombo } from "../src/hotkeys";

const key = (code: string, mods: Partial<{ altKey: boolean; ctrlKey: boolean; shiftKey: boolean; metaKey: boolean }> = {}) => ({
  altKey: false,
  ctrlKey: false,
  shiftKey: false,
  metaKey: false,
  code,
  ...mods,
});

describe("hotkeys", () => {
  it("my defaults work by key position, so any layout gets them", () => {
    const { bindings, problems } = buildBindings(null);
    expect(problems).toEqual([]);
    expect(matchAction(bindings, key("Digit2", { altKey: true }))).toBe("tab.2");
    expect(matchAction(bindings, key("Digit2", { altKey: true, shiftKey: true }))).toBe("panel.servers"); // (it used to be "send to screen")
    expect(matchAction(bindings, key("KeyM", { altKey: true }))).toBe("mode.toggle");
    expect(matchAction(bindings, key("ArrowLeft", { altKey: true, shiftKey: true }))).toBe("move.left");
    expect(matchAction(bindings, key("Comma", { altKey: true }))).toBe("tab.prev");
    expect(matchAction(bindings, key("KeyN", { ctrlKey: true }))).toBe("tab.new");
    expect(matchAction(bindings, key("KeyM"))).toBeNull();
    expect(matchAction(bindings, key("KeyM", { altKey: true, ctrlKey: true }))).toBeNull();
  });

  it("the user's file changes, adds and removes shortcuts", () => {
    const { bindings, problems } = buildBindings({ "mode.toggle": "Super+M", "split.right": ["Alt+V", "Ctrl+Shift+D"], "tab.close": null });
    expect(problems).toEqual([]);
    expect(matchAction(bindings, key("KeyM", { altKey: true }))).toBeNull();
    expect(matchAction(bindings, key("KeyM", { metaKey: true }))).toBe("mode.toggle");
    expect(matchAction(bindings, key("KeyD", { ctrlKey: true, shiftKey: true }))).toBe("split.right");
    expect(matchAction(bindings, key("KeyW", { altKey: true }))).toBeNull();
  });

  it("mistakes are reported and do not break the rest", () => {
    const { bindings, problems } = buildBindings({ "nope.nothing": "Alt+X", "mode.toggle": "Alt+Bogus", "search": 5, "tab.close": "Alt+V" });
    expect(problems.some((p) => p.includes("Unknown action"))).toBe(true);
    expect(problems.some((p) => p.includes("not a valid shortcut"))).toBe(true);
    expect(problems.some((p) => p.includes("must be a text"))).toBe(true);
    expect(problems.some((p) => p.includes("used by both"))).toBe(true);
    expect(matchAction(bindings, key("Digit1", { altKey: true }))).toBe("tab.1");
  });

  it("one setting changes the main key of every default shortcut at once", () => {
    const sup = buildBindings({ modifier: "Super" });
    expect(sup.problems).toEqual([]);
    expect(matchAction(sup.bindings, key("Digit2", { metaKey: true }))).toBe("tab.2");
    expect(matchAction(sup.bindings, key("Digit2", { altKey: true }))).toBeNull();
    expect(matchAction(sup.bindings, key("ArrowLeft", { metaKey: true, shiftKey: true }))).toBe("move.left");
    expect(matchAction(sup.bindings, key("KeyM", { metaKey: true }))).toBe("mode.toggle");
    // the ones that never used Alt stay where they were
    expect(matchAction(sup.bindings, key("KeyN", { ctrlKey: true }))).toBe("tab.new");
    expect(matchAction(sup.bindings, key("F2"))).toBe("tab.rename");
    const both = buildBindings({ modifier: "Ctrl+Alt" });
    expect(matchAction(both.bindings, key("KeyV", { ctrlKey: true, altKey: true }))).toBe("split.right");
    // a shortcut written by hand is kept as written, and "Mod" in it follows the setting
    const mixed = buildBindings({ modifier: "Super", "mode.toggle": "Alt+Shift+X", "split.down": "Mod+J" });
    expect(matchAction(mixed.bindings, key("KeyX", { altKey: true, shiftKey: true }))).toBe("mode.toggle");
    expect(matchAction(mixed.bindings, key("KeyJ", { metaKey: true }))).toBe("split.down");
    expect(comboText({ modifier: "Super" }, "tab.3")).toBe("Super+3");
    expect(comboText(null, "tab.3")).toBe("Alt+3");
  });

  it("a wrong main key is reported and Alt is used", () => {
    const r = buildBindings({ modifier: "Hyper" });
    expect(r.problems.some((p) => p.includes("modifier"))).toBe(true);
    expect(matchAction(r.bindings, key("Digit1", { altKey: true }))).toBe("tab.1");
  });

  it("parses the odd keys", () => {
    expect(parseCombo("Ctrl+,")?.code).toBe("Comma");
    expect(parseCombo("Alt+Shift+Left")?.code).toBe("ArrowLeft");
    expect(parseCombo("F2")?.code).toBe("F2");
    expect(parseCombo("Hyper+X")).toBeNull();
    expect(Object.keys(DEFAULTS).length).toBeGreaterThan(20);
  });
});
