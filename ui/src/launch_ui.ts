// "New agent": which folder, which agent, in a worktree of its own or not. And the list of agents (Settings).

import { AgentKindRow, Backend, SessionInfo, Space } from "./api";
import { baseName, proposedBranch } from "./agents";
import { clear, el, field, fieldHelp, makeSelect, openModal, toast } from "./dom";
import { icon } from "./icons";

export interface LaunchCtx {
  be: Backend;
  /** Opens a local terminal (in the Space, in that folder). `null` when it could not be (the person was told). */
  openLocal(space: Space | null, cwd: string): Promise<SessionInfo | null>;
  /** Names the tab. */
  rename(sid: number, title: string): void;
  /** Types the command once the terminal shows a prompt. */
  start(sid: number, command: string): void;
}

/** The dialog of "New agent" (and "New terminal", which is the same without the agent). */
export async function newAgentDialog(ctx: LaunchCtx, space: Space | null) {
  const kinds = await ctx.be.agentKinds().catch(() => [] as AgentKindRow[]);
  openModal((close) => {
    const folders = space?.folders ?? [];
    const folder = el("input", { value: folders[0] ?? "", autocomplete: "off", spellcheck: "false", placeholder: "~/code/your-project (empty: the home folder)", list: "launch-folders", "data-name": "launch-folder" }) as HTMLInputElement;
    const known = el("datalist", { id: "launch-folders" }, ...folders.map((f) => el("option", { value: f })));
    const agent = makeSelect(kinds.map((k): [string, string] => [k.id, k.name]), kinds[0]?.id ?? "", { "data-name": "launch-agent" });
    const useTree = el("input", { type: "checkbox", id: "launch-wt", "data-name": "launch-worktree" }) as HTMLInputElement;
    const branch = el("input", { value: proposedBranch(kinds[0]?.id ?? "agent", new Date()), autocomplete: "off", spellcheck: "false", "data-name": "launch-branch" }) as HTMLInputElement;
    const branchRow = fieldHelp("Branch", branch, "A new branch in its own folder next to the repository (<repo>.worktrees/<branch>), so that agents do not step on each other.");
    branchRow.hidden = true;
    useTree.onchange = () => (branchRow.hidden = !useTree.checked);
    agent.onchange = () => {
      if (branch.dataset.edited !== "1") branch.value = proposedBranch(agent.value, new Date());
    };
    branch.oninput = () => (branch.dataset.edited = "1");
    const err = el("div", { class: "err" });
    const go = async (withAgent: boolean) => {
      err.textContent = "";
      let cwd = folder.value.trim();
      const kind = kinds.find((k) => k.id === agent.value);
      if (withAgent && !kind) return void (err.textContent = "There is no agent in the list: add one in Settings → Agents.");
      if (useTree.checked) {
        if (!cwd) return void (err.textContent = "A worktree needs the folder of a repository.");
        try {
          cwd = await ctx.be.worktreeAdd(cwd, branch.value.trim());
        } catch (e) {
          err.textContent = (e as Error).message;
          return;
        }
      }
      close();
      const info = await ctx.openLocal(space, cwd);
      if (!info) return;
      const where = cwd ? baseName(cwd) : "";
      if (withAgent && kind) {
        ctx.rename(info.sid, [kind.name, where].filter(Boolean).join(" · "));
        ctx.start(info.sid, kind.command);
      } else if (where) ctx.rename(info.sid, ["Local", where].join(" · "));
      toast(withAgent && kind ? `${kind.name} is starting${useTree.checked ? ` on ${branch.value.trim()}` : ""}` : "Terminal opened");
    };
    const startBtn = el("button", { class: "btn primary", type: "button", "data-act": "launch-agent" }, "Start agent");
    const termBtn = el("button", { class: "btn", type: "button", "data-act": "launch-terminal" }, "Terminal without an agent");
    const no = el("button", { class: "btn", type: "button" }, "Cancel");
    no.onclick = close;
    startBtn.onclick = () => void go(true);
    termBtn.onclick = () => void go(false);
    const form = el(
      "form",
      { class: "launch-form" },
      el("h2", {}, space ? `New agent in ${space.name}` : "New agent"),
      fieldHelp("Folder", el("div", {}, folder, known), "Where the terminal opens. Pick a repository for an agent."),
      field("Agent", agent),
      el("label", { class: "group-check" }, useTree, "In a new git worktree"),
      branchRow,
      err,
      el("div", { class: "actions" }, no, termBtn, startBtn),
    );
    form.onsubmit = (e) => {
      e.preventDefault();
      void go(true);
    };
    queueMicrotask(() => folder.focus());
    return el("div", {}, form);
  });
}

/** The list of agents: what is looked for in a terminal, and what is typed to start one. */
export async function agentKindsDialog(be: Backend) {
  let kinds = await be.agentKinds().catch(() => [] as AgentKindRow[]);
  openModal((close) => {
    const rows = el("div", { class: "agent-kinds", "data-name": "agent-kinds" });
    const err = el("div", { class: "err" });
    const draw = () => {
      clear(rows);
      kinds.forEach((k, i) => {
        const name = el("input", { value: k.name, autocomplete: "off", "data-name": "kind-name", "aria-label": "Name" }) as HTMLInputElement;
        const cmd = el("input", { value: k.command, autocomplete: "off", spellcheck: "false", "data-name": "kind-command", "aria-label": "Command" }) as HTMLInputElement;
        name.oninput = () => (kinds[i] = { ...kinds[i], name: name.value });
        cmd.oninput = () => (kinds[i] = { ...kinds[i], command: cmd.value });
        const rm = el("button", { class: "ib", type: "button", title: "Remove this agent", "data-act": "kind-remove" });
        rm.append(icon("close", 14));
        rm.onclick = () => {
          kinds = kinds.filter((_, j) => j !== i);
          draw();
        };
        rows.append(el("div", { class: "keyrow", style: "grid-template-columns:1fr 1.4fr 34px" }, name, cmd, rm));
      });
    };
    draw();
    const add = el("button", { class: "btn small", type: "button", "data-act": "kind-add" }, "+ Add an agent");
    add.onclick = () => {
      kinds = [...kinds, { id: `agent-${kinds.length + 1}`, name: "", command: "" }];
      draw();
    };
    const reset = el("button", { class: "btn small", type: "button", "data-act": "kind-defaults" }, "Back to claude, opencode, codex");
    reset.onclick = async () => {
      await be.agentKindsSave([]);
      kinds = await be.agentKinds();
      draw();
    };
    const save = el("button", { class: "btn primary", type: "button", "data-act": "kinds-save" }, "Save");
    const no = el("button", { class: "btn", type: "button" }, "Cancel");
    no.onclick = close;
    save.onclick = async () => {
      // an id comes from its name (the first time) so that a new one needs no thought
      const slugs = new Set<string>();
      const fixed = kinds.map((k) => {
        let id = /^agent-\d+$/.test(k.id) || !k.id ? k.name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || k.id : k.id;
        while (slugs.has(id)) id += "-2";
        slugs.add(id);
        return { id, name: k.name.trim(), command: k.command.trim() };
      });
      try {
        await be.agentKindsSave(fixed);
      } catch (e) {
        err.textContent = (e as Error).message;
        return;
      }
      close();
      toast("Agents saved");
    };
    return el("div", { class: "wide" }, el("h2", {}, "Agents"), el("div", { class: "hint", style: "margin:0 0 10px" }, "The programs oso treats as AI agents: it finds them in your terminals (also when you start them by hand) and starts them with “New agent”. The first word of the command is the program it looks for."), el("div", { class: "keyrow head", style: "grid-template-columns:1fr 1.4fr 34px" }, el("span", {}, "Name"), el("span", {}, "Command")), rows, el("div", { style: "margin-top:8px;display:flex;gap:8px" }, add, reset), err, el("div", { class: "actions" }, no, save));
  });
}
