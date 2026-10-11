// Copying files to and from a server: a window to ask what and where, and a small panel with the progress.

import { Backend, TransferStatus, VaultLockedError } from "./api";
import { clear, el, field, makeSelect, openModal, toast } from "./dom";
import { icon } from "./icons";

export interface FilesCtx {
  be: Backend;
  unlock(offerWithout: boolean): Promise<"unlocked" | "without" | "cancel">;
}

/** Paths typed one per line (a pasted list is fine); blank lines and surrounding quotes go. */
export function parsePaths(text: string): string[] {
  return text
    .split("\n")
    .map((l) => l.trim().replace(/^(['"])(.*)\1$/, "$2"))
    .filter(Boolean);
}

/** `file:///home/me/a%20b.txt` (what some desktops hand over when a file is dropped) as a path. */
export function fromFileUrl(p: string): string {
  if (!p.startsWith("file://")) return p;
  try {
    return decodeURIComponent(new URL(p).pathname);
  } catch {
    return p;
  }
}

const lastKey = (server: number, up: boolean) => `oso.files.${up ? "up" : "down"}.${server}`;
const remember = (k: string): string => {
  try {
    return localStorage.getItem(k) ?? "";
  } catch {
    return "";
  }
};
const keep = (k: string, v: string) => {
  try {
    localStorage.setItem(k, v);
  } catch {
    /* fine without it */
  }
};

/** The panel at the bottom right that follows the copies that were started. */
export class Transfers {
  private panel = el("div", { id: "transfers" });
  private timer = 0;
  private shown = new Map<number, TransferStatus>();
  private doneAt = new Map<number, number>();
  /** Ids this window started (copies of an earlier window are not shown). */
  private mine = new Set<number>();

  constructor(private be: Backend) {
    document.body.append(this.panel);
    this.panel.style.display = "none";
  }

  track(id: number) {
    this.mine.add(id);
    if (!this.timer) this.timer = window.setInterval(() => void this.poll(), 500);
    void this.poll();
  }

  private async poll() {
    let list: TransferStatus[] = [];
    try {
      list = (await this.be.transfers()).filter((t) => this.mine.has(t.id));
    } catch {
      return;
    }
    const now = Date.now();
    for (const t of list) {
      this.shown.set(t.id, t);
      if (t.state === "done" && !this.doneAt.has(t.id)) {
        this.doneAt.set(t.id, now);
        toast(`${t.upload ? "Uploaded" : "Downloaded"} ${t.what}`);
      }
      if (t.state === "failed" && !this.doneAt.has(t.id)) {
        this.doneAt.set(t.id, now);
      }
    }
    // a finished copy leaves by itself after a few seconds; a failed one stays until it is closed
    for (const [id, at] of this.doneAt) {
      const t = this.shown.get(id);
      if (t?.state === "done" && now - at > 5000) this.dismiss(id);
    }
    this.draw();
    if (![...this.shown.values()].some((t) => t.state === "running") && this.shown.size === 0) {
      clearInterval(this.timer);
      this.timer = 0;
    }
  }

  private dismiss(id: number) {
    this.shown.delete(id);
    this.doneAt.delete(id);
    this.mine.delete(id);
    void this.be.transferClear(id).catch(() => {});
    this.draw();
  }

  private draw() {
    clear(this.panel);
    this.panel.style.display = this.shown.size ? "block" : "none";
    for (const t of this.shown.values()) {
      const x = el("button", { class: "ib", type: "button", title: t.state === "running" ? "Cancel" : "Close" });
      x.append(icon("close", 13));
      x.onclick = () => this.dismiss(t.id);
      const bar = el("div", { class: "bar" }, el("i", { style: `width:${t.state === "running" ? (t.percent ?? 4) : 100}%` }));
      this.panel.append(
        el(
          "div",
          { class: `xfer ${t.state}`, "data-transfer": String(t.id) },
          el("div", { class: "top" }, icon(t.upload ? "upload" : "download", 14), el("b", {}, t.what), el("span", { class: "st" }, t.state === "running" ? (t.percent != null ? `${t.percent}%` : "starting…") : t.state === "done" ? "done" : "failed"), x),
          t.state === "failed" ? el("div", { class: "err" }, t.message) : bar,
        ),
      );
    }
  }
}

/** The window that asks what to copy and where. `dropped` are paths that were dropped on the terminal. */
export function filesDialog(ctx: FilesCtx, transfers: Transfers, server: { id: number; name: string }, dropped: string[] = []) {
  openModal((close) => {
    const dir = makeSelect([["up", "Upload to the server"], ["down", "Download from the server"]], dropped.length ? "up" : "up", { "data-name": "direction" });
    const local = el("textarea", { rows: "3", spellcheck: "false", name: "local", placeholder: "/home/me/report.pdf" }) as HTMLTextAreaElement;
    local.value = dropped.map(fromFileUrl).join("\n");
    const remote = el("input", { name: "remote", autocomplete: "off", spellcheck: "false" }) as HTMLInputElement;
    const saveIn = el("input", { name: "savein", autocomplete: "off", spellcheck: "false", placeholder: "empty: your Downloads folder" }) as HTMLInputElement;
    const localRow = field("Files or folders on this computer (one per line)", local);
    const remoteLabel = el("label", {}, "");
    const remoteRow = el("div", {}, remoteLabel, remote);
    const saveRow = field("Save in (a folder on this computer)", saveIn);
    const sync = () => {
      const up = dir.value === "up";
      localRow.style.display = up ? "" : "none";
      saveRow.style.display = up ? "none" : "";
      remoteLabel.textContent = up ? "Folder on the server (empty: its home folder)" : "Path on the server (a file or a folder)";
      remote.placeholder = up ? "~/backups" : "/var/log/syslog";
      remote.value = remember(lastKey(server.id, up));
    };
    dir.onchange = sync;
    sync();
    const err = el("div", { class: "err" });
    const form = el("form", {}, el("h2", {}, `Files · ${server.name}`), el("div", { class: "hint", style: "margin:0 0 10px" }, "Drag files from your file manager onto a terminal to upload them. Folders go with everything inside."), field("What to do", dir), localRow, remoteRow, saveRow, err);
    const ok = el("button", { class: "btn primary", type: "submit" }, "Start");
    const no = el("button", { class: "btn", type: "button" }, "Cancel");
    no.onclick = close;
    form.append(el("div", { class: "actions" }, no, ok));
    form.onsubmit = async (e) => {
      e.preventDefault();
      const up = dir.value === "up";
      const paths = up ? parsePaths(local.value) : saveIn.value.trim() ? [saveIn.value.trim()] : [];
      if (up && !paths.length) return void (err.textContent = "Write the file or folder to upload");
      if (!up && !remote.value.trim()) return void (err.textContent = "Write the path of what to download");
      let useVault = true;
      for (;;) {
        try {
          const id = await ctx.be.transferStart(server.id, up, paths, remote.value.trim(), useVault);
          keep(lastKey(server.id, up), remote.value.trim());
          transfers.track(id);
          close();
          return;
        } catch (x) {
          if (x instanceof VaultLockedError) {
            const c = await ctx.unlock(true);
            if (c === "cancel") return;
            useVault = c === "unlocked";
            continue;
          }
          err.textContent = (x as Error).message;
          return;
        }
      }
    };
    return form;
  });
}
