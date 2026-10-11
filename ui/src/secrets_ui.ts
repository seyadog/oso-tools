// The Secrets window: every secret of the vault with its type and profile, to read, copy, change or delete.

import { Backend, SecretKind, SecretMeta } from "./api";
import { clear, confirmBox, el, field, makeSelect, openModal, toast } from "./dom";
import { icon } from "./icons";

export interface Ctx {
  be: Backend;
  /** Profiles that exist: id and name. */
  profiles(): { id: string; name: string }[];
  /** The profile the window is showing. */
  activeProfile(): string;
  lockVault(): Promise<void>;
}

export const KIND_LABEL: Record<SecretKind, string> = { password: "Password", sudo: "sudo password", ssh_key: "SSH key", api_key: "API key", token: "Token", totp: "One-time codes" };
const KINDS = Object.keys(KIND_LABEL) as SecretKind[];
export const STALE_DAYS = 90;

export const daysOld = (updated: number, now = Date.now()) => (updated ? Math.floor((now / 1000 - updated) / 86400) : 0);
export const isStale = (m: SecretMeta, now = Date.now()) => daysOld(m.updated, now) > STALE_DAYS;

/** Which secrets a filter lets through: text in the name, the type, and the profile (`""` any, `"-"` for all-profile ones). */
export function matches(m: SecretMeta, q: string, kind: string, profile: string): boolean {
  if (kind && m.kind !== kind) return false;
  if (profile === "-" ? m.profile !== null : profile && m.profile !== profile && m.profile !== null) return false;
  const text = `${m.name} ${m.env_var ?? ""} ${m.notes}`.toLowerCase();
  return !q || text.includes(q.toLowerCase());
}

let clipTimer = 0;
/** Copies and clears the clipboard 30 seconds later. */
export function copyForAWhile(text: string) {
  void navigator.clipboard?.writeText(text);
  clearTimeout(clipTimer);
  clipTimer = window.setTimeout(() => void navigator.clipboard?.writeText("").catch(() => {}), 30000);
  toast("Copied. The clipboard is cleared in 30 seconds.");
}

export function secretsDialog(ctx: Ctx, preset: { kind?: string; profile?: string } = {}) {
  const shown = new Map<number, string>();
  openModal((close) => {
    const list = el("div", { class: "vault-list" });
    const filter = el("input", { type: "search", placeholder: "Search secrets", autocomplete: "off", spellcheck: "false", name: "secret-search" });
    const kinds = makeSelect([["", "All types"], ...KINDS.map((k): [string, string] => [k, KIND_LABEL[k]])], preset.kind ?? "", { "data-name": "kind-filter" });
    const profiles = makeSelect([["", "All profiles"], ["-", "Shared by all"], ...ctx.profiles().map((p): [string, string] => [p.id, p.name])], preset.profile ?? "", { "data-name": "profile-filter" });
    let all: SecretMeta[] = [];

    const draw = () => {
      clear(list);
      const rows = all.filter((m) => matches(m, filter.value.trim(), kinds.value, profiles.value));
      if (!rows.length) list.append(el("div", { class: "empty" }, all.length ? "No secret matches." : "Nothing here yet. Add a password, an API key, an SSH key or a token."));
      for (const m of rows) {
        const key = m.id;
        const old = isStale(m);
        const where = [KIND_LABEL[m.kind], m.profile ? ctx.profiles().find((p) => p.id === m.profile)?.name ?? m.profile : "all profiles", m.env_var ? `$${m.env_var}` : ""].filter(Boolean).join(" · ");
        const text = el("span", { class: "pw" }, shown.get(key) ?? "••••••••••");
        const btn = (name: string, title: string, f: () => void) => {
          const b = el("button", { class: "ib", type: "button", title, "aria-label": title });
          b.append(icon(name, 15));
          b.onclick = f;
          return b;
        };
        const eye = btn(shown.has(key) ? "eye-off" : "eye", shown.has(key) ? "Hide" : "Show", async () => {
          if (shown.has(key)) shown.delete(key);
          else shown.set(key, await ctx.be.secretReveal(key).catch(() => ""));
          draw();
        });
        const cp = btn("copy", "Copy for 30 seconds", async () => {
          const v = await ctx.be.secretReveal(key).catch(() => null);
          if (v) copyForAWhile(v);
        });
        const ed = btn("edit", "Change", () => {
          close();
          secretForm(ctx, m, () => secretsDialog(ctx));
        });
        const rm = btn("trash", "Delete", async () => {
          if (await confirmBox("Delete this secret?", `“${m.name}” will be deleted from the vault${m.kind === "password" || m.kind === "sudo" ? ", and the servers that used it will ask for it again" : ""}.`, "Delete")) {
            await ctx.be.secretDelete(key);
            shown.delete(key);
          }
          await load();
        });
        list.append(
          el("div", { class: "vrow srow", "data-secret": m.name },
            el("div", { class: "who" }, el("span", { class: "txt" }, el("b", {}, m.name), el("small", {}, where))),
            el("div", { class: "secret" }, text, old ? el("span", { class: "old", title: `Last changed ${daysOld(m.updated)} days ago` }, `${daysOld(m.updated)} days old`) : null, eye, cp, ed, rm),
          ),
        );
      }
    };
    const load = async () => {
      all = await ctx.be.secretList().catch(() => []);
      draw();
    };
    filter.oninput = draw;
    kinds.onchange = draw;
    profiles.onchange = draw;
    void load();

    const add = el("button", { class: "btn", type: "button", "data-act": "new-secret" }, "New secret");
    add.onclick = () => {
      close();
      secretForm(ctx, null, () => secretsDialog(ctx));
    };
    const sudo = el("button", { class: "btn", type: "button", "data-act": "local-sudo", title: "Typed by itself at the password question of sudo in a terminal of this computer" }, "This computer's sudo password");
    sudo.onclick = async () => {
      const { promptBox } = await import("./dom");
      close();
      const pw = await promptBox("sudo password of this computer", "", "password", "Typed at sudo's question in your local terminals, once per command. Leave empty to remove it.");
      if (pw != null) {
        try {
          await ctx.be.vaultSet(0, true, pw || null);
          toast(pw ? "Saved" : "Removed");
        } catch (e) {
          toast((e as Error).message);
        }
      }
      secretsDialog(ctx);
    };
    const lock = el("button", { class: "btn", type: "button" }, "Lock the vault");
    lock.onclick = () => {
      close();
      void ctx.lockVault();
    };
    const change = el("button", { class: "btn", type: "button" }, "Change master password");
    change.onclick = async () => {
      const { promptBox } = await import("./dom");
      const pw = await promptBox("New master password", "", "password", "Everything in the vault is encrypted again under it.");
      if (!pw) return secretsDialog(ctx);
      try {
        await ctx.be.vaultChangeMaster(pw);
        toast("Master password changed");
      } catch (e) {
        toast((e as Error).message);
      }
      secretsDialog(ctx);
    };
    const done = el("button", { class: "btn primary", type: "button" }, "Done");
    done.onclick = close;
    return el("div", { class: "wide" }, el("h2", {}, "Secrets"), el("div", { class: "hint", style: "margin:0 0 10px" }, "Everything here is encrypted, names included. It never leaves this computer unencrypted."), el("div", { class: "filters" }, filter, kinds, profiles), list, el("div", { class: "actions" }, add, sudo, lock, change, done));
  });
}

/** Add (`null`) or change a secret. On a change, leaving the value empty keeps the saved one. */
export function secretForm(ctx: Ctx, existing: SecretMeta | null, back: () => void) {
  const m: SecretMeta = existing ?? { id: 0, name: "", kind: "password", profile: null, env_var: null, agent_visible: false, notes: "", updated: 0 };
  openModal((close) => {
    const name = el("input", { value: m.name, name: "name", autocomplete: "off", spellcheck: "false", placeholder: "anthropic-work" });
    const kind = makeSelect(KINDS.map((k): [string, string] => [k, KIND_LABEL[k]]), m.kind, { "data-name": "kind" });
    const long = () => kind.value === "ssh_key";
    const value = el("input", { type: "password", name: "value", autocomplete: "new-password", spellcheck: "false", placeholder: existing ? "saved — leave empty to keep" : "" });
    const keyText = el("textarea", { name: "value-key", rows: "5", spellcheck: "false", placeholder: existing ? "saved — leave empty to keep" : "-----BEGIN OPENSSH PRIVATE KEY-----" });
    const profile = makeSelect([["", "All profiles"], ...ctx.profiles().map((p): [string, string] => [p.id, p.name])], m.profile ?? "", { "data-name": "profile" });
    const env = el("input", { value: m.env_var ?? "", name: "env", autocomplete: "off", spellcheck: "false", placeholder: "ANTHROPIC_API_KEY (optional)" });
    const agent = el("input", { type: "checkbox", id: "agent-visible", name: "agent" }) as HTMLInputElement;
    agent.checked = existing ? m.agent_visible : false;
    const notes = el("textarea", { name: "notes", rows: "2", spellcheck: "false" }, m.notes);
    const valueRow = el("div", {});
    const sync = () => {
      clear(valueRow);
      valueRow.append(el("label", {}, long() ? "Private key (OpenSSH format)" : kind.value === "totp" ? "Secret (base32) or otpauth:// link" : "Value"), long() ? keyText : value);
    };
    kind.onchange = () => {
      if (!existing) agent.checked = kind.value === "api_key";
      sync();
    };
    if (!existing) agent.checked = false;
    sync();
    const err = el("div", { class: "err" });
    const form = el("form", {}, el("h2", {}, existing ? `Change ${m.name}` : "New secret"), field("Name", name), field("Type", kind), valueRow, field("Profile", profile), field("Environment variable", env), el("div", { class: "hint" }, agent, el("label", { for: "agent-visible", style: "display:inline;margin-left:6px" }, "Terminals where an AI agent can run may get it in their environment")), field("Notes", notes), err);
    const ok = el("button", { class: "btn primary", type: "submit" }, "Save");
    const no = el("button", { class: "btn", type: "button" }, "Cancel");
    no.onclick = () => {
      close();
      back();
    };
    form.append(el("div", { class: "actions" }, no, ok));
    form.onsubmit = async (e) => {
      e.preventDefault();
      const v = long() ? keyText.value.trim() : value.value;
      try {
        await ctx.be.secretSave({ ...m, name: name.value.trim(), kind: kind.value as SecretKind, profile: profile.value || null, env_var: env.value.trim() || null, agent_visible: agent.checked, notes: notes.value }, v || null);
        close();
        back();
      } catch (x) {
        err.textContent = (x as Error).message;
      }
    };
    return form;
  });
}
