// "Tell me when this terminal is done": you start a long command, switch the watch on, and go do something else.
// These are the rules for when it speaks, kept apart from the window so they can be tested.

import { looksReady } from "./groups";

/** Output stopped and the terminal shows a prompt again: the command ended. */
export const QUIET_AT_PROMPT_MS = 2500;
/** Output stopped and nothing looks like a prompt (a program waiting for you, or a very patient one). */
export const QUIET_ANY_MS = 20000;
/** Output right after a key press is the terminal echoing what you typed, not the command working. */
export const ECHO_MS = 400;

export interface Watch {
  /** Output has come since the watch was switched on (and not just as an echo of typing). */
  saw: boolean;
  lastOutput: number;
  lastInput: number;
}

export const startWatch = (now: number): Watch => ({ saw: false, lastOutput: now, lastInput: 0 });

export function noteOutput(w: Watch, now: number) {
  w.lastOutput = now;
  if (now - w.lastInput > ECHO_MS) w.saw = true;
}

export function noteInput(w: Watch, now: number) {
  w.lastInput = now;
}

/** What to say, if it is time: `prompt` (back at its prompt), `quiet` (silent for long), or nothing yet. */
export function verdict(w: Watch, now: number, cursorLine: string): "prompt" | "quiet" | null {
  if (!w.saw) return null;
  const quiet = now - w.lastOutput;
  if (quiet >= QUIET_AT_PROMPT_MS && looksReady(cursorLine)) return "prompt";
  if (quiet >= QUIET_ANY_MS) return "quiet";
  return null;
}
