// "Connect your AI": for each agent, whether oso is registered with it, and a button that shows exactly what would
// change in its files before anything is written.

import { Backend } from "./api";
import { el, openModal, toast } from "./dom";

const AGENTS: { id: "claude" | "opencode" | "codex"; name: string }[] = [
  { id: "claude", name: "Claude Code" },
  { id: "opencode", name: "OpenCode" },
  { id: "codex", name: "Codex" },
];

export async function connectAiDialog(be: Backend, changed: () => void) {
  const status = await be.integrationStatus().catch(() => ({ claude: false, opencode: false, codex: false }));
  openModal((close) => {
    const rows = AGENTS.map((a) => {
      const on = status[a.id];
      const b = el("button", { class: `btn${on ? "" : " primary"} small`, type: "button", "data-agent": a.id }, on ? "Remove…" : "Connect…");
      b.onclick = () => {
        close();
        void preview(be, a.id, a.name, !on, changed);
      };
      return el("div", { class: "step", "data-agent-row": a.id }, el("span", { class: `mark${on ? "" : " off"}` }, on ? "✓" : "–"), el("div", { class: "txt" }, el("b", {}, a.name), el("small", {}, on ? "Connected: it can use oso's tools." : "Not connected.")), b);
    });
    const done = el("button", { class: "btn primary", type: "button" }, "Done");
    done.onclick = close;
    return el("div", {}, el("h2", {}, "Connect your AI"), el("div", { class: "hint", style: "margin:0 0 10px" }, "oso registers itself as a tool server in the agent's settings and adds one short note to its global instructions. Nothing is written before you see it, and a copy of each old file is kept. Open a new session of the agent afterwards."), ...rows, el("div", { class: "actions" }, done));
  });
}

async function preview(be: Backend, agent: "claude" | "opencode" | "codex", name: string, install: boolean, changed: () => void) {
  let plan;
  try {
    plan = await be.integrationPlan(agent, install);
  } catch (e) {
    toast((e as Error).message, 8000);
    return void connectAiDialog(be, changed);
  }
  openModal((close) => {
    const body = el("div", { class: "plan" });
    for (const f of plan) body.append(el("h3", {}, f.path), f.changed ? el("pre", { class: "cmd" }, f.diff) : el("div", { class: "hint" }, "Nothing to change here."));
    const go = el("button", { class: "btn primary", type: "button", "data-act": "integration-apply" }, install ? `Connect ${name}` : `Remove from ${name}`);
    const no = el("button", { class: "btn", type: "button" }, "Cancel");
    no.onclick = () => {
      close();
      void connectAiDialog(be, changed);
    };
    go.onclick = async () => {
      try {
        await be.integrationApply(agent, install);
        close();
        toast(install ? `${name} is connected. Start a new session of it inside an oso terminal.` : `${name} is no longer connected.`, 7000);
        changed();
        void connectAiDialog(be, changed);
      } catch (e) {
        toast((e as Error).message, 8000);
      }
    };
    return el("div", { class: "wide" }, el("h2", {}, install ? `Connect ${name}` : `Remove oso from ${name}`), el("div", { class: "hint", style: "margin:0 0 10px" }, "This is what changes (+ added, − removed):"), body, el("div", { class: "actions" }, no, go));
  });
}
