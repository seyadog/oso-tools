// When a connection drops (the wifi went away, the laptop slept) oso tries again by itself. These are the rules,
// kept apart from the window so they can be tested.

/** How many times it tries before leaving the terminal for you to decide. */
export const MAX_TRIES = 8;
/** A connection that lasted this long was a real one: if it drops, it is a new story, not a failed attempt. */
export const STABLE_MS = 10000;
/** ssh ends with 255 when it cannot connect or the connection is lost (any other code is the remote program's own). */
const SSH_FAILED = 255;

/** Seconds to wait before attempt number `attempt` (0 is the first). */
export function retryDelay(attempt: number): number {
  return [2, 4, 8, 15, 30, 30, 60, 60][Math.min(attempt, MAX_TRIES - 1)];
}

/**
 * Whether a session that ended should be started again by itself.
 * `uptimeMs`: how long it had been up. `attempt`: how many automatic tries in a row already failed.
 * A first-time failure that came right after connecting (wrong password, a typo in the host) is not retried: it
 * would only fail again and again.
 */
export function worthRetrying(code: number, uptimeMs: number, attempt: number, isLocal: boolean): { retry: boolean; attempt: number } {
  if (isLocal || code !== SSH_FAILED) return { retry: false, attempt: 0 };
  if (uptimeMs >= STABLE_MS) return { retry: true, attempt: 0 };
  if (attempt === 0) return { retry: false, attempt: 0 };
  return { retry: attempt < MAX_TRIES, attempt };
}

/**
 * Something on the screen that trying again cannot fix, and that repeated tries could make worse (a wrong
 * password counts towards a lockout). `null` when it looks like the network.
 */
export function fatalReason(screen: string): string | null {
  const t = screen.toLowerCase();
  if (t.includes("permission denied") || t.includes("too many authentication failures") || t.includes("authentication failed")) return "The server did not accept the login.";
  if (t.includes("remote host identification has changed") || t.includes("host key verification failed")) return "The server's key does not match the one saved.";
  return null;
}
