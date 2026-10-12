import { describe, expect, it } from "vitest";
import { Backend } from "../src/api";
import { DEFAULT_SOUNDS, MIN_GAP_MS, SoundPlayer, parseSettings, shouldPlay } from "../src/sound";

describe("when a sound may play", () => {
  const base = { enabled: true, lookingAtIt: false, last: undefined as number | undefined, now: 100_000 };
  it("plays for what calls for you", () => expect(shouldPlay(base)).toBe(true));
  it("is quiet when sounds are off", () => expect(shouldPlay({ ...base, enabled: false })).toBe(false));
  it("is quiet when you are looking at that tab", () => expect(shouldPlay({ ...base, lookingAtIt: true })).toBe(false));
  it("does not repeat within a few seconds, and does after them", () => {
    expect(shouldPlay({ ...base, last: base.now - 1000 })).toBe(false);
    expect(shouldPlay({ ...base, last: base.now - (MIN_GAP_MS - 1) })).toBe(false);
    expect(shouldPlay({ ...base, last: base.now - MIN_GAP_MS })).toBe(true);
  });
});

describe("the saved settings", () => {
  it("are the defaults when there is nothing, and survive a broken file", () => {
    expect(parseSettings(null)).toEqual(DEFAULT_SOUNDS);
    expect(parseSettings("not json")).toEqual(DEFAULT_SOUNDS);
    expect(parseSettings("[1]")).toEqual(DEFAULT_SOUNDS);
  });
  it("keep what is valid and fix what is not", () => {
    const s = parseSettings(JSON.stringify({ enabled: false, volume: 3, finished: { kind: "file", path: " ~/a.ogg " }, needs: { kind: "nonsense" } }));
    expect(s).toEqual({ enabled: false, volume: 1, finished: { kind: "file", path: "~/a.ogg" }, needs: { kind: "builtin" } });
    expect(parseSettings(JSON.stringify({ volume: -1, finished: { kind: "file", path: "  " }, needs: { kind: "system" } }))).toEqual({ enabled: true, volume: 0, finished: { kind: "builtin" }, needs: { kind: "system" } });
  });
});

describe("the player", () => {
  const be = { soundLoad: async (kind: string, value: string) => ({ mime: "audio/ogg", b64: `${kind}-${value}` }) } as unknown as Backend;
  it("plays when something calls for you, once per agent in a burst, and never while you look", async () => {
    const p = new SoundPlayer(be);
    await p.notify("finished", 7, false, 1000);
    await p.notify("needs", 7, false, 2000); // the same agent, a second later: nothing
    await p.notify("needs", 8, false, 2000); // another agent: sounds
    await p.notify("finished", 9, true, 3000); // you are looking at its tab: nothing
    await p.notify("finished", 7, false, 1000 + MIN_GAP_MS); // later: sounds again
    expect(p.played.map((x) => [x.event, x.sid])).toEqual([["finished", 7], ["needs", 8], ["finished", 7]]);
  });
  it("is silent when switched off, and the Test button always sounds", async () => {
    const p = new SoundPlayer(be);
    p.update({ ...DEFAULT_SOUNDS, enabled: false });
    await p.notify("finished", 1, false, 1000);
    expect(p.played).toEqual([]);
    await p.test("needs");
    expect(p.played).toHaveLength(1);
    expect(p.played[0]).toMatchObject({ event: "needs", sid: null, source: "builtin", volume: DEFAULT_SOUNDS.volume });
    localStorage.removeItem("oso.sound");
  });
  it("uses the source chosen for each event", async () => {
    const p = new SoundPlayer(be);
    p.update({ enabled: true, volume: 0.3, finished: { kind: "system" }, needs: { kind: "file", path: "~/x.ogg" } });
    await p.notify("finished", 1, false, 1000);
    await p.notify("needs", 2, false, 1000);
    expect(p.played.map((x) => [x.event, x.source, x.volume])).toEqual([["finished", "system", 0.3], ["needs", "file", 0.3]]);
    localStorage.removeItem("oso.sound");
  });
});
