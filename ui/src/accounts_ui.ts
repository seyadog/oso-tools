// The Accounts section of the sidebar: who is logged in to GitHub and Tailscale, with a button to use another.
// oso only switches them: the tools keep their own tokens.

import { AccountsView, Backend, ToolAccounts } from "./api";
import { el, toast } from "./dom";
import { icon } from "./icons";

/** The choices of a drop-down: leave it alone, or one of the accounts (and the saved one even if the tool no longer lists it). */
export function accountChoices(tool: ToolAccounts | undefined, saved: string | null): [string, string][] {
  const out: [string, string][] = [["", "Leave it alone"]];
  for (const a of tool?.accounts ?? []) out.push([a.id, `${a.name}${a.detail ? ` · ${a.detail}` : ""}`]);
  if (saved && !out.some(([v]) => v === saved)) out.push([saved, saved]);
  return out;
}

const TITLE = { github: "GitHub", tailscale: "Tailscale" } as const;

/** Adds the section to `parent` (nothing, when neither tool is installed). */
export function accountsSection(parent: HTMLElement, view: AccountsView, be: Backend, changed: () => Promise<void>) {
  const tools = (["github", "tailscale"] as const).filter((t) => view[t].installed);
  if (!tools.length) return;
  const open = localStorage.getItem("oso.accounts") !== "closed";
  const head = el("div", { class: "sec", title: "The accounts of other tools: oso only switches them" });
  const chev = el("span", { class: "chev" });
  chev.append(icon(open ? "chevron-down" : "chevron-right", 13));
  head.append(chev, "Accounts");
  head.onclick = () => {
    localStorage.setItem("oso.accounts", open ? "closed" : "open");
    void changed();
  };
  parent.append(head);
  if (!open) return;
  for (const t of tools) {
    const v = view[t];
    const now = v.accounts.find((a) => a.active);
    parent.append(el("div", { class: "row acct-head", "data-tool": t }, el("span", { class: "txt" }, el("b", {}, TITLE[t]), el("small", {}, now ? `in use: ${now.name}` : "no account in use"))));
    for (const a of v.accounts.filter((x) => !x.active)) {
      const use = el("button", { class: "btn small", type: "button", "data-account": a.id }, "Use");
      use.onclick = async () => {
        try {
          await be.accountUse(t, a.id);
          toast(`${TITLE[t]}: now ${a.name}`);
        } catch (e) {
          toast((e as Error).message, 6000);
        }
        await changed();
      };
      parent.append(el("div", { class: "row acct" }, el("span", { class: "txt" }, el("b", {}, a.name), el("small", {}, a.detail)), use));
    }
    if (v.error) parent.append(el("div", { class: "hint-row" }, v.error));
  }
}
