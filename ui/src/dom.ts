// Small helpers to build the interface without a framework.

export function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  attrs: Record<string, string | boolean | undefined> = {},
  ...kids: (Node | string | null | undefined | false)[]
): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (v === undefined || v === false) continue;
    if (k === "class") e.className = String(v);
    else if (v === true) e.setAttribute(k, "");
    else e.setAttribute(k, v);
  }
  for (const kid of kids) if (kid) e.append(kid);
  return e;
}

export function clear(e: Element) {
  while (e.firstChild) e.removeChild(e.firstChild);
}

let toastTimer = 0;
export function toast(msg: string, ms = 3500) {
  let t = document.getElementById("toast");
  if (!t) {
    t = el("div", { id: "toast" });
    document.body.append(t);
  }
  t.textContent = msg;
  t.style.display = "block";
  clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => (t!.style.display = "none"), ms);
}

// ------------------------------------------------------------------ modals

let current: { close: () => void } | null = null;
export const modalOpen = () => current !== null;

/** Shows a dialog; `build` gets `close` and returns the content. Escape closes it. */
export function openModal(build: (close: () => void) => HTMLElement, onClose?: () => void): () => void {
  current?.close();
  let bg = document.getElementById("modal-bg");
  if (!bg) {
    bg = el("div", { id: "modal-bg" });
    document.body.append(bg);
  }
  const root = bg;
  const close = () => {
    if (current?.close !== close) return;
    root.classList.remove("show");
    clear(root);
    current = null;
    window.removeEventListener("keydown", onKey, true);
    onClose?.();
  };
  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      close();
    }
  };
  current = { close };
  clear(root);
  const box = el("div", { class: "modal", role: "dialog" });
  box.append(build(close));
  root.append(box);
  root.classList.add("show");
  root.onmousedown = (e) => {
    if (e.target === root) close();
  };
  window.addEventListener("keydown", onKey, true);
  queueMicrotask(() => (box.querySelector("input, select, button.primary") as HTMLElement | null)?.focus());
  return close;
}

export function field(label: string, input: HTMLElement): HTMLElement {
  const wrap = el("div");
  wrap.append(el("label", {}, label), input);
  return wrap;
}

/** A field with a short line of help under it (what it is for, in plain words). */
export function fieldHelp(label: string, input: HTMLElement, help: string): HTMLElement {
  const wrap = field(label, input);
  wrap.append(el("div", { class: "help" }, help));
  return wrap;
}

export function confirmBox(title: string, text: string, okLabel = "OK"): Promise<boolean> {
  return new Promise((resolve) => {
    let answered = false;
    openModal(
      (close) => {
        const ok = el("button", { class: "btn primary", type: "button" }, okLabel);
        const no = el("button", { class: "btn", type: "button" }, "Cancel");
        ok.onclick = () => {
          answered = true;
          resolve(true);
          close();
        };
        no.onclick = close;
        return el("div", {}, el("h2", {}, title), el("div", {}, text), el("div", { class: "actions" }, no, ok));
      },
      () => !answered && resolve(false),
    );
  });
}

export function promptBox(title: string, value: string, type = "text", hint = ""): Promise<string | null> {
  return new Promise((resolve) => {
    let answered = false;
    openModal(
      (close) => {
        const input = el("input", { type, value, autocomplete: "off", spellcheck: "false" });
        const form = el("form", {}, el("h2", {}, title), input, hint ? el("div", { class: "hint" }, hint) : null);
        const ok = el("button", { class: "btn primary", type: "submit" }, "OK");
        const no = el("button", { class: "btn", type: "button" }, "Cancel");
        no.onclick = close;
        form.append(el("div", { class: "actions" }, no, ok));
        form.onsubmit = (e) => {
          e.preventDefault();
          answered = true;
          resolve(input.value);
          close();
        };
        return form;
      },
      () => !answered && resolve(null),
    );
  });
}

// ------------------------------------------------------------------ select

export interface SelectEl extends HTMLDivElement {
  value: string;
  onchange: (() => void) | null;
}

/**
 * A drop-down made of plain elements. The native one is a separate window of the desktop toolkit that can get
 * stuck or never open inside a dialog; this one lives in the page, so it always works.
 */
export function makeSelect(options: [string, string][], value: string, attrs: Record<string, string> = {}): SelectEl {
  const root = el("div", { class: "select", tabindex: "0", role: "combobox", ...attrs }) as SelectEl;
  const label = el("span", { class: "sel-label" });
  const btn = el("button", { class: "sel-btn", type: "button", tabindex: "-1" }, label, el("span", { class: "sel-arrow" }, "▾"));
  const pop = el("div", { class: "sel-pop" });
  root.append(btn, pop);
  let current = value;
  let open = false;
  let at = 0;
  root.onchange = null;

  const labelOf = (v: string) => options.find(([x]) => x === v)?.[1] ?? "";
  const paint = () => {
    label.textContent = labelOf(current);
    clear(pop);
    options.forEach(([v, text], i) => {
      const item = el("div", { class: `sel-item${v === current ? " cur" : ""}${open && i === at ? " at" : ""}`, "data-value": v }, text);
      item.addEventListener("pointerdown", (e) => {
        e.preventDefault();
        choose(v);
      });
      pop.append(item);
    });
    pop.style.display = open ? "block" : "none";
  };
  const choose = (v: string) => {
    const changed = v !== current;
    current = v;
    setOpen(false);
    if (changed) root.onchange?.();
  };
  const setOpen = (o: boolean) => {
    open = o;
    if (o) at = Math.max(0, options.findIndex(([v]) => v === current));
    paint();
    if (o) pop.querySelector(".at")?.scrollIntoView({ block: "nearest" });
  };

  Object.defineProperty(root, "value", {
    get: () => current,
    set: (v: string) => {
      current = v;
      paint();
    },
  });
  btn.addEventListener("pointerdown", (e) => {
    e.preventDefault();
    root.focus();
    setOpen(!open);
  });
  root.addEventListener("blur", () => open && setOpen(false));
  root.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && open) {
      e.preventDefault();
      e.stopPropagation();
      return setOpen(false);
    }
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (!open) return setOpen(true);
      at = Math.max(0, Math.min(options.length - 1, at + (e.key === "ArrowDown" ? 1 : -1)));
      paint();
      pop.querySelector(".at")?.scrollIntoView({ block: "nearest" });
    } else if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      if (open) choose(options[at][0]);
      else setOpen(true);
    }
  });
  paint();
  return root;
}

/** A small menu at the pointer (right click): `items` run when picked; anything else closes it. */
export function contextMenu(x: number, y: number, items: { label: string; run: () => void; danger?: boolean }[]) {
  document.querySelector(".ctx-menu")?.remove();
  const menu = el("div", { class: "ctx-menu", role: "menu" });
  const close = () => {
    menu.remove();
    window.removeEventListener("pointerdown", away, true);
    window.removeEventListener("keydown", key, true);
  };
  const away = (e: Event) => {
    if (!menu.contains(e.target as Node)) close();
  };
  const key = (e: KeyboardEvent) => {
    if (e.key === "Escape") close();
  };
  for (const it of items) {
    const b = el("button", { class: `ctx-item${it.danger ? " danger" : ""}`, type: "button", role: "menuitem" }, it.label);
    b.onclick = () => {
      close();
      it.run();
    };
    menu.append(b);
  }
  document.body.append(menu);
  const r = menu.getBoundingClientRect();
  menu.style.left = `${Math.max(4, Math.min(x, window.innerWidth - r.width - 4))}px`;
  menu.style.top = `${Math.max(4, Math.min(y, window.innerHeight - r.height - 4))}px`;
  window.addEventListener("pointerdown", away, true);
  window.addEventListener("keydown", key, true);
}
