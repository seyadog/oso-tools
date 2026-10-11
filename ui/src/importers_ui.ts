// More ways in: `.env` files of projects, and the export of Proton Pass. Both show names and never values; the
// originals are never deleted unless the person says so, and the first answer is to keep them.

import { Backend, EnvFile, ImportSummary, ProtonItem } from "./api";
import { confirmBox, el, openModal, promptBox, toast } from "./dom";
import { copyForAWhile } from "./secrets_ui";

export interface Ctx {
  be: Backend;
  unlock(): Promise<boolean>;
  refreshTree(): Promise<void>;
}

/** One sentence about what an import did. */
export function sentence(s: ImportSummary): string {
  const skipped = s.skipped.length ? ` ${s.skipped.length} skipped (${s.skipped.slice(0, 2).join("; ")}${s.skipped.length > 2 ? "…" : ""}).` : "";
  return `Imported ${s.added.length} secret${s.added.length === 1 ? "" : "s"}.${skipped}`;
}

/** `[1]` as a list of the ones ticked. */
function checks<T>(items: T[], label: (t: T) => HTMLElement, onChange: (i: number, on: boolean) => void): HTMLElement[] {
  return items.map((it, i) => {
    const c = el("input", { type: "checkbox", "data-pick": String(i) }) as HTMLInputElement;
    c.checked = true;
    c.onchange = () => onChange(i, c.checked);
    return el("label", { class: "vrow irow", "data-row": String(i) }, el("span", { class: "who" }, c, label(it)));
  });
}

export async function envImportDialog(ctx: Ctx, back: () => void = () => {}) {
  if (!(await ctx.unlock())) return back();
  const dir = await promptBox("Folder to look in for .env files", "~/Documents", "text", "It looks inside it (and the folders in it) for .env files with secrets. Names are shown; values never are.");
  if (!dir?.trim()) return back();
  let files: EnvFile[];
  try {
    files = await ctx.be.envPreview(dir.trim());
  } catch (e) {
    toast((e as Error).message);
    return back();
  }
  if (!files.length) {
    toast("No .env file with secrets in it was found there.");
    return back();
  }
  openModal((close) => {
    // `path|name` of each variable still ticked
    const on = new Set(files.flatMap((f) => f.entries.map((e) => `${f.path}|${e.name}`)));
    const err = el("div", { class: "err" });
    const list = el("div", { class: "vault-list" });
    for (const f of files) {
      list.append(el("h3", {}, `${f.project} · ${f.path.split("/").slice(-1)[0]}`, el("small", {}, ` ${f.others} other setting${f.others === 1 ? "" : "s"} not imported`)));
      list.append(
        ...checks(f.entries, (e) => el("span", { class: "txt" }, el("b", {}, e.name), el("small", {}, `${e.kind.replace("_", " ")} · ${e.length} characters`)), (i, v) => {
          const k = `${f.path}|${f.entries[i].name}`;
          if (v) on.add(k);
          else on.delete(k);
        }),
      );
    }
    const go = el("button", { class: "btn primary", type: "button", "data-act": "do-env-import" }, "Import");
    const no = el("button", { class: "btn", type: "button" }, "Cancel");
    no.onclick = () => {
      close();
      back();
    };
    go.onclick = async () => {
      const choices = files.map((f) => ({ path: f.path, names: f.entries.map((e) => e.name).filter((n) => on.has(`${f.path}|${n}`)) })).filter((c) => c.names.length);
      if (!choices.length) return void (err.textContent = "Tick at least one");
      try {
        const r = await ctx.be.envImport(dir.trim(), choices);
        close();
        toast(sentence(r), 8000);
        if (r.originals.length) {
          const del = await confirmBox("Delete the original .env files?", `${r.originals.length} file${r.originals.length === 1 ? " is" : "s are"} now in the vault. oso never deletes them by itself; if you do, the vault is the only copy.`, "Delete the files");
          if (del) toast(`Deleted ${(await ctx.be.envDelete(dir.trim(), r.originals)).length} file(s).`);
        }
        back();
      } catch (e) {
        err.textContent = (e as Error).message;
      }
    };
    return el("div", { class: "wide" }, el("h2", {}, "Secrets in .env files"), el("div", { class: "hint", style: "margin:0 0 10px" }, "They come into the vault as secrets named project/VARIABLE, without an environment variable of their own."), list, err, el("div", { class: "actions" }, no, go));
  });
}

export async function protonImportDialog(ctx: Ctx, back: () => void = () => {}) {
  if (!(await ctx.unlock())) return back();
  const path = await promptBox("Proton Pass export file", "~/Downloads/", "text", "Export from Proton Pass as CSV or JSON, without encryption, and give its path. Delete the export afterwards: it holds your passwords in plain text.");
  if (!path?.trim()) return back();
  let items: ProtonItem[];
  try {
    items = await ctx.be.protonPreview(path.trim());
  } catch (e) {
    toast((e as Error).message, 6000);
    return back();
  }
  openModal((close) => {
    const on = new Set(items.map((_, i) => i));
    const err = el("div", { class: "err" });
    const list = el("div", { class: "vault-list" }, ...checks(items, (it) => el("span", { class: "txt" }, el("b", {}, it.name), el("small", {}, [it.username, it.url, it.has_totp ? "with 2FA" : ""].filter(Boolean).join(" · "))), (i, v) => (v ? on.add(i) : on.delete(i))));
    const go = el("button", { class: "btn primary", type: "button", "data-act": "do-proton-import" }, "Import");
    const no = el("button", { class: "btn", type: "button" }, "Cancel");
    no.onclick = () => {
      close();
      back();
    };
    go.onclick = async () => {
      if (!on.size) return void (err.textContent = "Tick at least one");
      try {
        const r = await ctx.be.protonImport(path.trim(), [...on].sort((a, b) => a - b));
        close();
        toast(sentence(r) + " The export file is still where it was: delete it.", 9000);
        back();
      } catch (e) {
        err.textContent = (e as Error).message;
      }
    };
    return el("div", { class: "wide" }, el("h2", {}, "Import from Proton Pass"), el("div", { class: "hint", style: "margin:0 0 10px" }, `${items.length} login${items.length === 1 ? "" : "s"} found. Passwords are not shown here.`), list, err, el("div", { class: "actions" }, no, go));
  });
}

/** Shows the current one-time code of a seed and copies it for 30 seconds. */
export async function showCode(be: Backend, id: number, name: string) {
  try {
    const { code, seconds_left } = await be.totpCode(id);
    copyForAWhile(code);
    toast(`${name}: ${code.slice(0, 3)} ${code.slice(3)}  (${seconds_left} s left, copied)`, 6000);
  } catch (e) {
    toast((e as Error).message);
  }
}
