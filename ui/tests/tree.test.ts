import { describe, expect, it } from "vitest";
import { autoSide, contains, dock, dockSide, dockZone, dwindle, layout, leaf, leaves, neighbour, remove, replace, setPm, swap } from "../src/tree";

const AREA = { x: 0, y: 0, w: 1600, h: 900 };
const sum = (boxes: { rect: { w: number; h: number } }[]) => boxes.reduce((a, b) => a + b.rect.w * b.rect.h, 0);

describe("the tree of divisions", () => {
  it("docking splits a terminal on any side again and again, with no gaps", () => {
    let t = leaf(1);
    t = dock(t, 1, 2, "right");
    t = dock(t, 2, 3, "bottom");
    t = dock(t, 1, 4, "top");
    t = dock(t, 3, 5, "left");
    expect(leaves(t).sort()).toEqual([1, 2, 3, 4, 5]);
    const { boxes } = layout(t, AREA);
    expect(sum(boxes)).toBeCloseTo(AREA.w * AREA.h);
    const at = (id: number) => boxes.find((b) => b.id === id)!.rect;
    expect(at(4).y).toBeLessThan(at(1).y);
    expect(at(2).x).toBeGreaterThan(at(1).x);
    expect(at(3).y).toBeGreaterThan(at(2).y);
    expect(at(5).x).toBeLessThan(at(3).x);
  });

  it("removing gives the room to the neighbour, and the last one leaves nothing", () => {
    const t = dock(dock(leaf(1), 1, 2, "right"), 2, 3, "bottom");
    const r = remove(t, 2)!;
    expect(leaves(r)).toEqual([1, 3]);
    expect(sum(layout(r, AREA).boxes)).toBeCloseTo(AREA.w * AREA.h);
    expect(remove(leaf(1), 1)).toBeNull();
    expect(remove(t, 99)).toBe(t);
  });

  it("swap, replace and resize", () => {
    let t = dock(leaf(1), 1, 2, "right");
    t = swap(t, 1, 2);
    expect(leaves(t)).toEqual([2, 1]);
    t = replace(t, 2, 9);
    expect(leaves(t)).toEqual([9, 1]);
    t = setPm(t, [], 250);
    expect(layout(t, AREA).boxes[0].rect.w).toBe(400);
    expect(contains(t, 9)).toBe(true);
  });

  it("opens terminals by themselves like Hyprland and never makes one too small", () => {
    const t = dwindle([1, 2, 3, 4], AREA)!;
    const { boxes } = layout(t, AREA);
    const at = (id: number) => boxes.find((b) => b.id === id)!.rect;
    expect(at(2).x).toBeGreaterThan(at(1).x);
    expect(at(3).y).toBeGreaterThan(at(2).y);
    expect(boxes.every((b) => b.rect.w >= 320 && b.rect.h >= 180)).toBe(true);
    expect(autoSide({ x: 0, y: 0, w: 1000, h: 400 })).toBe("right");
    expect(autoSide({ x: 0, y: 0, w: 500, h: 800 })).toBe("bottom");
    expect(autoSide({ x: 0, y: 0, w: 500, h: 300 })).toBe("right"); // neither fits: the way the box is longer
    expect(dwindle([], AREA)).toBeNull();
  });

  it("the edge under the pointer says where to dock", () => {
    const r = { x: 100, y: 100, w: 400, h: 200 };
    expect(dockSide(r, 110, 200)).toBe("left");
    expect(dockSide(r, 490, 200)).toBe("right");
    expect(dockSide(r, 300, 105)).toBe("top");
    expect(dockSide(r, 300, 295)).toBe("bottom");
    expect(dockSide(r, 300, 200)).toBeNull();
    expect(dockZone(r, "left")).toEqual({ x: 100, y: 100, w: 200, h: 200 });
    expect(dockZone(r, null)).toEqual(r);
  });

  it("neighbours follow the real boxes", () => {
    let t = dock(leaf(1), 1, 2, "right");
    t = dock(t, 2, 3, "bottom"); // 1 on the left, 2 over 3 on the right
    const rects = layout(t, AREA).boxes.map((b) => b.rect);
    expect(leaves(t)).toEqual([1, 2, 3]);
    expect(neighbour(rects, 0, 1, 0)).toBe(1);
    expect(neighbour(rects, 2, -1, 0)).toBe(0);
    expect(neighbour(rects, 1, 0, 1)).toBe(2);
    expect(neighbour(rects, 0, -1, 0)).toBeNull();
  });
});
