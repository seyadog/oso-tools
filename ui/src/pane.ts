// One terminal on screen: an xterm.js emulator inside a frame with a title bar (the title bar shows in mosaic mode).

import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { b64 } from "./api";
import { el } from "./dom";
import { icon } from "./icons";

export interface PaneHandlers {
  onData(sid: number, data: string): void;
  onBinary(sid: number, data: string): void;
  onResize(sid: number, rows: number, cols: number): void;
  onClose(sid: number): void;
  onFocus(sid: number): void;
  onTitlePointerDown(e: PointerEvent, sid: number): void;
}

// The terminal version's colours: the soft pastels over the dark of the window.
const THEME = {
  background: "#1d1d20",
  foreground: "#e4e4e8",
  cursor: "#a096ff",
  cursorAccent: "#1d1d20",
  selectionBackground: "#3d3e52",
  black: "#45475a",
  red: "#ff6e91",
  green: "#82e682",
  yellow: "#ffd764",
  blue: "#6ea8ff",
  magenta: "#be82ff",
  cyan: "#5ad7f5",
  white: "#bac2de",
  brightBlack: "#6c7086",
  brightRed: "#ff8fab",
  brightGreen: "#a3f0a3",
  brightYellow: "#ffe38f",
  brightBlue: "#8fbcff",
  brightMagenta: "#d0a3ff",
  brightCyan: "#82e3f8",
  brightWhite: "#f0f0f4",
};

export class Pane {
  readonly root: HTMLElement;
  readonly titleText: HTMLElement;
  readonly badge: HTMLElement;
  readonly dot: HTMLElement;
  readonly num: HTMLElement;
  readonly banner: HTMLElement;
  readonly term: Terminal;
  private fit = new FitAddon();
  private termEl: HTMLElement;
  private opened = false;
  private resizeObs: ResizeObserver;
  private lastSize = "";

  constructor(
    public sid: number,
    private h: PaneHandlers,
  ) {
    this.titleText = el("span", { class: "t" });
    this.badge = el("span", { class: "bc" }, "BROADCAST");
    this.badge.style.display = "none";
    const close = el("button", { class: "x", type: "button", title: "Close" });
    close.append(icon("close", 14));
    this.dot = el("span", { class: "dot" });
    this.num = el("span", { class: "num" });
    const title = el("div", { class: "title" }, this.num, this.dot, this.titleText, this.badge, close);
    this.termEl = el("div", { class: "term" });
    this.banner = el("div", { class: "dead-banner" }, "Session ended — Enter: reconnect · Alt+W: close");
    const frame = el("div", { class: "frame" }, title, this.termEl, this.banner);
    this.root = el("div", { class: "pane" }, frame);

    title.addEventListener("pointerdown", (e) => {
      if ((e.target as HTMLElement).closest(".x")) return;
      this.h.onTitlePointerDown(e, this.sid);
    });
    close.onclick = () => this.h.onClose(this.sid);
    this.root.addEventListener("pointerdown", () => this.h.onFocus(this.sid), true);

    this.term = new Terminal({
      fontFamily: '"Adwaita Mono", "JetBrains Mono", "Liberation Mono", monospace',
      fontSize: 14,
      cursorBlink: true,
      scrollback: 10000,
      allowProposedApi: true,
      theme: THEME,
      macOptionIsMeta: false,
    });
    this.term.loadAddon(this.fit);
    this.term.onData((d) => this.h.onData(this.sid, d));
    this.term.onBinary((d) => this.h.onBinary(this.sid, d));
    this.term.onResize(({ rows, cols }) => this.h.onResize(this.sid, rows, cols));
    // Selecting text copies it, like most terminals.
    this.term.onSelectionChange(() => {
      const t = this.term.getSelection();
      if (t) navigator.clipboard?.writeText(t).catch(() => {});
    });
    this.term.attachCustomKeyEventHandler((e) => {
      // Ctrl+Shift+V pastes (the browser does it); Ctrl+Shift+C copies the selection.
      if (e.type === "keydown" && e.ctrlKey && e.shiftKey && e.code === "KeyC") {
        const t = this.term.getSelection();
        if (t) navigator.clipboard?.writeText(t).catch(() => {});
        return false;
      }
      return true;
    });
    this.resizeObs = new ResizeObserver(() => this.refit());
    this.resizeObs.observe(this.termEl);
  }

  /** Makes the emulator once the pane is in the page (xterm needs a real element to measure). */
  attach(parent: HTMLElement) {
    parent.append(this.root);
    if (!this.opened) {
      this.term.open(this.termEl);
      this.opened = true;
    }
  }

  setTitle(t: string) {
    this.titleText.textContent = t;
  }

  setNumber(n: number) {
    this.num.textContent = String(n);
  }

  setColor(c: string) {
    this.dot.style.background = c;
  }

  refit() {
    if (!this.opened || !this.root.classList.contains("show")) return;
    const w = this.termEl.clientWidth;
    const h = this.termEl.clientHeight;
    if (w < 20 || h < 20) return;
    const key = `${w}x${h}`;
    if (key === this.lastSize) return;
    this.lastSize = key;
    try {
      this.fit.fit();
    } catch {
      /* not measurable yet */
    }
  }

  forceRefit() {
    this.lastSize = "";
    this.refit();
  }

  /** The screen as the background server holds it: scroll-back first, then the exact screen. */
  snapshot(replay: string, screen: string) {
    this.term.reset();
    if (replay) this.term.write(b64.decode(replay));
    if (screen) this.term.write(b64.decode(screen));
  }

  output(data: string) {
    this.term.write(b64.decode(data));
  }

  focus() {
    this.term.focus();
  }

  dispose() {
    this.resizeObs.disconnect();
    this.term.dispose();
    this.root.remove();
  }
}
