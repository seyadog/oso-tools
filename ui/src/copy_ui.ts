// A small card under a server's address: one row per value, each with its own copy button (like Tailscale's machine list).
import { el } from "./dom";
import { icon } from "./icons";
import type { Server } from "./api";

export type CopyRow = [label: string, value: string];

/** What is worth copying about a server. `jumpName`: the server it is reached through, if any. */
export function copyRows(s: Pick<Server, "name" | "host" | "user" | "port">, jumpName?: string): CopyRow[] {
  const kind = /^\d{1,3}(\.\d{1,3}){3}$/.test(s.host) ? "IPv4" : s.host.includes(":") ? "IPv6" : "Address";
  const login = s.user ? `${s.user}@${s.host}` : s.host;
  const rows: CopyRow[] = [["Name", s.name], [kind, s.host]];
  if (s.user) rows.push(["User", s.user], ["User@address", login]);
  if (s.port !== 22) rows.push(["Port", String(s.port)]);
  rows.push(["ssh command", `ssh ${s.port !== 22 ? `-p ${s.port} ` : ""}${login}`]);
  if (jumpName) rows.push(["Reached via", jumpName]);
  return rows;
}

let current: HTMLElement | null = null;

export function closeCopyCard() {
  current?.remove();
  current = null;
}

export function showCopyCard(anchor: HTMLElement, rows: CopyRow[]) {
  closeCopyCard();
  const card = el("div", { class: "copycard", role: "dialog", "aria-label": "Copy server details" });
  for (const [label, value] of rows) {
    const b = el("button", { type: "button", class: "cc-copy", title: `Copy ${label.toLowerCase()}`, "aria-label": `Copy ${label}` });
    b.append(icon("copy", 15));
    b.onclick = (e) => {
      e.stopPropagation();
      void navigator.clipboard?.writeText(value).catch(() => {});
      b.replaceChildren("copied");
      window.setTimeout(() => b.replaceChildren(icon("copy", 15)), 1200);
    };
    card.append(el("div", { class: "cc-row" }, el("span", { class: "cc-text" }, el("small", {}, label), el("span", { class: "cc-val" }, value)), b));
  }
  document.body.append(card);
  const r = anchor.getBoundingClientRect();
  card.style.left = `${Math.max(8, Math.min(r.left, window.innerWidth - card.offsetWidth - 8))}px`;
  card.style.top = `${Math.min(r.bottom + 4, window.innerHeight - card.offsetHeight - 8)}px`;
  current = card;
  const off = (e: Event) => {
    if (e instanceof KeyboardEvent ? e.key !== "Escape" : card.contains(e.target as Node)) return;
    closeCopyCard();
    document.removeEventListener("pointerdown", off, true);
    document.removeEventListener("keydown", off, true);
  };
  window.setTimeout(() => {
    document.addEventListener("pointerdown", off, true);
    document.addEventListener("keydown", off, true);
  });
}
