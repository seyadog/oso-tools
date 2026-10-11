// Profiles: "work", "personal". Switching the profile switches the hosts and secrets that are shown, and the
// secrets and accounts the new terminals start with.

import { Backend, Profile, Proxy } from "./api";
import { clear, confirmBox, el, field, makeSelect, openModal, promptBox, toast } from "./dom";
import { icon } from "./icons";
import { accountChoices } from "./accounts_ui";

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
        const accts = btn("key", "GitHub and Tailscale accounts of this profile", () => {
          close();
          void profileAccountsDialog(be, p, onChange, () => profilesDialog(be, profiles, proxies, onChange));
        });
        accts.setAttribute("data-act", "profile-accounts");
        acts.append(
          accts,
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

/** The accounts a profile puts in use when it becomes the active one. */
async function profileAccountsDialog(be: Backend, p: Profile, onChange: () => Promise<void>, again: () => void) {
  const view = await be.accounts().catch(() => null);
  const cloud = await be.cloudOptions().catch(() => ({ aws_profiles: [] as string[], kubeconfigs: [] as { path: string; contexts: string[] }[] }));
  openModal((close) => {
    const gh = makeSelect(accountChoices(view?.github, p.gh_user), p.gh_user ?? "", { "data-name": "gh-user" });
    const ts = makeSelect(accountChoices(view?.tailscale, p.tailscale_account), p.tailscale_account ?? "", { "data-name": "ts-account" });
    const aws = makeSelect(accountChoices({ installed: true, error: null, accounts: cloud.aws_profiles.map((n) => ({ id: n, name: n, detail: "", active: false })) }, p.aws_profile), p.aws_profile ?? "", { "data-name": "aws-profile" });
    const kube = makeSelect(accountChoices({ installed: true, error: null, accounts: cloud.kubeconfigs.map((k) => ({ id: k.path, name: k.path.split("/").pop() ?? k.path, detail: k.contexts.join(", "), active: false })) }, p.kubeconfig), p.kubeconfig ?? "", { "data-name": "kubeconfig" });
    const prod = el("input", { type: "checkbox", id: "profile-production", name: "production" }) as HTMLInputElement;
    prod.checked = p.production;
    const form = el("form", {}, el("h2", {}, `Accounts of ${p.name}`), el("div", { class: "hint", style: "margin:0 0 10px" }, "Switching to this profile also switches these. oso does not keep their tokens: the tools themselves do. Leave one alone to not touch it."));
    if (view?.github.installed) form.append(field("GitHub (gh)", gh));
    if (view?.tailscale.installed) form.append(field("Tailscale", ts));
    if (!view?.github.installed && !view?.tailscale.installed) form.append(el("div", { class: "empty" }, "Neither `gh` nor `tailscale` is installed."));
    if (view?.tailscale.error) form.append(el("div", { class: "hint" }, view.tailscale.error));
    form.append(el("h3", {}, "Cloud"), el("div", { class: "hint", style: "margin:0 0 8px" }, "Set in this profile's local terminals (AWS_PROFILE, KUBECONFIG). Only names are read; no key is ever shown or kept."), field("AWS profile", aws), field("Kubernetes config", kube), el("div", { class: "hint" }, prod, el("label", { for: "profile-production", style: "display:inline;margin-left:6px" }, "This is a production profile (shown in red)")));
    const ok = el("button", { class: "btn primary", type: "submit" }, "Save");
    const no = el("button", { class: "btn", type: "button" }, "Cancel");
    no.onclick = close;
    form.append(el("div", { class: "actions" }, no, ok));
    form.onsubmit = async (e) => {
      e.preventDefault();
      try {
        await be.profileSetCloud(p.id, aws.value || null, kube.value || null, prod.checked);
        await be.profileSetAccounts(p.id, view?.github.installed ? gh.value || null : p.gh_user, view?.tailscale.installed ? ts.value || null : p.tailscale_account);
        await onChange();
        close();
      } catch (x) {
        toast((x as Error).message);
      }
    };
    return form;
  }, again);
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
  const current = profiles.find((p) => p.id === active);
  const bar = el("div", { class: `profile-bar${current?.production ? " prod" : ""}` });
  const sel = makeSelect(profiles.map((p): [string, string] => [p.id, p.name]), active, { "data-name": "active-profile", title: "Active profile" });
  sel.onchange = async () => {
    const problems = await be.setActiveProfile(sel.value);
    if (problems.length) toast(problems.join(" · "), 8000);
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
