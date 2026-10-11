// The Accounts section of the sidebar: who is logged in to GitHub and Tailscale, with a button to use another.
// oso only switches them: the tools keep their own tokens.

import { AccountsView, Backend, ToolAccounts } from "./api";
import { el, toast } from "./dom";

/** The choices of a drop-down: leave it alone, or one of the accounts (and the saved one even if the tool no longer lists it). */
export function accountChoices(tool: ToolAccounts | undefined, saved: string | null): [string, string][] {
  const out: [string, string][] = [["", "Leave it alone"]];
  for (const a of tool?.accounts ?? []) out.push([a.id, `${a.name}${a.detail ? ` · ${a.detail}` : ""}`]);
  if (saved && !out.some(([v]) => v === saved)) out.push([saved, saved]);
  return out;
}

const TITLE = { github: "GitHub", tailscale: "Tailscale" } as const;

/** The rows of the Accounts section (its title line is drawn by the window, like the other sections). */
export function accountsRows(parent: HTMLElement, view: AccountsView, be: Backend, changed: () => Promise<void>) {
  const tools = (["github", "tailscale"] as const).filter((t) => view[t].installed);
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
