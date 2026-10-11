// What AI agents ask of oso, for the person to answer: a window per request (they queue), always naming who asks,
// from which terminal, on which host and the exact command. And the record of what was asked.

import { ApprovalDecision, ApprovalRequest, AuditEntry, Backend } from "./api";
import { clear, el, openModal } from "./dom";

export function actionTitle(r: ApprovalRequest): string {
  switch (r.action) {
    case "sudo":
      return `${r.label} wants to use sudo on this computer`;
    case "run_with":
      return `${r.label} wants to run a command with secrets`;
    case "tunnel":
      return `${r.label} wants to change a tunnel`;
    default:
      return `${r.label} wants to run a command`;
  }
}

/** One line for the audit list. */
export function auditLine(e: AuditEntry): string {
  const what = e.command ? `${e.action}: ${e.command}` : e.action;
  const where = e.host ? ` on ${e.host}` : "";
  const code = e.exit_code != null ? ` (exit ${e.exit_code})` : "";
  return `${what}${where} → ${e.decision.replace(/_/g, " ")}${code}`;
}

export interface QueueCtx {
  be: Backend;
  /** Puts the command in a terminal for the person to run themselves (nothing is sent to the agent). */
  openInTerminal(r: ApprovalRequest): Promise<void>;
}

/** Shows requests one at a time, over whatever else is open (it never closes another dialog). */
export class ApprovalQueue {
  private waiting: ApprovalRequest[] = [];
  private shown: ApprovalRequest | null = null;
  private root: HTMLElement | null = null;

  constructor(private ctx: QueueCtx) {}

  push(r: ApprovalRequest) {
    this.waiting.push(r);
    if (!this.shown) this.next();
    else this.draw();
  }

  get pending(): number {
    return this.waiting.length + (this.shown ? 1 : 0);
  }

  private next() {
    this.shown = this.waiting.shift() ?? null;
    this.draw();
  }

  private async answer(r: ApprovalRequest, d: ApprovalDecision) {
    if (this.shown !== r) return;
    this.shown = null;
    if (d === "terminal") await this.ctx.openInTerminal(r).catch(() => {});
    await this.ctx.be.approvalAnswer(r.id, d).catch(() => false);
    this.next();
  }

  private draw() {
    const r = this.shown;
    if (!r) {
      this.root?.remove();
      this.root = null;
      return;
    }
    if (!this.root) {
      this.root = el("div", { id: "approval-bg", class: "show" });
      document.body.append(this.root);
    }
    const root = this.root;
    clear(root);
    const btn = (cls: string, text: string, d: ApprovalDecision, act: string) => {
      const b = el("button", { class: `btn ${cls}`.trim(), type: "button", "data-act": act }, text);
      b.onclick = () => void this.answer(r, d);
      return b;
    };
    const deny = btn("", "Deny", "deny", "approval-deny");
    const actions = el("div", { class: "actions" }, btn("", "Open in a terminal", "terminal", "approval-terminal"), deny, btn("primary", "Allow once", "once", "approval-once"));
    if (r.can_remember) actions.prepend(btn("", "Always for read-only commands on this host", "always_read", "approval-always"));
    const more = this.waiting.length ? el("div", { class: "hint" }, `${this.waiting.length} more waiting after this one`) : null;
    const box = el(
      "div",
      { class: `modal approval${r.production ? " prod" : ""}`, role: "alertdialog", "aria-label": actionTitle(r) },
      el("h2", {}, actionTitle(r)),
      el("div", { class: "where" }, r.host, r.production ? el("span", { class: "badge-prod" }, "PRODUCTION") : null),
      el("pre", { class: "cmd" }, r.command),
      el("div", { class: "hint" }, "It is run only if you allow it. The agent never sees a password or a key."),
      more,
      actions,
    );
    box.addEventListener("keydown", (e) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        void this.answer(r, "deny");
      }
    });
    root.append(box);
    // the safe answer has the focus: pressing Enter by mistake denies
    queueMicrotask(() => deny.focus());
  }
}

/** The last things agents asked, and what became of them. */
export async function auditDialog(be: Backend) {
  const rows = await be.auditRecent(60).catch(() => [] as AuditEntry[]);
  openModal((close) => {
    const list = el("div", { class: "vault-list audit" });
    if (!rows.length) list.append(el("div", { class: "empty" }, "No AI agent has asked for anything yet."));
    for (const e of [...rows].reverse()) {
      list.append(
        el("div", { class: "vrow arow", "data-decision": e.decision },
          el("div", { class: "txt" }, el("b", {}, auditLine(e)), el("small", {}, `${new Date(e.ts * 1000).toLocaleString()} · ${e.agent || "an agent"} · pane ${e.pane}${e.note ? " · " + e.note : ""}`)),
        ),
      );
    }
    const done = el("button", { class: "btn primary", type: "button" }, "Done");
    done.onclick = close;
    return el("div", { class: "wide" }, el("h2", {}, "What AI agents asked"), el("div", { class: "hint", style: "margin:0 0 10px" }, "Kept in audit.jsonl in oso's folder, only ever added to. It holds the commands, never a password or a key."), list, el("div", { class: "actions" }, done));
  });
}
