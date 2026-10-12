// Sounds for what needs your attention: an agent that finished, one that needs you, a watched terminal that is done.
// The rules (when it may sound) are plain functions so that they are tested; the player only plays. The sound that comes
// with oso is a file of the app (Kenney, CC0); the others are read by the engine (a sound of the desktop's theme, or a
// file the person chose) and played from here, by the audio of the window: there is no audio library.

import { Backend } from "./api";
import bundled from "../../assets/sounds/pepSound2.ogg";

export type SoundEvent = "finished" | "needs";
export type SoundSource = { kind: "builtin" } | { kind: "system" } | { kind: "file"; path: string };

export interface SoundSettings {
  enabled: boolean;
  /** 0..1 */
  volume: number;
  finished: SoundSource;
  needs: SoundSource;
}

export const DEFAULT_SOUNDS: SoundSettings = { enabled: true, volume: 0.7, finished: { kind: "builtin" }, needs: { kind: "builtin" } };

/** One agent (or watched terminal) does not sound more often than this. */
export const MIN_GAP_MS = 5000;

const KEY = "oso.sound";

const sourceOf = (v: unknown, fallback: SoundSource): SoundSource => {
  const o = v as { kind?: string; path?: unknown } | null;
  if (o?.kind === "system") return { kind: "system" };
  if (o?.kind === "file" && typeof o.path === "string" && o.path.trim()) return { kind: "file", path: o.path.trim() };
  if (o?.kind === "builtin") return { kind: "builtin" };
  return fallback;
};

/** Reads what was saved, defensively (a file edited by hand, or nothing). */
export function parseSettings(text: string | null): SoundSettings {
  let raw: Record<string, unknown> = {};
  try {
    raw = text ? (JSON.parse(text) as Record<string, unknown>) : {};
  } catch {
    raw = {};
  }
  const v = typeof raw.volume === "number" && Number.isFinite(raw.volume) ? Math.min(1, Math.max(0, raw.volume)) : DEFAULT_SOUNDS.volume;
  return { enabled: raw.enabled !== false, volume: v, finished: sourceOf(raw.finished, DEFAULT_SOUNDS.finished), needs: sourceOf(raw.needs, DEFAULT_SOUNDS.needs) };
}

export function loadSettings(): SoundSettings {
  try {
    return parseSettings(localStorage.getItem(KEY));
  } catch {
    return parseSettings(null);
  }
}

export function saveSettings(s: SoundSettings) {
  try {
    localStorage.setItem(KEY, JSON.stringify(s));
  } catch {
    /* it just will not be remembered */
  }
}

/** Should it sound? Not when sounds are off, not when you are looking at that very tab, and not again within the gap. */
export function shouldPlay(i: { enabled: boolean; lookingAtIt: boolean; last: number | undefined; now: number; gap?: number }): boolean {
  if (!i.enabled || i.lookingAtIt) return false;
  return i.last === undefined || i.now - i.last >= (i.gap ?? MIN_GAP_MS);
}

export interface Played {
  event: SoundEvent;
  sid: number | null;
  source: SoundSource["kind"];
  volume: number;
}

/** Plays the sounds, and remembers what it played (the tests read that). */
export class SoundPlayer {
  settings: SoundSettings = loadSettings();
  played: Played[] = [];
  private last = new Map<number, number>();
  private cache = new Map<string, string>();

  constructor(private be: Backend) {}

  /** Plays an event with the settings of a form that is not saved yet. */
  testWith(event: SoundEvent, settings: SoundSettings) {
    return this.play(event, null, { ...settings, enabled: true });
  }

  /** Forgets when each agent last sounded (the tests use it between one thing and the next). */
  forget() {
    this.last.clear();
  }

  update(s: SoundSettings) {
    this.settings = s;
    saveSettings(s);
  }

  /** An agent or a terminal calls for you. `lookingAtIt`: the window is in front and that tab is on show. */
  async notify(event: SoundEvent, sid: number, lookingAtIt: boolean, now = Date.now()) {
    if (!shouldPlay({ enabled: this.settings.enabled, lookingAtIt, last: this.last.get(sid), now })) return;
    this.last.set(sid, now);
    await this.play(event, sid);
  }

  /** The "Test" button: always sounds, whatever the rules say. */
  test(event: SoundEvent) {
    return this.play(event, null, { ...this.settings, enabled: true });
  }

  private async play(event: SoundEvent, sid: number | null, settings = this.settings) {
    const source = settings[event];
    this.played.push({ event, sid, source: source.kind, volume: settings.volume });
    try {
      const url = await this.urlFor(event, source);
      const audio = new Audio(url);
      audio.volume = settings.volume;
      await audio.play();
    } catch {
      /* no sound is not worth an error: the desktop may have none, or the file may be gone */
    }
  }

  private async urlFor(event: SoundEvent, source: SoundSource): Promise<string> {
    if (source.kind === "builtin") return bundled;
    const key = source.kind === "system" ? `system:${event}` : `file:${source.path}`;
    const hit = this.cache.get(key);
    if (hit) return hit;
    const data = source.kind === "system" ? await this.be.soundLoad("system", event === "finished" ? "complete" : "message") : await this.be.soundLoad("file", source.path);
    const url = `data:${data.mime};base64,${data.b64}`;
    this.cache.set(key, url);
    return url;
  }
}
