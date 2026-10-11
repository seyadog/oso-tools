// Port forwards ("tunnels"): how they are described to a person, and checked before they are saved.

import { Forward } from "./api";

export type Kind = Forward["kind"];

export const KIND_LABEL: Record<Kind, string> = {
  local: "Open a port here that leads to something the server can reach",
  remote: "Open a port on the server that leads back to something here",
  dynamic: "SOCKS proxy here: browse as if you were the server",
};

/** One line: `localhost:5433 → db.internal:5432`. */
export function describe(f: Forward): string {
  if (f.kind === "dynamic") return `SOCKS proxy on localhost:${f.bind_port}`;
  if (f.kind === "local") return `localhost:${f.bind_port} → ${f.host}:${f.port}`;
  return `on the server :${f.bind_port} → ${f.host}:${f.port} here`;
}

/** What a tunnel is called in lists: its name, else the line that describes it. */
export function label(f: Forward): string {
  return f.name.trim() || describe(f);
}

export function isPort(text: string): boolean {
  if (!/^\d{1,5}$/.test(text.trim())) return false;
  const n = Number(text);
  return n >= 1 && n <= 65535;
}

/** What is wrong with a tunnel being typed in, or null. A port under 1024 on this computer needs privileges. */
export function problem(kind: Kind, bind: string, host: string, port: string): string | null {
  if (!isPort(bind)) return "The port to open must be a number from 1 to 65535";
  if (kind === "local" && Number(bind) < 1024) return "Ports under 1024 need administrator rights on this computer: use 1024 or more";
  if (kind === "dynamic") return null;
  const h = host.trim();
  if (!h) return "Write the host it leads to";
  if (/[\s:]/.test(h) || h.startsWith("-")) return "The host is a name or an IPv4 address (no spaces or colons)";
  if (!isPort(port)) return "The port it leads to must be a number from 1 to 65535";
  return null;
}

/** Same tunnel (ignoring its id): the window warns before saving a copy. */
export function sameTunnel(a: Forward, b: Forward): boolean {
  return a.kind === b.kind && a.bind_port === b.bind_port && a.host === b.host && a.port === b.port;
}
