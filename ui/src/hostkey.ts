// ssh refuses to connect when a server shows a different key than the one saved. Its message is a wall of capital
// letters; this finds it in what a terminal printed so the window can explain it and offer the fix.

export interface KeyWarning {
  /** The name ssh has the old key under (`web1.example.com`, `[web1.example.com]:2222`), if it said. */
  host: string | null;
  /** The key the server shows now, as ssh printed it (`SHA256:…`). */
  fingerprint: string | null;
}

export function parseHostKeyWarning(text: string): KeyWarning | null {
  if (!/REMOTE HOST IDENTIFICATION HAS CHANGED/i.test(text)) return null;
  // the last warning on screen is the one that matters
  const at = text.toUpperCase().lastIndexOf("REMOTE HOST IDENTIFICATION HAS CHANGED");
  const t = text.slice(at);
  const host = /Host key for (\S+) has changed/.exec(t)?.[1] ?? null;
  const fingerprint = /remote host is\s+(SHA256:\S+|MD5:\S+)/.exec(t)?.[1] ?? null;
  return { host, fingerprint };
}
