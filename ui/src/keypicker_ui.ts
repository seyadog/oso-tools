// A small browser for choosing a key file: starts at ~/.ssh, folders open, a click on a file picks its path.
// It only lists names; oso never reads the file here.

import { Backend } from "./api";
import { el } from "./dom";

export function keyBrowser(be: Backend, onPick: (path: string) => void): HTMLElement {
  const box = el("div", { class: "keybrowser", "data-el": "key-browser" });
  const go = async (path: string) => {
    box.replaceChildren();
    try {
      const d = await be.listDir(path);
      const head = el("div", { class: "kb-head" }, el("b", {}, d.shown));
      if (d.parent) {
        const up = el("button", { class: "btn small", type: "button", "data-kb": "up" }, "Up");
        up.onclick = () => void go(d.parent!);
        head.prepend(up);
      }
      box.append(head);
      for (const e of d.entries) {
        const full = `${d.shown}/${e.name}`;
        const row = el("button", { class: `kb-row${e.dir ? " dir" : ""}`, type: "button", "data-kb": e.dir ? "dir" : "file" }, e.dir ? `${e.name}/` : e.name);
        row.onclick = () => (e.dir ? void go(`${d.path}/${e.name}`) : onPick(full));
        box.append(row);
      }
    } catch (x) {
      box.append(el("div", { class: "err" }, (x as Error).message ?? String(x)));
    }
  };
  void go("");
  return box;
}
