// Settings → Sounds: on or off, how loud, and which sound for each thing that calls for you, with a "Test" button.

import { clear, el, field, makeSelect, openModal } from "./dom";
import { DEFAULT_SOUNDS, SoundEvent, SoundPlayer, SoundSettings, SoundSource } from "./sound";

const LABEL: Record<SoundEvent, string> = { finished: "An agent finished, or a terminal you were watching is done", needs: "An agent needs you (a request waiting, or waiting for your answer)" };

export function soundsDialog(player: SoundPlayer) {
  openModal((close) => {
    const draft: SoundSettings = JSON.parse(JSON.stringify(player.settings));
    const enabled = el("input", { type: "checkbox", "data-name": "sound-enabled" }) as HTMLInputElement;
    enabled.checked = draft.enabled;
    enabled.onchange = () => (draft.enabled = enabled.checked);
    const volume = el("input", { type: "range", min: "0", max: "100", "data-name": "sound-volume" }) as HTMLInputElement;
    volume.value = String(Math.round(draft.volume * 100));
    volume.oninput = () => {
      draft.volume = Number(volume.value) / 100;
      pct.textContent = `${volume.value}%`;
    };
    const pct = el("span", { class: "dim" }, `${volume.value}%`);
    const rows = el("div", { class: "sound-rows" });
    const draw = () => {
      clear(rows);
      for (const event of ["finished", "needs"] as SoundEvent[]) {
        const src = draft[event];
        const sel = makeSelect(
          [
            ["builtin", "oso's sound"],
            ["system", "The sound of the system (the desktop's theme)"],
            ["file", "A file…"],
          ],
          src.kind,
          { "data-name": `sound-${event}` },
        );
        const path = el("input", { value: src.kind === "file" ? src.path : "", autocomplete: "off", spellcheck: "false", placeholder: "~/sounds/ding.ogg (ogg, wav, mp3 or flac)", "data-name": `sound-${event}-file` }) as HTMLInputElement;
        path.hidden = src.kind !== "file";
        const set = () => {
          const kind = sel.value;
          draft[event] = (kind === "file" ? { kind: "file", path: path.value.trim() } : { kind }) as SoundSource;
          path.hidden = kind !== "file";
        };
        sel.onchange = set;
        path.oninput = set;
        const test = el("button", { class: "btn small", type: "button", "data-act": `sound-test-${event}` }, "Test");
        test.onclick = () => {
          set();
          // it sounds the way the form says now, before it is saved
          void player.testWith(event, draft);
        };
        rows.append(el("div", { class: "sound-row" }, el("div", { class: "lbl" }, LABEL[event]), el("div", { class: "keyrow", style: "grid-template-columns:1fr auto" }, sel, test), path));
      }
    };
    draw();
    const save = el("button", { class: "btn primary", type: "button", "data-act": "sounds-save" }, "Save");
    const reset = el("button", { class: "btn", type: "button", "data-act": "sounds-defaults" }, "Back to oso's sound");
    const no = el("button", { class: "btn", type: "button" }, "Cancel");
    no.onclick = close;
    reset.onclick = () => {
      Object.assign(draft, JSON.parse(JSON.stringify(DEFAULT_SOUNDS)));
      enabled.checked = draft.enabled;
      volume.value = String(Math.round(draft.volume * 100));
      pct.textContent = `${volume.value}%`;
      draw();
    };
    save.onclick = () => {
      player.update(draft);
      close();
    };
    return el(
      "div",
      { class: "wide sounds-form" },
      el("h2", {}, "Sounds"),
      el("div", { class: "hint", style: "margin:0 0 10px" }, "oso makes a sound when an agent finishes or needs you, and when a terminal you were watching (the bell) is done. It stays quiet while you are looking at that tab, and an agent does not sound again within a few seconds."),
      el("label", { class: "group-check" }, enabled, "Sounds on"),
      field("Volume", el("div", { class: "keyrow", style: "grid-template-columns:1fr 50px" }, volume, pct)),
      rows,
      el("div", { class: "actions" }, reset, no, save),
    );
  });
}
