// The top of the left panel: which Space you work in (General by default), and what a Space or General fixes for the
// machine while you work in it (accounts, a proxy, cloud settings), and the saved proxies.
//
// General is what is in no Space: it is available in every Space. A Space may fix a Tailscale account, a GitHub account,
// a proxy for its local terminals and cloud settings; oso asks before switching an account.

import { AccountsView, Backend, CloudOptions, Proxy, SpaceSettings } from "./api";
import { clear, confirmBox, el, field, makeSelect, openModal, toast } from "./dom";
import { icon } from "./icons";
import { accountChoices } from "./accounts_ui";

export const NO_SETTINGS: SpaceSettings = { tailscale_account: null, gh_user: null, proxy: null, aws_profile: null, kubeconfig: null, production: false };

/** The part of a Space (or of General) that is about the machine, as its own piece of the form. */
export interface SettingsFields {
  el: HTMLElement;
  read(): SpaceSettings;
}

/**
 * "Accounts and network": the Tailscale and GitHub accounts, the proxy, and the cloud settings. Each is optional ("leave it
 * alone"). A tool that is not installed is not offered, and its saved value is kept as it was.
 */
export function settingsFields(s: SpaceSettings, proxies: Proxy[], view: AccountsView | null, cloud: CloudOptions): SettingsFields {
  const gh = makeSelect(accountChoices(view?.github, s.gh_user), s.gh_user ?? "", { "data-name": "gh-user" });
  const ts = makeSelect(accountChoices(view?.tailscale, s.tailscale_account), s.tailscale_account ?? "", { "data-name": "ts-account" });
  const px = makeSelect([["", "No proxy"], ...proxies.map((x): [string, string] => [String(x.id), `${x.name} · ${x.kind === "socks5" ? "SOCKS5" : "HTTP"} ${x.host}:${x.port}`])], s.proxy == null ? "" : String(s.proxy), { "data-name": "space-proxy" });
  const aws = makeSelect(accountChoices({ installed: true, error: null, accounts: cloud.aws_profiles.map((n) => ({ id: n, name: n, detail: "", active: false })) }, s.aws_profile), s.aws_profile ?? "", { "data-name": "aws-profile" });
  const kube = makeSelect(accountChoices({ installed: true, error: null, accounts: cloud.kubeconfigs.map((k) => ({ id: k.path, name: k.path.split("/").pop() ?? k.path, detail: k.contexts.join(", "), active: false })) }, s.kubeconfig), s.kubeconfig ?? "", { "data-name": "kubeconfig" });
  const prod = el("input", { type: "checkbox", id: "space-production", name: "production" }) as HTMLInputElement;
  prod.checked = s.production;
  const box = el("div", { class: "space-settings" });
  box.append(el("h3", {}, "Accounts and network"), el("div", { class: "hint", style: "margin:0 0 8px" }, "Optional. When you work here, oso asks before switching to these accounts (it does not keep their tokens: the tools do). What you leave alone is not touched."));
  if (view?.github.installed) box.append(field("GitHub (gh)", gh));
  if (view?.tailscale.installed) box.append(field("Tailscale", ts));
  if (!view?.github.installed && !view?.tailscale.installed) box.append(el("div", { class: "empty" }, "Neither `gh` nor `tailscale` is installed."));
  box.append(field("Proxy for its local terminals", px), el("h3", {}, "Cloud"), el("div", { class: "hint", style: "margin:0 0 8px" }, "Set in its local terminals (AWS_PROFILE, KUBECONFIG). Only names are read; no key is ever shown or kept."), field("AWS account name", aws), field("Kubernetes config", kube), el("div", { class: "hint" }, prod, el("label", { for: "space-production", style: "display:inline;margin-left:6px" }, "Real machines are on the other end (shown in red)")));
  return {
    el: box,
    read: () => ({
      gh_user: view?.github.installed ? gh.value || null : s.gh_user,
      tailscale_account: view?.tailscale.installed ? ts.value || null : s.tailscale_account,
      proxy: px.value ? Number(px.value) : null,
      aws_profile: aws.value || null,
      kubeconfig: kube.value || null,
      production: prod.checked,
    }),
  };
}

/** The data the fields need, read before the window opens. */
export async function settingsData(be: Backend): Promise<{ view: AccountsView | null; cloud: CloudOptions }> {
  const [view, cloud] = await Promise.all([be.accounts().catch(() => null), be.cloudOptions().catch(() => ({ aws_profiles: [] as string[], kubeconfigs: [] as { path: string; contexts: string[] }[] }))]);
  return { view, cloud };
}

/** What General fixes: the same things as a Space, for everything that is in no Space. */
export async function generalDialog(be: Backend, proxies: () => Proxy[], onChange: () => Promise<void>) {
  const [settings, data] = await Promise.all([be.spaceSettings(null), settingsData(be)]);
  openModal((close) => {
    const fields = settingsFields(settings, proxies(), data.view, data.cloud);
    const ok = el("button", { class: "btn primary", type: "submit", "data-act": "general-save" }, "Save");
    const no = el("button", { class: "btn", type: "button" }, "Cancel");
    no.onclick = close;
    const prox = el("button", { class: "btn", type: "button", "data-act": "proxies" }, "Proxies…");
    prox.onclick = () => {
      close();
      proxiesDialog(be, proxies, onChange, () => void generalDialog(be, proxies, onChange));
    };
    const form = el("form", {}, el("h2", {}, "General"), el("div", { class: "hint", style: "margin:0 0 10px" }, "General is everything that is in no Space: its servers, secrets and tunnels are available in every Space. Here is what it fixes when you work in it."), fields.el, el("div", { class: "actions" }, prox, no, ok));
    form.onsubmit = async (e) => {
      e.preventDefault();
      try {
        await be.spaceSetSettings(null, fields.read());
        await onChange();
        close();
      } catch (x) {
        toast((x as Error).message);
      }
    };
    return form;
  });
}

/** Saved proxies: a server can connect through one, and the local terminals of a Space can go out through one. */
export function proxiesDialog(be: Backend, proxies: () => Proxy[], onChange: () => Promise<void>, back: () => void) {
  openModal((close) => {
    const list = el("div", { class: "plist" });
    const draw = () => {
      clear(list);
      if (!proxies().length) list.append(el("div", { class: "empty" }, "No proxies yet. Add the one your network needs: a server can then be reached through it, and the local terminals of a Space can go out through it (only those terminals; the system's network is never touched)."));
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
              proxyForm(be, x, onChange, () => proxiesDialog(be, proxies, onChange, back));
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
      proxyForm(be, null, onChange, () => proxiesDialog(be, proxies, onChange, back));
    };
    const done = el("button", { class: "btn primary", type: "button" }, "Done");
    done.onclick = () => {
      close();
      back();
    };
    return el("div", {}, el("h2", {}, "Proxies"), list, el("div", { class: "actions" }, add, done));
  });
}

function proxyForm(be: Backend, existing: Proxy | null, onChange: () => Promise<void>, again: () => void) {
  const p: Proxy = existing ?? { id: 0, name: "", kind: "socks5", host: "", port: 1080 };
  openModal((close) => {
    const name = el("input", { value: p.name, name: "proxy-name", autocomplete: "off", placeholder: "office" });
    const kind = makeSelect([["socks5", "SOCKS5"], ["http", "HTTP (CONNECT)"]], p.kind, { "data-name": "proxy-kind" });
    const host = el("input", { value: p.host, name: "proxy-host", autocomplete: "off", spellcheck: "false", placeholder: "proxy.example.com" });
    const port = el("input", { value: String(p.port), name: "proxy-port", inputmode: "numeric", autocomplete: "off" });
    const err = el("div", { class: "err" });
    const form = el("form", {}, el("h2", {}, existing ? `Change ${p.name}` : "New proxy"), field("Name", name), field("Type", kind), el("div", { class: "grid2" }, field("Host", host), field("Port", port)), err);
    const ok = el("button", { class: "btn primary", type: "submit" }, "Save");
    const no = el("button", { class: "btn", type: "button" }, "Cancel");
    no.onclick = close;
    form.append(el("div", { class: "actions" }, no, ok));
    form.onsubmit = async (e) => {
      e.preventDefault();
      const n = Number(port.value);
      if (!Number.isInteger(n) || n < 1 || n > 65535) return void (err.textContent = "The port must be a number from 1 to 65535");
      try {
        await be.proxySave({ ...p, name: name.value, kind: kind.value as Proxy["kind"], host: host.value, port: n });
        await onChange();
        close();
      } catch (x) {
        err.textContent = (x as Error).message;
      }
    };
    return form;
  }, again);
}
