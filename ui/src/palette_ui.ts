// The command palette window: a search box at the top, the results below in groups with their title and icon, each with
// its keys on the right. Arrows move, Enter runs, Esc closes (the window gives that). What is found and how it is ranked
// is `palette.ts`; what is offered is decided by the app.

import { clear, el, openModal } from "./dom";
import { icon } from "./icons";
import { GroupOut, Item, Kind, Row, flat, rank } from "./palette";

export interface PaletteOptions {
  items: Item[];
  /** Ids of the actions used lately, newest first. */
  recent: string[];
  /** Only these kinds (Alt+K: the servers). */
  only?: Kind[];
  placeholder: string;
  /** What to do with the one chosen (the window is already closed). */
  onRun(item: Item): void;
}

function marked(text: string, at: number[]): (Node | string)[] {
  if (!at.length) return [text];
  const out: (Node | string)[] = [];
  let last = 0;
  for (const i of at) {
    if (i > last) out.push(text.slice(last, i));
    out.push(el("b", { class: "hit" }, text[i]));
    last = i + 1;
  }
  if (last < text.length) out.push(text.slice(last));
  return out;
}

export function openPalette(o: PaletteOptions) {
  openModal(
    (close) => {
      const root = document.getElementById("modal-bg");
      root?.classList.add("palette-mode");
      const input = el("input", { type: "search", placeholder: o.placeholder, autocomplete: "off", spellcheck: "false", role: "combobox", "aria-expanded": "true", "aria-controls": "palette-list", "data-name": "palette-input" }) as HTMLInputElement;
      const list = el("div", { id: "palette-list", class: "palette-list", role: "listbox" });
      let groups: GroupOut[] = [];
      let rows: Row[] = [];
      let at = 0;
      const choose = (r: Row | undefined) => {
        if (!r) return;
        close();
        queueMicrotask(() => o.onRun(r.item));
      };
      const draw = () => {
        groups = rank(o.items, input.value, { recent: o.recent, only: o.only });
        rows = flat(groups);
        at = Math.min(at, Math.max(0, rows.length - 1));
        clear(list);
        let n = 0;
        for (const g of groups) {
          const title = el("div", { class: "pal-title", "data-group": g.kind });
          title.append(icon(g.icon, 13), g.title);
          list.append(title);
          for (const r of g.rows) {
            const idx = n++;
            const row = el("div", { class: `pal-row${idx === at ? " sel" : ""}`, role: "option", "aria-selected": String(idx === at), "data-kind": r.item.kind, "data-id": r.item.id, "data-group": g.kind });
            row.append(el("span", { class: "pal-icon" }), el("span", { class: "pal-txt" }, el("span", { class: "pal-label" }, ...marked(r.item.label, r.at)), r.item.sub ? el("small", {}, r.item.sub) : null));
            (row.firstChild as HTMLElement).append(icon(r.item.icon, 16));
            if (r.item.hotkey) row.append(el("kbd", {}, r.item.hotkey));
            row.onmousemove = () => {
              if (at !== idx) {
                at = idx;
                paintSel();
              }
            };
            row.onclick = () => choose(r);
            list.append(row);
          }
        }
        if (!rows.length) list.append(el("div", { class: "empty" }, input.value.trim() ? "Nothing matches." : "Nothing here yet."));
        paintSel(true);
      };
      const paintSel = (instant = false) => {
        list.querySelectorAll(".pal-row").forEach((r, i) => {
          r.classList.toggle("sel", i === at);
          r.setAttribute("aria-selected", String(i === at));
        });
        list.querySelector(".pal-row.sel")?.scrollIntoView({ block: "nearest", behavior: instant ? "instant" : "auto" });
      };
      input.oninput = () => {
        at = 0;
        draw();
      };
      input.onkeydown = (e) => {
        const n = rows.length;
        if (e.key === "ArrowDown" || (e.key === "n" && e.ctrlKey)) at = n ? (at + 1) % n : 0;
        else if (e.key === "ArrowUp" || (e.key === "p" && e.ctrlKey)) at = n ? (at - 1 + n) % n : 0;
        else if (e.key === "PageDown") at = Math.min(n - 1, at + 6);
        else if (e.key === "PageUp") at = Math.max(0, at - 6);
        else if (e.key === "Home" && e.ctrlKey) at = 0;
        else if (e.key === "End" && e.ctrlKey) at = Math.max(0, n - 1);
        else if (e.key === "Enter") {
          e.preventDefault();
          return choose(rows[at]);
        } else return;
        e.preventDefault();
        paintSel();
      };
      draw();
      return el("div", { class: "palette" }, el("div", { class: "pal-search" }, icon("search", 16), input), list);
    },
    () => document.getElementById("modal-bg")?.classList.remove("palette-mode"),
  );
}
