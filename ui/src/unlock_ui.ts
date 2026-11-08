// The small window that only asks for the master password (opened by oso when an AI agent needs the vault and it is locked).
// It closes by itself as soon as the vault is open, whoever opened it.
import { el } from "./dom";
import { logoMark } from "./icons";
import type { Backend } from "./api";

export function mountUnlock(be: Backend, root: HTMLElement, close: () => void) {
  const input = el("input", { type: "password", class: "ul-input", placeholder: "Master password", autocomplete: "off", "aria-label": "Master password" });
  const error = el("div", { class: "ul-error", role: "alert" });
  const go = el("button", { type: "button", class: "ul-go" }, "Unlock");
  let busy = false;
  const submit = async () => {
    if (busy || !input.value) return;
    busy = true;
    go.setAttribute("disabled", "");
    error.textContent = "";
    try {
      await be.vaultUnlock(input.value);
      input.value = "";
      close();
    } catch (e) {
      error.textContent = String((e as Error)?.message ?? e).replace(/^Error:\s*/, "") || "That is not the master password.";
      input.select();
    } finally {
      busy = false;
      go.removeAttribute("disabled");
    }
  };
  go.addEventListener("click", submit);
  input.addEventListener("keydown", (ev) => {
    if (ev.key === "Enter") submit();
    if (ev.key === "Escape") close();
  });
  root.id = "unlock-root"; // not #app: the big window's grid does not apply here
  root.className = "ul";
  root.append(
    el("div", { class: "ul-head" }, logoMark(26), el("div", {}, el("b", {}, "An AI agent needs oso's vault"), el("small", {}, "Type the master password and it carries on by itself."))),
    input,
    error,
    el("div", { class: "ul-row" }, el("button", { type: "button", class: "ul-cancel" }, "Not now"), go),
  );
  root.querySelector(".ul-cancel")!.addEventListener("click", close);
  input.focus();
  // someone unlocked it somewhere else: nothing left to do here
  const watch = window.setInterval(async () => {
    try {
      if ((await be.vaultStatus()).unlocked) {
        window.clearInterval(watch);
        close();
      }
    } catch {
      /* try again */
    }
  }, 1000);
}
