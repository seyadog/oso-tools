// "Getting started": the four things that make oso useful, each with a button and a mark when done. It shows on the
// empty screen until it is closed, and comes back from Help.

import { el } from "./dom";

export type StepId = "profile" | "key" | "terminal" | "ai";

export interface State {
  profiles: number;
  /** Secrets of the API-key type (null: the vault is locked or does not exist, so it is not known). */
  apiKeys: number | null;
  /** A local terminal was opened at least once. */
  openedLocal: boolean;
  /** Some AI agent has `oso mcp` registered. */
  aiConnected: boolean;
}

export interface Step {
  id: StepId;
  title: string;
  text: string;
  button: string;
  done: boolean;
}

export function steps(s: State): Step[] {
  return [
    { id: "profile", title: "Create a profile", text: "A profile keeps servers, secrets and accounts together (work, personal…). Switch it at the top of the left panel.", button: "New profile", done: s.profiles > 1 },
    { id: "key", title: "Add an API key", text: "Save a key once, in the encrypted vault, and give it an environment variable (like ANTHROPIC_API_KEY): every local terminal of the profile will have it.", button: "Add an API key", done: (s.apiKeys ?? 0) > 0 },
    { id: "terminal", title: "Open a local terminal", text: "It starts inside your profile, with its variables set. Start your AI tool there.", button: "Open a local terminal", done: s.openedLocal },
    { id: "ai", title: "Connect your AI", text: "Lets Claude Code, OpenCode or Codex ask oso for servers and secrets instead of being given passwords. You see every change before it is made.", button: "Connect your AI", done: s.aiConnected },
  ];
}

export function progress(s: State): { done: number; total: number } {
  const list = steps(s);
  return { done: list.filter((x) => x.done).length, total: list.length };
}

/** The card for the empty screen. `go` runs a step; `close` hides the card (Help brings it back). */
export function startCard(s: State, go: (id: StepId) => void, close: () => void): HTMLElement {
  const p = progress(s);
  const head = el("div", { class: "start-head" }, el("h2", {}, "Getting started"), el("span", { class: "start-n" }, `${p.done} of ${p.total} done`));
  const x = el("button", { class: "ib", type: "button", title: "Close (Help brings it back)", "aria-label": "Close getting started", "data-act": "start-close" }, "×");
  x.onclick = close;
  head.append(x);
  const card = el("section", { class: "start", "data-start": "card" }, head);
  steps(s).forEach((st, i) => {
    const btn = el("button", { class: `btn${st.done ? "" : " primary"} small`, type: "button", "data-step": st.id }, st.done ? "Open again" : st.button);
    btn.onclick = () => go(st.id);
    card.append(el("div", { class: `step${st.done ? " done" : ""}`, "data-step-row": st.id }, el("span", { class: "mark", "aria-label": st.done ? "done" : "to do" }, st.done ? "✓" : String(i + 1)), el("div", { class: "txt" }, el("b", {}, st.title), el("small", {}, st.text)), btn));
  });
  return card;
}
