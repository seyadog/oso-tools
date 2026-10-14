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
  onWatch(sid: number): void;
  /** A right click on the terminal: the menu of what can be done with it. */
  onContextMenu(e: MouseEvent, sid: number): void;
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
  readonly watchBtn: HTMLElement;
  private keyCard: HTMLElement | null = null;
  readonly term: Terminal;
  private fit = new FitAddon();
  private termEl: HTMLElement;
  private opened = false;
  isView = false;
  private resizeObs: ResizeObserver;
  private lastSize = "";
  /** When output last arrived (ms since the epoch), 0 if none yet. */
  lastOutputAt = 0;
  /** The next snapshot is of a new session in the same place: keep what is on screen and scroll-back above it. */
  joinOld = false;
  private bannerDefault = "Session ended — Enter: reconnect · Alt+W: close";

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
    this.watchBtn = el("button", { class: "w", type: "button", title: "Tell me when this terminal is done" });
    this.watchBtn.append(icon("bell", 14));
    const title = el("div", { class: "title" }, this.num, this.dot, this.titleText, this.badge, this.watchBtn, close);
    this.termEl = el("div", { class: "term" });
    this.banner = el("div", { class: "dead-banner" }, this.bannerDefault);
    const frame = el("div", { class: "frame" }, title, this.termEl, this.banner);
    this.root = el("div", { class: "pane" }, frame);

    title.addEventListener("pointerdown", (e) => {
      if ((e.target as HTMLElement).closest(".x, .w")) return;
      this.h.onTitlePointerDown(e, this.sid);
    });
    close.onclick = () => this.h.onClose(this.sid);
    this.watchBtn.onclick = () => this.h.onWatch(this.sid);
    this.root.addEventListener("pointerdown", () => this.h.onFocus(this.sid), true);
    this.root.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      this.h.onContextMenu(e, this.sid);
    });

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
    if (!this.opened && !this.isView) {
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

  setWatching(on: boolean) {
    this.watchBtn.classList.toggle("on", on);
    this.watchBtn.title = on ? "Watching: it will tell you when this terminal is done (click to stop)" : "Tell me when this terminal is done";
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
    this.lastOutputAt = Date.now();
    if (this.joinOld) {
      // push what was on screen into the scroll-back, with a line that says where the new connection starts
      this.joinOld = false;
      const at = new Date().toTimeString().slice(0, 5);
      this.term.write(`\r\n\x1b[2m──── reconnected ${at} ────\x1b[0m` + "\r\n".repeat(this.term.rows));
    } else {
      this.term.reset();
      if (replay) this.term.write(b64.decode(replay));
    }
    if (screen) this.term.write(b64.decode(screen));
  }

  /** A card over the terminal that explains something and offers buttons; `null` takes it away. */
  showCard(card: HTMLElement | null) {
    this.keyCard?.remove();
    this.keyCard = card;
    if (card) this.root.querySelector(".frame")!.append(card);
  }

  /** The strip at the bottom of a terminal that is not connected (`null`: the usual one). */
  setBanner(text: string | null) {
    this.banner.textContent = text ?? this.bannerDefault;
  }

  output(data: string) {
    this.lastOutputAt = Date.now();
    this.term.write(b64.decode(data));
  }

  /** Resolves when everything written to the emulator so far has been drawn into its buffer. */
  written(): Promise<void> {
    return new Promise((resolve) => this.term.write("", resolve));
  }

  /** The last `n` lines of the terminal as text, counting only what came after the last reconnection. */
  tail(n: number): string {
    const b = this.term.buffer.active;
    const out: string[] = [];
    const end = b.baseY + b.cursorY + 1; // below the cursor the screen is empty
    for (let i = Math.max(0, end - n); i < end; i++) out.push(b.getLine(i)?.translateToString(true) ?? "");
    const mark = out.map((l) => l.includes("──── reconnected")).lastIndexOf(true);
    return out.slice(mark + 1).join("\n");
  }

  /** The last `n` lines of the screen, as text (kept in the window: only used to guess what an agent is doing). */
  tailLines(n: number): string[] {
    const b = this.term.buffer.active;
    const end = b.baseY + this.term.rows;
    const out: string[] = [];
    for (let i = Math.max(0, end - n); i < end; i++) out.push(b.getLine(i)?.translateToString(true) ?? "");
    return out;
  }

  /** The line the cursor is on (what a prompt looks like when the shell is waiting). */
  cursorLine(): string {
    const b = this.term.buffer.active;
    return b.getLine(b.baseY + b.cursorY)?.translateToString(true) ?? "";
  }

  /** The text selected in the terminal ("" when none). */
  selection(): string {
    return this.term.getSelection();
  }

  /** Types text as a paste (the shell is told it is a paste when it asked for that). */
  paste(text: string) {
    this.term.paste(text);
  }

  /** Makes this pane a page of oso (the console of a cluster) instead of a terminal. */
  setView(node: HTMLElement) {
    this.isView = true;
    this.root.classList.add("view");
    this.termEl.style.display = "none";
    this.root.querySelector(".frame")!.insertBefore(node, this.banner);
  }

  focus() {
    if (!this.isView) this.term.focus();
  }

  dispose() {
    this.resizeObs.disconnect();
    this.term.dispose();
    this.root.remove();
  }
}
