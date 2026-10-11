// Bringing what is in ~/.ssh into oso: the keys go into the vault (without passphrase), the hosts of the config
// become servers. It only reads until the person says import, and it never deletes the originals by itself.

import { Backend, ImportPreview, ImportResult } from "./api";
import { clear, confirmBox, el, openModal, toast } from "./dom";

export interface Ctx {
  be: Backend;
  /** The vault must be open to import. */
  unlock(): Promise<boolean>;
  refreshTree(): Promise<void>;
}

/** One sentence about what an import did. */
export function summary(r: ImportResult): string {
  const k = r.keys_added.length;
  const h = r.hosts_added.length;
  const parts = [`${k} key${k === 1 ? "" : "s"}`, `${h} host${h === 1 ? "" : "s"}`];
  return `Imported ${parts.join(" and ")}.`;
}

export async function importDialog(ctx: Ctx, back: () => void = () => {}) {
  if (!(await ctx.unlock())) return back();
  let preview: ImportPreview;
  try {
    preview = await ctx.be.importPreview();
  } catch (e) {
    toast((e as Error).message);
    return back();
  }
  openModal(
    (close) => {
      const chosenKeys = new Set(preview.keys.filter((k) => !k.unsupported).map((k) => k.path));
      const chosenHosts = new Set(preview.hosts.map((h) => h.alias));
      const pass = new Map<string, HTMLInputElement>();
      const err = el("div", { class: "err" });
      const box = (checked: boolean, f: (on: boolean) => void, label: string) => {
        const c = el("input", { type: "checkbox", "data-pick": label }) as HTMLInputElement;
        c.checked = checked;
        c.onchange = () => f(c.checked);
        return c;
      };
      const keyRows = preview.keys.map((k) => {
        const row = el("div", { class: "vrow irow", "data-key": k.name });
        const c = box(!k.unsupported, (on) => (on ? chosenKeys.add(k.path) : chosenKeys.delete(k.path)), k.name);
        c.disabled = !!k.unsupported;
        const what = el("span", { class: "txt" }, el("b", {}, k.name), el("small", {}, k.unsupported || `${k.algorithm} · ${k.fingerprint}${k.comment ? ` · ${k.comment}` : ""}`));
        row.append(el("label", { class: "who" }, c, what));
        if (k.encrypted && !k.unsupported) {
          const p = el("input", { type: "password", placeholder: "passphrase", autocomplete: "new-password", "data-pass": k.name });
          pass.set(k.path, p);
          row.append(p);
        }
        return row;
      });
      const hostRows = preview.hosts.map((h) => {
        const c = box(true, (on) => (on ? chosenHosts.add(h.alias) : chosenHosts.delete(h.alias)), h.alias);
        const detail = `${h.user ? h.user + "@" : ""}${h.host}${h.port !== 22 ? ":" + h.port : ""}${h.proxy_jump ? " · via " + h.proxy_jump : ""}`;
        return el("div", { class: "vrow irow", "data-host": h.alias }, el("label", { class: "who" }, c, el("span", { class: "txt" }, el("b", {}, h.alias), el("small", {}, detail))));
      });
      const empty = !preview.keys.length && !preview.hosts.length;
      const go = el("button", { class: "btn primary", type: "button", "data-act": "do-import" }, "Import");
      const no = el("button", { class: "btn", type: "button" }, "Cancel");
      no.onclick = () => {
        close();
        back();
      };
      go.onclick = async () => {
        err.textContent = "";
        const req = { keys: [...chosenKeys].map((path) => ({ path, passphrase: pass.get(path)?.value || null })), hosts: [...chosenHosts] };
        try {
          const r = await ctx.be.importApply(req);
          close();
          await ctx.refreshTree();
          await finish(ctx, r, back);
        } catch (e) {
          err.textContent = (e as Error).message;
        }
      };
      const list = el("div", { class: "vault-list" });
      if (preview.keys.length) list.append(el("h3", {}, "Keys"), ...keyRows);
      if (preview.hosts.length) list.append(el("h3", {}, "Hosts"), ...hostRows);
      if (empty) list.append(el("div", { class: "empty" }, "Nothing to import: no id_* keys and no hosts in ~/.ssh/config."));
      for (const s of preview.skipped) list.append(el("div", { class: "hint-row" }, s));
      return el("div", { class: "wide" }, el("h2", {}, "Import from ~/.ssh"), el("div", { class: "hint", style: "margin:0 0 10px" }, "Keys with a passphrase ask for it once; they are kept in the vault without it. Your files are not changed or deleted."), list, err, el("div", { class: "actions" }, no, go));
    },
    undefined,
  );
}

/** What was done, then the question about the originals (the first answer is to keep them). */
async function finish(ctx: Ctx, r: ImportResult, back: () => void) {
  const lines = [summary(r), ...r.problems];
  toast(lines.join(" "), 9000);
  if (r.originals.length) {
    const names = r.originals.map((p) => p.split("/").pop()).join(", ");
    const del = await confirmBox("Delete the original key files?", `${names} ${r.originals.length === 1 ? "is" : "are"} now in the vault. oso never deletes them by itself. If you delete them, the vault is the only copy: keep a backup of it.`, "Delete the files");
    if (del) {
      try {
        const gone = await ctx.be.importDeleteOriginals(r.originals);
        toast(`Deleted ${gone.join(", ")}.`);
      } catch (e) {
        toast((e as Error).message);
      }
    }
  }
  back();
}

// kept for tests of the wording
export const _clear = clear;
