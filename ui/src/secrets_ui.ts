// The Secrets window: every secret of the vault with its type and Space, to read, copy, change or delete.

import { Backend, SecretKind, SecretMeta } from "./api";
import { clear, confirmBox, el, fieldHelp, makeSelect, openModal, toast } from "./dom";
import { icon } from "./icons";

export interface Ctx {
  be: Backend;
  /** The ways of bringing secrets in. */
  importSsh(): void;
  importEnv(): void;
  importProton(): void;
  /** Shows the current code of a one-time-code secret. */
  showCode(id: number, name: string): void;
  /** Spaces that exist: id and name. */
  spaces(): { id: number; name: string }[];
  lockVault(): Promise<void>;
  /** Something about the secrets may have changed (the left panel counts them). */
  changed?(): void;
}

export const KIND_LABEL: Record<SecretKind, string> = { password: "Password", sudo: "sudo password", ssh_key: "SSH key", api_key: "API key", token: "Token", totp: "One-time codes" };
const KINDS = Object.keys(KIND_LABEL) as SecretKind[];
export const STALE_DAYS = 90;

export const daysOld = (updated: number, now = Date.now()) => (updated ? Math.floor((now / 1000 - updated) / 86400) : 0);
export const isStale = (m: SecretMeta, now = Date.now()) => daysOld(m.updated, now) > STALE_DAYS;

/** Which secrets a filter lets through: text in the name, the type, and the Space it lives in (`space` is `""` for any, `"-"` for General, else the id; `own` is the secret's Space, `null` for General). */
export function matches(m: SecretMeta, q: string, kind: string, space: string, own: number | null = null): boolean {
  if (kind && m.kind !== kind) return false;
  if (space === "-" ? own !== null : space && String(own) !== space) return false;
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

export function secretsDialog(ctx: Ctx, preset: { kind?: string; space?: string } = {}) {
  const shown = new Map<number, string>();
  openModal((close) => {
    const list = el("div", { class: "vault-list" });
    const filter = el("input", { type: "search", placeholder: "Search secrets", autocomplete: "off", spellcheck: "false", name: "secret-search" });
    const kinds = makeSelect([["", "All types"], ...KINDS.map((k): [string, string] => [k, KIND_LABEL[k]])], preset.kind ?? "", { "data-name": "kind-filter" });
    const spaces = makeSelect([["", "All Spaces"], ["-", "General"], ...ctx.spaces().map((p): [string, string] => [String(p.id), p.name])], preset.space ?? "", { "data-name": "space-filter" });
    let all: SecretMeta[] = [];
    let where: Record<number, number> = {};

    const draw = () => {
      clear(list);
      const rows = all.filter((m) => matches(m, filter.value.trim(), kinds.value, spaces.value, where[m.id] ?? null));
      if (!rows.length) {
        const none = el("div", { class: "empty-box" }, el("div", {}, all.length ? "No secret matches." : "You have no secrets yet. Add your first API key."));
        if (!all.length) {
          const b = el("button", { class: "btn primary", type: "button", "data-act": "empty-new-key" }, "Add an API key");
          b.onclick = () => {
            close();
            secretForm(ctx, null, () => secretsDialog(ctx), { kind: "api_key", agent_visible: true });
          };
          none.append(b);
        }
        list.append(none);
      }
      for (const m of rows) {
        const key = m.id;
        const old = isStale(m);
        const wherein = where[m.id] !== undefined ? ctx.spaces().find((p) => p.id === where[m.id])?.name ?? "a Space" : "General";
        const detail = [KIND_LABEL[m.kind], wherein, m.env_var ? `$${m.env_var}` : ""].filter(Boolean).join(" · ");
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
        const code = m.kind === "totp" ? btn("key", "Show the code now", () => ctx.showCode(m.id, m.name)) : null;
        if (code) code.setAttribute("data-act", "totp-code");
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
            el("div", { class: "who" }, el("span", { class: "txt" }, el("b", {}, m.name), el("small", {}, detail))),
            el("div", { class: "secret" }, text, old ? el("span", { class: "old", title: `Last changed ${daysOld(m.updated)} days ago` }, `${daysOld(m.updated)} days old`) : null, code, eye, cp, ed, rm),
          ),
        );
      }
    };
    const load = async () => {
      all = await ctx.be.secretList().catch(() => []);
      where = await ctx.be.secretSpaces().catch(() => ({}));
      draw();
    };
    filter.oninput = draw;
    kinds.onchange = draw;
    spaces.onchange = draw;
    void load();

    const add = el("button", { class: "btn primary", type: "button", "data-act": "new-secret" }, "New secret");
    add.onclick = () => {
      close();
      secretForm(ctx, null, () => secretsDialog(ctx));
    };
    const sudo = el("button", { class: "btn", type: "button", "data-act": "local-sudo", title: "Typed by itself at the password question of sudo in a terminal of this computer" }, "Sudo password");
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
    const imp = el("button", { class: "btn", type: "button", "data-act": "import" }, "Import…");
    imp.onclick = () => {
      close();
      importChooser(ctx);
    };
    const lock = el("button", { class: "btn", type: "button" }, "Lock vault");
    lock.onclick = () => {
      close();
      void ctx.lockVault();
    };
    const change = el("button", { class: "btn", type: "button" }, "Master password");
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
    const done = el("button", { class: "btn", type: "button" }, "Done");
    done.onclick = close;
    return el("div", { class: "wide" }, el("h2", {}, "Secrets"), el("div", { class: "hint", style: "margin:0 0 10px" }, "API keys, passwords, SSH keys, tokens and one-time codes. Everything here is encrypted, names included, and never leaves this computer unencrypted."), el("div", { class: "filters" }, filter, kinds, spaces), list, el("div", { class: "actions" }, add, imp, sudo, lock, change, done));
  }, () => ctx.changed?.());
}

/** Where secrets can come from. */
function importChooser(ctx: Ctx) {
  openModal((close) => {
    const way = (act: string, title: string, text: string, f: () => void) => {
      const b = el("button", { class: "btn way", type: "button", "data-act": act }, el("b", {}, title), el("small", {}, text));
      b.onclick = () => {
        close();
        f();
      };
      return b;
    };
    const back = el("button", { class: "btn", type: "button" }, "Back");
    back.onclick = () => {
      close();
      secretsDialog(ctx);
    };
    return el("div", {}, el("h2", {}, "Import"), el("div", { class: "ways" },
      way("import-ssh", "SSH keys and hosts", "from ~/.ssh: keys go into the vault, hosts into the list", () => ctx.importSsh()),
      way("import-env", ".env files", "secrets in your projects' .env files", () => ctx.importEnv()),
      way("import-proton", "Proton Pass", "the CSV or JSON export", () => ctx.importProton()),
    ), el("div", { class: "actions" }, back));
  });
}

/** Add (`null`) or change a secret. On a change, leaving the value empty keeps the saved one. */
export function secretForm(ctx: Ctx, existing: SecretMeta | null, back: () => void, preset: Partial<SecretMeta> & { space?: string } = {}) {
  const m: SecretMeta = existing ?? { id: 0, name: "", kind: "password", env_var: null, agent_visible: false, notes: "", updated: 0, ...preset };
  openModal((close) => {
    const name = el("input", { value: m.name, name: "name", autocomplete: "off", spellcheck: "false", placeholder: "anthropic-work" });
    const kind = makeSelect(KINDS.map((k): [string, string] => [k, KIND_LABEL[k]]), m.kind, { "data-name": "kind" });
    const long = () => kind.value === "ssh_key";
    const value = el("input", { type: "password", name: "value", autocomplete: "new-password", spellcheck: "false", placeholder: existing ? "saved — leave empty to keep" : "" });
    const keyText = el("textarea", { name: "value-key", rows: "5", spellcheck: "false", placeholder: existing ? "saved — leave empty to keep" : "-----BEGIN OPENSSH PRIVATE KEY-----" });
    const space = makeSelect([["", "General (every Space)"], ...ctx.spaces().map((p): [string, string] => [String(p.id), p.name])], preset.space ?? "", { "data-name": "space" });
    if (existing) void ctx.be.secretSpaces().then((w) => (space.value = w[existing.id] !== undefined ? String(w[existing.id]) : "")).catch(() => {});
    const env = el("input", { value: m.env_var ?? "", name: "env", autocomplete: "off", spellcheck: "false", placeholder: "ANTHROPIC_API_KEY (optional)" });
    const agent = el("input", { type: "checkbox", id: "agent-visible", name: "agent" }) as HTMLInputElement;
    agent.checked = existing ? m.agent_visible : m.agent_visible;
    const notes = el("textarea", { name: "notes", rows: "2", spellcheck: "false" }, m.notes);
    const valueRow = el("div", {});
    const sync = () => {
      clear(valueRow);
      valueRow.append(el("label", {}, long() ? "Private key (OpenSSH format)" : kind.value === "totp" ? "Secret (base32) or otpauth:// link" : "Value"), long() ? keyText : value, el("div", { class: "help" }, long() ? "Paste the private key, without a passphrase. It stays in the vault." : kind.value === "totp" ? "The text next to the QR code of the service, or the otpauth:// link." : "The secret itself. It is encrypted and not shown again unless you ask."));
    };
    kind.onchange = () => {
      if (!existing) agent.checked = kind.value === "api_key";
      sync();
    };
    sync();
    const err = el("div", { class: "err" });
    const visible = el("div", { class: "check" }, agent, el("label", { for: "agent-visible" }, "Visible to AI agents"));
    visible.append(el("div", { class: "help" }, "If on, the terminals of this Space (or of any Space, for a General one) where an AI tool runs get it in their environment. If off, only you (and tools you approve one by one) can use it."));
    const form = el("form", {}, el("h2", {}, existing ? `Change ${m.name}` : "New secret"), fieldHelp("Name", name, "How you find it in the list. Anything you like."), fieldHelp("Type", kind, "API key, password, SSH key, token, or the seed of one-time codes."), valueRow, fieldHelp("Space", space, "Which Space can use it. General is available in every Space."), fieldHelp("Environment variable", env, "The name a program reads it by, like ANTHROPIC_API_KEY. Leave it empty if programs do not need it."), visible, fieldHelp("Notes", notes, "Anything to remember about it (where it comes from, when it expires). Encrypted too."), err);
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
        const id = await ctx.be.secretSave({ ...m, name: name.value.trim(), kind: kind.value as SecretKind, env_var: env.value.trim() || null, agent_visible: agent.checked, notes: notes.value }, v || null);
        await ctx.be.moveToSpace("secret", id, space.value ? Number(space.value) : null);
        close();
        back();
      } catch (x) {
        err.textContent = (x as Error).message;
      }
    };
    return form;
  });
}
