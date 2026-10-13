// The agents of the window: what the engine reports (which agent runs in which terminal, and what each is doing), the
// last-resort guess from the screen, who needs you next, and when to tell the person from outside the window.

export type AgentState = "needs_you" | "working" | "finished" | "idle";

/** What the engine knows about one agent (see `agents.rs`). */
export interface AgentStatus {
  sid: number;
  kind: string;
  name: string;
  pid: number | null;
  cwd: string;
  folder: string;
  branch: string;
  state: AgentState;
  /** When it got into this state (ms). */
  since: number;
  /** How the engine knows: `approval` (a request waits in oso), `hook` (its own hooks), `process` (only that it runs). */
  source: "approval" | "hook" | "process";
}

export interface AgentKind {
  id: string;
  name: string;
  command: string;
}

/** What a screen can say when nothing exact is known: the last lines look like a question to you, or output is arriving. */
const ASKS = /\((?:y\/n|Y\/n|y\/N)\)|\[(?:y\/n|Y\/n|y\/N)\]|do you want to|would you like to|allow (?:this|once|always)|approve|press enter to|proceed\?|continue\?/i;

export function guessFromScreen(lastLines: string[], msSinceOutput: number): "needs_you" | "working" | null {
  const tail = lastLines.map((l) => l.trim()).filter(Boolean).slice(-6);
  if (tail.some((l) => ASKS.test(l))) return "needs_you";
  if (msSinceOutput >= 0 && msSinceOutput < 2000) return "working";
  return null;
}

/**
 * The state to show. The engine's exact signals are kept as they are; only when all it knows is that the program runs
 * (`process`, "idle") does the screen get a say. Returns `guessed` when it did.
 */
export function effectiveState(s: AgentStatus, guess: "needs_you" | "working" | null): { state: AgentState; guessed: boolean } {
  if (s.source === "process" && s.state === "idle" && guess) return { state: guess, guessed: true };
  return { state: s.state, guessed: false };
}

/** How long an agent that was producing output must be silent to count as done, when its own hooks are not there to say so. */
export const QUIET_MS = 6000;

/** Without hooks nothing says "finished": an agent that worked (output was arriving) and then stayed silent is done. */
export function doneByQuiet(workedAt: number | undefined, lastOutputAt: number, now: number, workedFor = Infinity): boolean {
  return workedAt !== undefined && lastOutputAt > 0 && now - lastOutputAt >= QUIET_MS && workedFor >= MIN_WORK_MS;
}

/** A blink of output (a spinner, a clock, a redraw) is not work: it must go on for this long before silence means "done". */
export const MIN_WORK_MS = 15000;

/** When did this run of work start? A silence longer than `QUIET_MS` between two sightings starts a new run. */
export function workStart(start: number | undefined, lastSeen: number | undefined, now: number): number {
  return start === undefined || lastSeen === undefined || now - lastSeen > QUIET_MS ? now : start;
}

/** An agent as the attention queue sees it. */
export interface Waiting {
  sid: number;
  state: AgentState;
  since: number;
}

/**
 * The next terminal to go to: those that need you, the one that has waited longest first; then, when nobody does, those
 * that finished, in the same order. From the one you are on (or the one you jumped to last) it goes to the next, and
 * after the last it starts again. `null` when there is nobody.
 */
export function nextAttention(agents: Waiting[], current: number | null): number | null {
  const order = (state: AgentState) => agents.filter((a) => a.state === state).sort((a, b) => a.since - b.since || a.sid - b.sid);
  const list = order("needs_you").length ? order("needs_you") : order("finished");
  if (!list.length) return null;
  const at = list.findIndex((a) => a.sid === current);
  return list[(at + 1) % list.length].sid;
}

export interface Notice {
  /** Which sound goes with it. */
  event: "needs" | "finished";
  sid: number;
  title: string;
  body: string;
}

/** What to say from outside the window: an agent that just started to need you, or just finished. Said once per change. */
export function noticesFor(before: Map<number, AgentState>, now: { sid: number; name: string; state: AgentState; where: string }[]): Notice[] {
  const out: Notice[] = [];
  for (const a of now) {
    const was = before.get(a.sid);
    if (was === undefined || was === a.state) continue; // the first time it is seen is the baseline, not news
    if (a.state === "needs_you") out.push({ event: "needs", sid: a.sid, title: `${a.name} needs you`, body: a.where });
    else if (a.state === "finished" && was === "working") out.push({ event: "finished", sid: a.sid, title: `${a.name} finished`, body: a.where });
  }
  return out;
}

/** The one dot a tab shows when several terminals of it have agents: the most urgent state. */
export function tabDot(states: AgentState[]): AgentState | null {
  for (const s of ["needs_you", "working", "finished", "idle"] as AgentState[]) if (states.includes(s)) return s;
  return null;
}

/** `oso/claude-241011-1430`: a branch name for a new worktree, to be edited. */
export function proposedBranch(agentId: string, when: Date): string {
  const p = (n: number) => String(n).padStart(2, "0");
  const stamp = `${p(when.getFullYear() % 100)}${p(when.getMonth() + 1)}${p(when.getDate())}-${p(when.getHours())}${p(when.getMinutes())}`;
  return `oso/${agentId.replace(/[^a-zA-Z0-9_-]/g, "") || "agent"}-${stamp}`;
}

/** The last part of a folder path. */
export function baseName(path: string): string {
  const t = path.replace(/\/+$/, "");
  return t.slice(t.lastIndexOf("/") + 1) || t;
}

/** An agent remembered between runs: after a reboot it stays in the list as a way back into its folder. */
export interface SavedAgent {
  kind: string;
  name: string;
  cwd: string;
  folder: string;
  branch: string;
  /** The Space it was in (null: General). */
  space: number | null;
  /** The last time it was seen running (ms). */
  last: number;
}

const savedKey = (s: { kind: string; cwd: string }) => `${s.kind}\n${s.cwd}`;

/** The remembered agents with what runs now written in (newest first). Returns the same list when nothing changed. */
export function rememberAgents(saved: SavedAgent[], live: Omit<SavedAgent, "last">[], now: number): SavedAgent[] {
  const map = new Map(saved.map((s) => [savedKey(s), s]));
  let changed = false;
  for (const l of live) {
    if (!l.cwd) continue;
    const old = map.get(savedKey(l));
    if (old && old.name === l.name && old.branch === l.branch && old.space === l.space && now - old.last < 60_000) continue;
    map.set(savedKey(l), { ...l, last: now });
    changed = true;
  }
  return changed ? [...map.values()].sort((a, b) => b.last - a.last).slice(0, 40) : saved;
}

/** The remembered agents that do not run right now. */
export function notRunning(saved: SavedAgent[], live: { kind: string; cwd: string }[]): SavedAgent[] {
  const on = new Set(live.map(savedKey));
  return saved.filter((s) => !on.has(savedKey(s)));
}

const SAVED_KEY = "oso.agents.saved";

export function loadSavedAgents(): SavedAgent[] {
  try {
    const v = JSON.parse(localStorage.getItem(SAVED_KEY) ?? "[]");
    return Array.isArray(v) ? v.filter((s) => s && typeof s.cwd === "string" && typeof s.kind === "string" && s.kind !== "term") : [];
  } catch {
    return [];
  }
}

export function storeSavedAgents(list: SavedAgent[]) {
  try {
    localStorage.setItem(SAVED_KEY, JSON.stringify(list));
  } catch {
    /* it just will not be remembered */
  }
}
