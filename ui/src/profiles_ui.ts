// Profiles: "work", "personal". Switching the profile switches the hosts and secrets that are shown, and the
// secrets and accounts the new terminals start with.

import { Backend, Profile, Proxy } from "./api";
import { clear, confirmBox, el, field, makeSelect, openModal, promptBox, toast } from "./dom";
import { icon } from "./icons";

/** The drop-down of a server or a folder: no profile of its own, or one of them. */
export function profileOptions(profiles: Profile[], none: string): [string, string][] {
  return [["", none], ...profiles.map((p): [string, string] => [p.id, p.name])];
}

/** Add, rename and delete profiles. `onChange` runs when the window must draw again. */
export function profilesDialog(be: Backend, profiles: () => Profile[], proxies: () => Proxy[], onChange: () => Promise<void>) {
  openModal((close) => {
    const list = el("div", { class: "plist" });
    const draw = () => {
      clear(list);
      for (const p of profiles()) {
        const btn = (name: string, title: string, f: () => void) => {
          const b = el("button", { class: "ib", type: "button", title, "aria-label": title });
          b.append(icon(name, 15));
          b.onclick = f;
          return b;
        };
        const row = el("div", { class: "vrow prow", "data-profile": p.id }, el("div", { class: "who" }, el("span", { class: "txt" }, el("b", {}, p.name), el("small", {}, p.id === "default" ? "always there" : p.id))));
        const acts = el("div", { class: "secret" });
        const px = makeSelect([["", "No proxy"], ...proxies().map((x): [string, string] => [String(x.id), x.name])], p.proxy == null ? "" : String(p.proxy), { "data-name": "profile-proxy", title: "Proxy of this profile's local terminals" });
        px.onchange = async () => {
          try {
            await be.profileSetProxy(p.id, px.value ? Number(px.value) : null);
            await onChange();
          } catch (e) {
            toast((e as Error).message);
          }
        };
        acts.append(
          px,
          btn("edit", "Rename", async () => {
            const n = await promptBox("Rename profile", p.name);
            if (!n?.trim()) return;
            try {
              await be.profileSave(p.id, n);
              await onChange();
              draw();
            } catch (e) {
              toast((e as Error).message);
            }
          }),
        );
        if (p.id !== "default") {
          acts.append(
            btn("trash", "Delete", async () => {
              if (!(await confirmBox(`Delete “${p.name}”?`, "Its hosts, folders and secrets move to the default profile.", "Delete"))) return;
              try {
                await be.profileDelete(p.id);
                await onChange();
              } catch (e) {
                toast((e as Error).message, 6000);
              }
              draw();
            }),
          );
        }
        row.append(acts);
        list.append(row);
      }
    };
    draw();
    const name = el("input", { name: "new-profile", placeholder: "New profile (work, personal…)", autocomplete: "off", spellcheck: "false" });
    const add = el("button", { class: "btn", type: "submit" }, "Add");
    const form = el("form", { class: "filters", style: "grid-template-columns:1fr auto;margin-top:10px" }, name, add);
    form.onsubmit = async (e) => {
      e.preventDefault();
      if (!name.value.trim()) return;
      try {
        await be.profileSave("", name.value);
        name.value = "";
        await onChange();
        draw();
      } catch (x) {
        toast((x as Error).message);
      }
    };
    const done = el("button", { class: "btn primary", type: "button" }, "Done");
    done.onclick = close;
    const prox = el("button", { class: "btn", type: "button", "data-act": "proxies" }, "Proxies…");
    prox.onclick = () => {
      close();
      proxiesDialog(be, profiles, proxies, onChange, () => profilesDialog(be, profiles, proxies, onChange));
    };
    return el("div", {}, el("h2", {}, "Profiles"), el("div", { class: "hint", style: "margin:0 0 10px" }, "A profile keeps a set of hosts, secrets and accounts together. Switch it from the top of the host list."), list, form, el("div", { class: "actions" }, prox, done));
  });
}

/** Saved proxies: a server can connect through one, and a profile's local terminals can go out through one. */
export function proxiesDialog(be: Backend, profiles: () => Profile[], proxies: () => Proxy[], onChange: () => Promise<void>, back: () => void) {
  openModal((close) => {
    const list = el("div", { class: "plist" });
    const draw = () => {
      clear(list);
      if (!proxies().length) list.append(el("div", { class: "empty" }, "No proxies yet. Add the one your network needs: a server can then be reached through it, and the local terminals of a profile can go out through it (only those terminals; the system's network is never touched)."));
      for (const x of proxies()) {
        const btn = (name: string, title: string, f: () => void) => {
          const b = el("button", { class: "ib", type: "button", title, "aria-label": title });
          b.append(icon(name, 15));
          b.onclick = f;
          return b;
        };
        list.append(
          el("div", { class: "vrow prow", "data-proxy": x.name }, el("div", { class: "who" }, el("span", { class: "txt" }, el("b", {}, x.name), el("small", {}, `${x.kind === "socks5" ? "SOCKS5" : "HTTP"} · ${x.host}:${x.port}`))), el("div", { class: "secret" },
            btn("edit", "Change", () => {
              close();
              proxyForm(be, profiles, x, onChange, () => proxiesDialog(be, profiles, proxies, onChange, back));
            }),
            btn("trash", "Delete", async () => {
              if (!(await confirmBox(`Delete “${x.name}”?`, "What used it will connect directly.", "Delete"))) return;
              await be.proxyDelete(x.id);
              await onChange();
              draw();
            }),
          )),
        );
      }
    };
    draw();
    const add = el("button", { class: "btn", type: "button", "data-act": "new-proxy" }, "New proxy");
    add.onclick = () => {
      close();
      proxyForm(be, profiles, null, onChange, () => proxiesDialog(be, profiles, proxies, onChange, back));
    };
    const done = el("button", { class: "btn primary", type: "button" }, "Done");
    done.onclick = () => {
      close();
      back();
    };
    return el("div", {}, el("h2", {}, "Proxies"), list, el("div", { class: "actions" }, add, done));
  });
}

function proxyForm(be: Backend, profiles: () => Profile[], existing: Proxy | null, onChange: () => Promise<void>, again: () => void) {
  const p: Proxy = existing ?? { id: 0, name: "", kind: "socks5", host: "", port: 1080, profile: null };
  openModal((close) => {
    const name = el("input", { value: p.name, name: "proxy-name", autocomplete: "off", placeholder: "office" });
    const kind = makeSelect([["socks5", "SOCKS5"], ["http", "HTTP (CONNECT)"]], p.kind, { "data-name": "proxy-kind" });
    const host = el("input", { value: p.host, name: "proxy-host", autocomplete: "off", spellcheck: "false", placeholder: "proxy.example.com" });
    const port = el("input", { value: String(p.port), name: "proxy-port", inputmode: "numeric", autocomplete: "off" });
    const profile = makeSelect(profileOptions(profiles(), "Every profile"), p.profile ?? "", { "data-name": "proxy-profile" });
    const err = el("div", { class: "err" });
    const form = el("form", {}, el("h2", {}, existing ? `Change ${p.name}` : "New proxy"), field("Name", name), field("Type", kind), el("div", { class: "grid2" }, field("Host", host), field("Port", port)), field("Profile", profile), err);
    const ok = el("button", { class: "btn primary", type: "submit" }, "Save");
    const no = el("button", { class: "btn", type: "button" }, "Cancel");
    no.onclick = close;
    form.append(el("div", { class: "actions" }, no, ok));
    form.onsubmit = async (e) => {
      e.preventDefault();
      const n = Number(port.value);
      if (!Number.isInteger(n) || n < 1 || n > 65535) return void (err.textContent = "The port must be a number from 1 to 65535");
      try {
        await be.proxySave({ ...p, name: name.value, kind: kind.value as Proxy["kind"], host: host.value, port: n, profile: profile.value || null });
        await onChange();
        close();
      } catch (x) {
        err.textContent = (x as Error).message;
      }
    };
    return form;
  }, again);
}

/** The strip above the host list: which profile is active, a switch to see every profile's hosts, and the manager. */
export function profileBar(be: Backend, state: () => { profiles: Profile[]; proxies: Proxy[]; active: string; all: boolean }, changed: () => Promise<void>): HTMLElement {
  const { profiles, active, all } = state();
  const bar = el("div", { class: "profile-bar" });
  const sel = makeSelect(profiles.map((p): [string, string] => [p.id, p.name]), active, { "data-name": "active-profile", title: "Active profile" });
  sel.onchange = async () => {
    await be.setActiveProfile(sel.value);
    await changed();
  };
  const eye = el("button", { class: `ib${all ? " on" : ""}`, type: "button", title: all ? "Showing every profile's hosts" : "Show every profile's hosts", "aria-pressed": String(all), "data-act": "view-all" });
  eye.append(icon("eye", 15));
  eye.onclick = async () => {
    await be.setViewAll(!all);
    await changed();
  };
  const more = el("button", { class: "ib", type: "button", title: "Profiles", "data-act": "profiles" });
  more.append(icon("more", 15));
  more.onclick = () => profilesDialog(be, () => state().profiles, () => state().proxies, changed);
  bar.append(sel, eye, more);
  return bar;
}
