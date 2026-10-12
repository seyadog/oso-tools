// The Tunnels window: every saved port forward of every server, each with a switch.

import { Backend, Forward, Server, TunnelStatus, VaultLockedError } from "./api";
import { clear, confirmBox, el, field, makeSelect, openModal, toast } from "./dom";
import { icon, serverColor } from "./icons";
import { KIND_LABEL, Kind, describe, problem, sameTunnel } from "./tunnels";

export interface Ctx {
  be: Backend;
  servers(): Server[];
  refreshTree(): Promise<void>;
  unlock(offerWithout: boolean): Promise<"unlocked" | "without" | "cancel">;
}

const key = (s: number, f: number) => `${s}:${f}`;

/** Switches a tunnel on, asking for the vault if it needs it. Returns whether it was started. */
export async function switchOn(ctx: Ctx, serverId: number, forwardId: number, beforeUnlock: () => void = () => {}): Promise<boolean> {
  let useVault = true;
  for (;;) {
    try {
      await ctx.be.tunnelStart(serverId, forwardId, useVault);
      return true;
    } catch (e) {
      if (e instanceof VaultLockedError) {
        beforeUnlock();
        const c = await ctx.unlock(true);
        if (c === "cancel") return false;
        useVault = c === "unlocked";
        continue;
      }
      toast(`Could not start the tunnel: ${(e as Error).message}`, 6000);
      return false;
    }
  }
}

export function tunnelsDialog(ctx: Ctx, onClose: () => void = () => {}) {
  let timer = 0;
  openModal(
    (close) => {
      const list = el("div", { class: "tunnels" });
      let status = new Map<string, TunnelStatus>();

      const draw = () => {
        clear(list);
        const withTunnels = ctx.servers().filter((s) => s.forwards.length);
        if (!withTunnels.length) {
          list.append(el("div", { class: "empty" }, "No tunnels yet. A tunnel keeps a port open through a server, for example to reach a database that only the server can see. Add one, then switch it on when you need it."));
        }
        for (const s of withTunnels) {
          list.append(el("div", { class: "t-head" }, el("span", { class: "dot", style: `background:${serverColor(s.id)}` }), s.name, el("small", {}, ` ${s.user ? s.user + "@" : ""}${s.host}`)));
          for (const f of s.forwards) {
            const st = status.get(key(s.id, f.id));
            const on = !!st && st.state !== "failed";
            const sw = el("input", { type: "checkbox", class: "switch", "aria-label": `Tunnel ${describe(f)}`, "data-tunnel": `${s.id}:${f.id}` }) as HTMLInputElement;
            sw.checked = on;
            sw.onchange = async () => {
              if (!sw.checked) {
                await ctx.be.tunnelStop(s.id, f.id);
                return void (await refresh());
              }
              // asking for the vault takes the place of this window: it comes back afterwards
              let left = false;
              const ok = await switchOn(ctx, s.id, f.id, () => {
                left = true;
                clearInterval(timer);
                close();
              });
              if (left) return tunnelsDialog(ctx, onClose);
              if (!ok) sw.checked = false;
              await refresh();
            };
            const edit = el("button", { class: "ib", type: "button", title: "Edit" });
            edit.append(icon("edit", 15));
            edit.onclick = () => {
              clearInterval(timer);
              close();
              tunnelForm(ctx, s.id, f, () => tunnelsDialog(ctx, onClose));
            };
            const del = el("button", { class: "ib", type: "button", title: "Delete" });
            del.append(icon("trash", 15));
            del.onclick = async () => {
              // the question takes the place of this window; it comes back after the answer
              clearInterval(timer);
              close();
              if (await confirmBox("Delete this tunnel?", `${describe(f)} (${s.name})`, "Delete")) {
                await ctx.be.setForwards(s.id, s.forwards.filter((x) => x.id !== f.id));
                await ctx.refreshTree();
              }
              tunnelsDialog(ctx, onClose);
            };
            const state = st?.state === "up" ? "on" : st?.state === "starting" ? "connecting…" : "";
            list.append(
              el(
                "div",
                { class: `t-row${st?.state === "up" ? " up" : ""}${st?.state === "failed" ? " failed" : ""}` },
                sw,
                el("div", { class: "txt" }, el("b", {}, f.name.trim() || describe(f)), st?.state === "failed" ? el("small", { class: "err" }, st.message) : el("small", {}, [f.name.trim() ? describe(f) : "", f.auto ? "with every terminal" : "", state].filter(Boolean).join(" · "))),
                edit,
                del,
              ),
            );
          }
        }
      };
      const refresh = async () => {
        try {
          status = new Map((await ctx.be.tunnels()).map((t) => [key(t.server_id, t.forward_id), t]));
        } catch {
          /* the next poll will do */
        }
        // do not redraw under the pointer while a switch is being clicked
        if (!list.querySelector("input:active")) draw();
      };
      void refresh();
      timer = window.setInterval(() => void refresh(), 1200);

      const add = el("button", { class: "btn", type: "button" }, "New tunnel");
      add.onclick = () => {
        if (!ctx.servers().length) return toast("Add a server first");
        clearInterval(timer);
        close();
        tunnelForm(ctx, null, null, () => tunnelsDialog(ctx, onClose));
      };
      const done = el("button", { class: "btn primary", type: "button" }, "Done");
      done.onclick = close;
      return el(
        "div",
        { class: "wide" },
        el("h2", {}, "Tunnels"),
        el("div", { class: "hint", style: "margin:0 0 10px" }, "A tunnel keeps a port open through a server. Switch it on when you need it: it stays on while oso is open."),
        list,
        el("div", { class: "actions" }, add, done),
      );
    },
    () => {
      clearInterval(timer);
      onClose();
    },
  );
}

/** Add (`forward == null`) or edit one tunnel. `again` runs when it closes (to show the list again). */
export function tunnelForm(ctx: Ctx, serverId: number | null, forward: Forward | null, again: () => void) {
  openModal(
    (close) => {
      const servers = ctx.servers();
      const server = makeSelect(servers.map((s): [string, string] => [String(s.id), s.name]), String(serverId ?? servers[0].id), { "data-name": "server" });
      const kind = makeSelect(Object.entries(KIND_LABEL) as [string, string][], forward?.kind ?? "local", { "data-name": "kind" });
      const bind = el("input", { value: forward ? String(forward.bind_port) : "", inputmode: "numeric", name: "bind", autocomplete: "off" });
      const host = el("input", { value: forward?.host ?? "", name: "host", autocomplete: "off", spellcheck: "false", placeholder: "db.internal" });
      const port = el("input", { value: forward?.port ? String(forward.port) : "", inputmode: "numeric", name: "port", autocomplete: "off" });
      const name = el("input", { value: forward?.name ?? "", name: "tunnel-name", autocomplete: "off", spellcheck: "false", placeholder: "postgres of your-project (optional)" });
      const auto = el("input", { type: "checkbox", id: "tunnel-auto", name: "auto" }) as HTMLInputElement;
      auto.checked = forward?.auto ?? false;
      const bindLabel = el("label", {}, "");
      const hostRow = el("div", { class: "grid2" }, field("Host it leads to", host), field("Its port", port));
      const sync = () => {
        const k = kind.value as Kind;
        bindLabel.textContent = k === "remote" ? "Port to open on the server" : "Port to open on this computer";
        hostRow.style.display = k === "dynamic" ? "none" : "";
        (hostRow.children[0].firstChild as HTMLElement).textContent = k === "remote" ? "Host it leads to (as seen from here)" : "Host it leads to (as the server sees it)";
      };
      kind.onchange = sync;
      sync();
      const err = el("div", { class: "err" });
      const form = el("form", {}, el("h2", {}, forward ? "Edit tunnel" : "New tunnel"), field("Through the server", server), field("Name", name), field("What it does", kind), el("div", {}, bindLabel, bind), hostRow, el("div", { class: "hint" }, auto, el("label", { for: "tunnel-auto", style: "display:inline;margin-left:6px" }, "Open it with every terminal of this server")), err);
      const ok = el("button", { class: "btn primary", type: "submit" }, "Save");
      const no = el("button", { class: "btn", type: "button" }, "Cancel");
      no.onclick = close;
      form.append(el("div", { class: "actions" }, no, ok));
      form.onsubmit = async (e) => {
        e.preventDefault();
        const k = kind.value as Kind;
        const bad = problem(k, bind.value, host.value, port.value);
        if (bad) return void (err.textContent = bad);
        const sid = Number(server.value);
        const target = ctx.servers().find((s) => s.id === sid)!;
        const next: Forward = { id: forward?.id ?? 0, name: name.value.trim(), auto: auto.checked, kind: k, bind_port: Number(bind.value), host: k === "dynamic" ? "" : host.value.trim(), port: k === "dynamic" ? 0 : Number(port.value) };
        if (target.forwards.some((x) => x.id !== next.id && sameTunnel(x, next))) return void (err.textContent = "That tunnel already exists");
        if (k !== "remote" && ctx.servers().some((s) => s.forwards.some((x) => x.id !== next.id && x.kind !== "remote" && x.bind_port === next.bind_port))) {
          return void (err.textContent = `Port ${next.bind_port} is already used by another tunnel: two of them could not be on at once`);
        }
        try {
          // moving a tunnel to another server removes it from the first one
          if (forward && serverId != null && serverId !== sid) await ctx.be.setForwards(serverId, ctx.servers().find((s) => s.id === serverId)!.forwards.filter((x) => x.id !== forward.id));
          const rest = target.forwards.filter((x) => x.id !== forward?.id || serverId !== sid);
          const list = forward && serverId === sid ? target.forwards.map((x) => (x.id === forward.id ? next : x)) : [...rest, { ...next, id: 0 }];
          await ctx.be.setForwards(sid, list);
          await ctx.refreshTree();
          close();
        } catch (x) {
          err.textContent = (x as Error).message;
        }
      };
      return form;
    },
    again,
  );
}
