// The Spaces section of the left panel, and the form where a Space is made or changed.

import { Backend, SecretMeta, Space, TreeData } from "./api";
import { clear, confirmBox, contextMenu, el, field, fieldHelp, makeSelect, openModal, toast } from "./dom";
import { icon } from "./icons";
import { emptySpace, missingOf, spaceSummary } from "./spaces";

export interface SpacesCtx {
  be: Backend;
  tree(): TreeData;
  /** The secrets of the vault by name and kind (`null` while it is locked). */
  secrets(): SecretMeta[] | null;
  spaces(): Space[];
  /** How many terminals of each Space are open. */
  openCount(id: number): number;
  open(s: Space, again?: boolean): void;
  newTerminal(s: Space): void;
  /** Starts an agent in a Space (not there until agents are: the button is only drawn when this is). */
  newAgent?(s: Space): void;
  /** Reads the Spaces again and draws the panel. */
  changed(): Promise<void>;
  /** The section header (with its "+ New Space" button) is drawn by the app, as for every section. */
  emptyBox(text: string, buttons: [string, () => void, boolean, string][]): HTMLElement;
}

/** The rows of the Spaces section. */
export function renderSpaces(host: HTMLElement, ctx: SpacesCtx) {
  const spaces = ctx.spaces();
  if (!spaces.length) {
    host.append(ctx.emptyBox("A Space is one piece of work: the folders, servers, secrets and notes it uses, and the tabs you open for it. Open it and everything is where you left it; an AI agent started in it sees only what it holds.", [["+ New Space", () => newSpace(ctx), true, "empty-new-space"]]));
    return;
  }
  const tree = ctx.tree();
  for (const sp of spaces) {
    const running = ctx.openCount(sp.id);
    const row = el("div", { class: `row space${running ? " running" : ""}`, "data-space": String(sp.id), title: sp.notes ? sp.notes.slice(0, 200) : "Double-click to open it" });
    const gi = el("span", { class: "gi" });
    gi.append(icon("layers", 15));
    const acts = el("span", { class: "acts" });
    const stop = (f: () => void) => (e: Event) => {
      e.stopPropagation();
      f();
    };
    const btn = (name: string, title: string, run: () => void, act: string) => {
      const b = el("button", { class: "ib", type: "button", title, "aria-label": title, "data-act": act });
      b.append(icon(name, 16));
      b.style.cssText = "width:24px;height:24px";
      b.onclick = stop(run);
      return b;
    };
    acts.append(
      btn("play", "Open this Space", () => ctx.open(sp), "space-open"),
      btn("terminal", "New terminal in this Space", () => ctx.newTerminal(sp), "space-terminal"),
      ...(ctx.newAgent ? [btn("agent", "New agent in this Space", () => ctx.newAgent?.(sp), "space-agent")] : []),
      btn("edit", "Edit", () => editSpace(ctx, sp), "space-edit"),
      btn("trash", "Delete", () => void deleteSpace(ctx, sp), "space-delete"),
    );
    const tail = el("span", { class: "tail", title: running ? `${running} open terminal${running > 1 ? "s" : ""}` : "" }, running ? String(running) : "");
    row.append(gi, el("span", { class: "txt" }, el("b", {}, sp.name), el("small", {}, spaceSummary(sp, tree))), tail, acts);
    row.ondblclick = () => ctx.open(sp);
    row.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      contextMenu(e.clientX, e.clientY, [
        { label: running ? "Go to it" : "Open", run: () => ctx.open(sp) },
        ...(running ? [{ label: "Open it again", run: () => ctx.open(sp, true) }] : []),
        { label: "New terminal", run: () => ctx.newTerminal(sp) },
        ...(ctx.newAgent ? [{ label: "New agent…", run: () => ctx.newAgent?.(sp) }] : []),
        { label: "Edit…", run: () => editSpace(ctx, sp) },
        { label: "Delete", run: () => void deleteSpace(ctx, sp), danger: true },
      ]);
    });
    host.append(row);
  }
}

export function newSpace(ctx: SpacesCtx, preset: Partial<Space> = {}): Promise<number | null> {
  return spaceForm(ctx, { ...emptySpace(), ...preset });
}

export function editSpace(ctx: SpacesCtx, sp: Space): Promise<number | null> {
  return spaceForm(ctx, sp);
}

async function deleteSpace(ctx: SpacesCtx, sp: Space) {
  const running = ctx.openCount(sp.id);
  const text = running ? `Its ${running} open terminal${running > 1 ? "s stay" : " stays"} open, but an AI agent running in ${running > 1 ? "them" : "it"} will be refused from now on.` : "Only the Space goes: its servers, secrets and folders are not touched.";
  if (!(await confirmBox(`Delete the Space “${sp.name}”?`, text, "Delete"))) return;
  await ctx.be.spaceDelete(sp.id);
  await ctx.changed();
}

/** The form: name, profile, folders, servers, secrets, tunnels, notes, and the saved tabs. Resolves to the id saved. */
function spaceForm(ctx: SpacesCtx, sp: Space): Promise<number | null> {
  return new Promise((resolve) => {
    let saved: number | null = null;
    openModal(
      (close) => {
        const tree = ctx.tree();
        const secrets = ctx.secrets();
        const name = el("input", { value: sp.name, autocomplete: "off", placeholder: "Acme API", "data-name": "space-name" }) as HTMLInputElement;
        const profile = makeSelect([["", "Whichever is active"], ...tree.profiles.map((p): [string, string] => [p.id, p.name])], sp.profile ?? "", { "data-name": "space-profile" });
        // folders: one line each, typed (there is no folder picker yet)
        const folderRows = el("div", { class: "space-folders" });
        const addFolder = (value = "") => {
          const input = el("input", { value, autocomplete: "off", spellcheck: "false", placeholder: "~/code/acme", "data-name": "space-folder" }) as HTMLInputElement;
          const rm = el("button", { class: "ib", type: "button", title: "Remove this folder" });
          rm.append(icon("close", 14));
          const line = el("div", { class: "keyrow", style: "grid-template-columns:1fr 34px" }, input, rm);
          rm.onclick = () => line.remove();
          folderRows.append(line);
        };
        sp.folders.forEach(addFolder);
        const addBtn = el("button", { class: "btn small", type: "button", "data-act": "space-add-folder" }, "+ Add a folder");
        addBtn.onclick = () => addFolder();
        // servers and folders of the tree
        const pickedServers = new Set(sp.servers);
        const pickedFolders = new Set(sp.server_folders);
        const serverList = el("div", { class: "checklist", "data-name": "space-servers" });
        const folderDepth = (id: number | null): number => {
          let d = 0;
          for (let f = tree.folders.find((x) => x.id === id); f && d < 20; f = tree.folders.find((x) => x.id === f!.parent)) d++;
          return d;
        };
        const check = (label: string, sub: string, on: boolean, depth: number, set: Set<number>, id: number, data: string) => {
          const box = el("input", { type: "checkbox", "data-pick": data }) as HTMLInputElement;
          box.checked = on;
          box.onchange = () => (box.checked ? set.add(id) : set.delete(id));
          return el("label", { class: "check", style: `padding-left:${depth * 16}px` }, box, el("span", {}, label, sub ? el("small", {}, sub) : null));
        };
        const walk = (parent: number | null) => {
          for (const f of tree.folders.filter((x) => x.parent === parent)) {
            serverList.append(check(f.name, "everything inside", pickedFolders.has(f.id), folderDepth(f.id) - 1, pickedFolders, f.id, `folder:${f.id}`));
            walk(f.id);
          }
          for (const s of tree.servers.filter((x) => x.parent === parent)) serverList.append(check(s.name, `${s.user}@${s.host}`, pickedServers.has(s.id), parent === null ? 0 : folderDepth(parent), pickedServers, s.id, `server:${s.id}`));
        };
        walk(null);
        if (!tree.servers.length) serverList.append(el("div", { class: "hint" }, "You have no servers yet."));
        // secrets
        const pickedSecrets = new Set(sp.secrets);
        const secretList = el("div", { class: "checklist", "data-name": "space-secrets" });
        if (secrets === null) secretList.append(el("div", { class: "hint" }, `The vault is locked: unlock it to pick secrets. ${sp.secrets.length ? `The ${sp.secrets.length} already picked are kept.` : ""}`));
        else if (!secrets.length) secretList.append(el("div", { class: "hint" }, "The vault has no secrets yet."));
        else for (const m of secrets) secretList.append(check(m.name, `${m.kind.replace("_", " ")}${m.env_var ? ` · $${m.env_var}` : ""}`, pickedSecrets.has(m.id), 0, pickedSecrets, m.id, `secret:${m.id}`));
        // tunnels
        const pickedTunnels = new Set(sp.tunnels.map((t) => `${t.server}:${t.forward}`));
        const tunnelList = el("div", { class: "checklist", "data-name": "space-tunnels" });
        const allTunnels = tree.servers.flatMap((s) => s.forwards.map((f) => ({ s, f })));
        if (!allTunnels.length) tunnelList.append(el("div", { class: "hint" }, "No saved tunnels yet (see Tunnels on the left)."));
        for (const { s, f } of allTunnels) {
          const key = `${s.id}:${f.id}`;
          const box = el("input", { type: "checkbox", "data-pick": `tunnel:${key}` }) as HTMLInputElement;
          box.checked = pickedTunnels.has(key);
          box.onchange = () => (box.checked ? pickedTunnels.add(key) : pickedTunnels.delete(key));
          tunnelList.append(el("label", { class: "check" }, box, el("span", {}, f.name || `port ${f.bind_port}`, el("small", {}, `${s.name} · opens with the Space`))));
        }
        const notes = el("textarea", { rows: "4", placeholder: "What this is, what to watch out for. An agent started here reads it.", "data-name": "space-notes" }) as HTMLTextAreaElement;
        notes.value = sp.notes;
        // saved tabs
        let tabs = [...sp.tabs];
        const tabList = el("div", { class: "space-tabs" });
        const drawTabs = () => {
          clear(tabList);
          if (!tabs.length) tabList.append(el("div", { class: "hint" }, "No saved tabs. Arrange some terminals in a tab, then use “Save this tab in a Space” (Alt+G): it opens again with the Space."));
          tabs.forEach((t, i) => {
            const rm = el("button", { class: "ib", type: "button", title: "Remove this tab from the Space" });
            rm.append(icon("close", 14));
            rm.onclick = () => {
              tabs = tabs.filter((_, j) => j !== i);
              drawTabs();
            };
            tabList.append(el("div", { class: "keyrow", style: "grid-template-columns:1fr 34px" }, el("span", {}, `${t.name || `Tab ${i + 1}`}`, el("small", { class: "dim" }, ` · ${t.panes.length} terminal${t.panes.length > 1 ? "s" : ""}`)), rm));
          });
        };
        drawTabs();
        const gone = missingOf(sp, tree, secrets ? secrets.map((m) => m.id) : null);
        const goneCount = gone.servers + gone.folders + gone.secrets;
        const err = el("div", { class: "err" });
        const ok = el("button", { class: "btn primary", type: "submit", "data-act": "space-save" }, "Save");
        const no = el("button", { class: "btn", type: "button" }, "Cancel");
        no.onclick = close;
        const form = el(
          "form",
          { class: "space-form" },
          el("h2", {}, sp.id ? `Edit ${sp.name}` : "New Space"),
          field("Name", name),
          fieldHelp("Profile", profile, "The profile to work in when this Space opens. Leave it if the Space does not care."),
          fieldHelp("Work folders", el("div", {}, folderRows, addBtn), "Where its terminals open. The first one is the default."),
          fieldHelp("Servers", serverList, "The ones this Space may use. A folder brings everything inside it."),
          fieldHelp("Secrets", secretList, "API keys and SSH keys of this Space. A terminal opened here gets only these."),
          fieldHelp("Tunnels", tunnelList, "Opened when the Space opens."),
          field("Notes", notes),
          fieldHelp("Saved tabs", tabList, "Opened again, as they were, when the Space opens."),
          goneCount ? el("div", { class: "hint warn" }, `${goneCount} thing${goneCount > 1 ? "s" : ""} this Space used ${goneCount > 1 ? "no longer exist" : "no longer exists"}; saving drops ${goneCount > 1 ? "them" : "it"}.`) : null,
          err,
          el("div", { class: "actions" }, no, ok),
        );
        form.onsubmit = async (e) => {
          e.preventDefault();
          if (!name.value.trim()) {
            err.textContent = "A Space needs a name.";
            name.focus();
            return;
          }
          const folders = [...folderRows.querySelectorAll("input")].map((i) => (i as HTMLInputElement).value.trim()).filter(Boolean);
          // what no longer exists is dropped; a secret the vault could not show (locked) is kept as it was
          const secretIds = secrets === null ? sp.secrets : [...pickedSecrets].filter((id) => secrets.some((m) => m.id === id));
          const next: Space = {
            ...sp,
            name: name.value.trim(),
            profile: profile.value || null,
            folders,
            servers: [...pickedServers].filter((id) => tree.servers.some((s) => s.id === id)),
            server_folders: [...pickedFolders].filter((id) => tree.folders.some((f) => f.id === id)),
            secrets: secretIds,
            tunnels: [...pickedTunnels].map((k) => ({ server: Number(k.split(":")[0]), forward: Number(k.split(":")[1]) })).filter((t) => tree.servers.some((s) => s.id === t.server && s.forwards.some((f) => f.id === t.forward))),
            notes: notes.value,
            tabs,
          };
          try {
            saved = await ctx.be.spaceSave(next);
          } catch (x) {
            err.textContent = (x as Error).message;
            return;
          }
          close();
          await ctx.changed();
          toast(`Saved “${next.name}”.`);
        };
        queueMicrotask(() => name.focus());
        return el("div", { class: "wide" }, form);
      },
      () => resolve(saved),
    );
  });
}

