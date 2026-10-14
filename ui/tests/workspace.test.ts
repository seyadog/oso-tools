import { beforeEach, describe, expect, it } from "vitest";
import { Tree, leaf, leaves } from "../src/tree";
import { Workspace } from "../src/workspace";

let ws: Workspace;
const tab = (sid: number) => ({ sid, title: `t${sid}`, serverId: 0 });
const tabsOf = () => ws.screen.groups.map((g) => leaves(g.tree));
/** Opens terminals 1..n as tabs, then joins all of them in one mosaic. */
const mosaic = (n: number) => {
  for (let i = 1; i <= n; i++) ws.addTab(tab(i));
  ws.toggleMode();
};

beforeEach(() => {
  ws = new Workspace();
  ws.size = { x: 0, y: 0, w: 1600, h: 900 };
});

describe("moving a tab", () => {
  it("changes its place and keeps the tab on show on show", () => {
    for (let i = 1; i <= 4; i++) ws.addTab(tab(i));
    ws.focus(2);
    ws.moveGroup(0, 3);
    expect(tabsOf()).toEqual([[2], [3], [4], [1]]);
    expect(ws.screen.active).toBe(2);
    ws.moveGroup(3, 0);
    expect(tabsOf()).toEqual([[1], [2], [3], [4]]);
    ws.moveGroup(1, 1);
    ws.moveGroup(9, 0);
    expect(tabsOf()).toEqual([[1], [2], [3], [4]]);
  });
});

describe("tabs", () => {
  it("every new terminal is a tab of its own", () => {
    ws.addTab(tab(1));
    ws.addTab(tab(2));
    expect(tabsOf()).toEqual([[1], [2]]);
    expect(ws.screen.cur).toBe(1);
    expect(ws.screen.mode).toBe("tabs");
  });

  it("closing a tab shows the one before it; the last one leaves an empty screen", () => {
    [1, 2, 3].forEach((n) => ws.addTab(tab(n)));
    ws.closeTab(3);
    expect(ws.screen.active).toBe(2);
    ws.closeTab(1);
    expect(ws.screen.active).toBe(2);
    ws.closeTab(2);
    expect(ws.screen.active).toBeNull();
    expect(ws.screen.groups).toEqual([]);
  });

  it("steps through the tabs and wraps", () => {
    [1, 2, 3].forEach((n) => ws.addTab(tab(n)));
    ws.step(1);
    expect(ws.screen.active).toBe(1);
    ws.step(-1);
    expect(ws.screen.active).toBe(3);
  });
});

describe("going to a tab, like a browser", () => {
  it("Alt+N goes to tab N; the last number goes to the last tab", () => {
    for (let i = 1; i <= 4; i++) ws.addTab(tab(i));
    ws.goTab(2);
    expect(ws.screen.active).toBe(2);
    ws.goTab(9);
    expect(ws.screen.active).toBe(4);
    ws.goTab(1);
    expect(ws.screen.active).toBe(1);
  });

  it("a number with no tab does nothing: there is no tab 3 without tab 2", () => {
    ws.addTab(tab(1));
    ws.addTab(tab(2));
    ws.goTab(1);
    ws.goTab(3);
    expect(ws.screen.active).toBe(1);
    expect(ws.screen.groups.length).toBe(2);
  });

  it("on an empty window it does nothing", () => {
    ws.goTab(2);
    expect(ws.screen.active).toBeNull();
  });
});

describe("the mosaic", () => {
  it("Alt+M joins every tab in one, and again sets them apart", () => {
    [1, 2, 3].forEach((n) => ws.addTab(tab(n)));
    ws.toggleMode();
    expect(tabsOf()).toEqual([[1, 2, 3]]);
    expect(ws.screen.mode).toBe("mosaic");
    ws.toggleMode();
    expect(tabsOf()).toEqual([[1], [2], [3]]);
    expect(ws.screen.active).toBe(3);
  });

  it("a new terminal after Alt+V / Alt+H joins the tab on show, the way it was asked for", () => {
    ws.addTab(tab(1));
    ws.prepareSplit("bottom");
    ws.addTab(tab(2));
    expect(tabsOf()).toEqual([[1, 2]]);
    const b = ws.boxes().boxes;
    expect(b.find((x) => x.id === 2)!.rect.y).toBeGreaterThan(b.find((x) => x.id === 1)!.rect.y);
    ws.prepareSplit("right");
    ws.addTab(tab(3));
    const c = ws.boxes().boxes;
    expect(c.find((x) => x.id === 3)!.rect.x).toBeGreaterThan(c.find((x) => x.id === 2)!.rect.x);
    expect(ws.pending).toBeNull();
  });

  it("without a split asked for, a new terminal is a new tab even beside a mosaic", () => {
    mosaic(2);
    ws.addTab(tab(3));
    expect(tabsOf()).toEqual([[1, 2], [3]]);
  });

  it("closing a terminal gives its room to its neighbour; a mosaic left with one is a plain tab", () => {
    mosaic(3);
    ws.closeTab(3);
    expect(leaves(ws.screen.tree!)).toEqual([1, 2]);
    const area = ws.boxes().boxes.reduce((a, b) => a + b.rect.w * b.rect.h, 0);
    expect(area).toBeCloseTo(1600 * 900);
    ws.closeTab(2);
    expect(ws.screen.mode).toBe("tabs");
    expect(tabsOf()).toEqual([[1]]);
  });

  it("Alt+arrows move the keyboard and Alt+Shift+arrows move the terminal", () => {
    ws.addTab(tab(1));
    ws.prepareSplit("right");
    ws.addTab(tab(2)); // 1 | 2, 2 active
    expect(ws.moveFocus(-1, 0)).toBe(true);
    expect(ws.screen.active).toBe(1);
    expect(ws.moveFocus(-1, 0)).toBe(false); // nothing there
    expect(ws.screen.active).toBe(1);
    expect(ws.moveActive(1, 0)).toBe(true);
    expect(leaves(ws.screen.tree!)).toEqual([2, 1]);
    expect(ws.moveActive(1, 0)).toBe(false);
  });

  it("dropping near an edge docks, in the middle it trades places", () => {
    mosaic(3);
    ws.dropOn(3, 1, "left");
    const b = ws.boxes().boxes;
    expect(b.find((x) => x.id === 3)!.rect.x).toBeLessThan(b.find((x) => x.id === 1)!.rect.x);
    expect(leaves(ws.screen.tree!).sort()).toEqual([1, 2, 3]);
    ws.dropOn(1, 2, null);
    expect(ws.screen.active).toBe(1);
  });

  it("a tab dragged from the bar onto a terminal joins its mosaic", () => {
    ws.addTab(tab(1));
    ws.addTab(tab(2));
    ws.focus(1);
    ws.dropGroup(2, 1, "right");
    expect(tabsOf()).toEqual([[1, 2]]);
    expect(ws.screen.active).toBe(2);
    const b = ws.boxes().boxes;
    expect(b.find((x) => x.id === 2)!.rect.x).toBeGreaterThan(b.find((x) => x.id === 1)!.rect.x);
  });

  it("a whole mosaic tab dragged onto another brings all its terminals", () => {
    mosaic(2); // [1,2]
    ws.addTab(tab(3));
    ws.addTab(tab(4));
    ws.toggleMode(); // everything in one…
    ws.toggleMode(); // …and apart again: [1] [2] [3] [4]
    ws.dropGroup(2, 1, "right");
    ws.dropGroup(4, 3, "right");
    expect(tabsOf()).toEqual([[1, 2], [3, 4]]);
    ws.focus(1);
    ws.dropGroup(3, 1, "bottom");
    expect(tabsOf().map((g) => g.slice().sort())).toEqual([[1, 2, 3, 4]]);
  });

  it("a terminal of another tab dropped on a mosaic joins it", () => {
    mosaic(2);
    ws.addTab(tab(3));
    ws.focus(1);
    ws.dropOn(3, 1, "bottom");
    expect(tabsOf().map((g) => g.slice().sort())).toEqual([[1, 2, 3]]);
  });

  it("a terminal dragged up to the tab bar becomes a tab of its own", () => {
    mosaic(3);
    ws.dropIntoBar(2);
    expect(tabsOf().map((g) => g.slice().sort())).toEqual([[1, 3], [2]]);
    expect(ws.screen.active).toBe(2);
    ws.dropIntoBar(2); // alone already: nothing to do
    expect(tabsOf().length).toBe(2);
  });

  it("dragging the line resizes, within limits", () => {
    mosaic(2);
    ws.resize([], 300);
    expect(ws.boxes().boxes[0].rect.w).toBe(480);
    ws.resize([], 5);
    expect(ws.boxes().boxes[0].rect.w).toBe(160);
  });

  it("a host dropped beside a terminal opens in that tab, on that side", () => {
    ws.addTab(tab(1));
    ws.addTab(tab(2), { target: 1, side: "left" });
    expect(tabsOf()).toEqual([[2, 1]]);
    ws.addTab(tab(3), { target: 1, side: "bottom" });
    const b = ws.boxes().boxes;
    expect(b.find((x) => x.id === 3)!.rect.y).toBeGreaterThan(b.find((x) => x.id === 1)!.rect.y);
    expect(leaves(ws.screen.tree!).sort()).toEqual([1, 2, 3]);
  });
});

describe("saving", () => {
  it("restores the layout over the sessions that are still alive and adds the ones it did not know", () => {
    mosaic(3);
    ws.addTab(tab(6));
    ws.goTab(1);
    const saved = JSON.parse(JSON.stringify(ws.save()));
    const again = new Workspace();
    again.size = ws.size;
    // 2 is gone, 5 is new
    again.restore(saved, [
      { sid: 1, title: "a", serverId: 0, screen: 0 },
      { sid: 3, title: "c", serverId: 0, screen: 0 },
      { sid: 5, title: "e", serverId: 0, screen: 0 },
      { sid: 6, title: "f", serverId: 0, screen: 0 },
    ]);
    expect(again.screens[0].groups.map((g) => leaves(g.tree).slice().sort())).toEqual([[1, 3], [6], [5]]);
    expect(again.screen.cur).toBe(0);
  });

  it("an older layout with several screens becomes one row of tabs, in order", () => {
    const leafNode = (id: number) => ({ kind: "leaf", id });
    const old = {
      current: 1,
      screens: [
        { mode: "mosaic", order: [1, 2], tree: { kind: "split", side: "right", pm: 500, a: leafNode(1), b: leafNode(2) }, active: 2 },
        { mode: "tabs", order: [3, 4], tree: null, active: 4 },
        { mode: "tabs", order: [], tree: null, active: null },
      ],
    };
    ws.restore(old as never, [1, 2, 3, 4].map((sid) => ({ sid, title: "x", serverId: 0, screen: sid > 2 ? 1 : 0 })));
    expect(ws.screens[0].groups.map((g) => leaves(g.tree))).toEqual([[1, 2], [3], [4]]);
    expect(ws.screen.active).toBe(4);
  });

  it("restores with nothing saved: every session its own tab", () => {
    ws.restore(null, [
      { sid: 1, title: "a", serverId: 0, screen: 1 },
      { sid: 2, title: "b", serverId: 0, screen: 0 },
    ]);
    expect(ws.screen.groups.map((g) => leaves(g.tree))).toEqual([[2], [1]]);
    expect(ws.screen.mode).toBe("tabs");
  });
});

describe("groups and per-tab broadcast", () => {
  const mk = (sid: number) => ({ sid, title: `t${sid}`, serverId: sid });
  const split = (a: number, b: number): Tree => ({ kind: "split", side: true, pm: 500, a: leaf(a), b: leaf(b) });

  it("opens a saved shape as one tab, with its terminals where the shape says", () => {
    const ws = new Workspace();
    ws.addGroup([mk(11), mk(12), mk(13)], { kind: "split", side: true, pm: 400, a: leaf(0), b: { kind: "split", side: false, pm: 500, a: leaf(1), b: leaf(2) } }, true);
    expect(ws.screen.groups.length).toBe(1);
    expect(leaves(ws.screen.group!.tree)).toEqual([11, 12, 13]);
    expect(ws.broadcasting).toBe(true);
    expect(ws.screen.mode).toBe("mosaic");
  });

  it("keeps the shape through shape() and addGroup", () => {
    const ws = new Workspace();
    ws.addGroup([mk(5), mk(6)], split(1, 0));
    const { shape, sids } = ws.shape()!;
    expect(sids).toEqual([6, 5]);
    expect(leaves(shape)).toEqual([0, 1]);
    const other = new Workspace();
    other.addGroup([mk(21), mk(22)], shape);
    expect(leaves(other.screen.group!.tree)).toEqual([21, 22]);
  });

  it("falls back to an automatic layout when the shape does not fit", () => {
    const ws = new Workspace();
    ws.addGroup([mk(1), mk(2), mk(3)], split(0, 1));
    expect(new Set(leaves(ws.screen.group!.tree))).toEqual(new Set([1, 2, 3]));
  });

  it("broadcast belongs to one tab and needs two terminals", () => {
    const ws = new Workspace();
    ws.addTab(mk(1));
    ws.setBroadcast(true);
    expect(ws.broadcasting).toBe(false);
    ws.addGroup([mk(2), mk(3)], split(0, 1));
    ws.setBroadcast(true);
    expect(ws.broadcastTeam(2)).toEqual([2, 3]);
    expect(ws.broadcastTeam(1)).toBeNull();
    ws.goTab(1);
    expect(ws.broadcasting).toBe(false);
    ws.goTab(2);
    expect(ws.broadcasting).toBe(true);
  });

  it("broadcast ends when the tab is left with one terminal, and is saved and restored", () => {
    const ws = new Workspace();
    ws.addGroup([mk(2), mk(3)], split(0, 1), true);
    const saved = JSON.parse(JSON.stringify(ws.save()));
    const again = new Workspace();
    again.restore(saved, [2, 3].map((sid) => ({ ...mk(sid), screen: 0 })));
    expect(again.broadcasting).toBe(true);
    again.closeTab(3);
    expect(again.broadcasting).toBe(false);
    expect(again.screen.group!.broadcast).toBe(false);
  });
});
