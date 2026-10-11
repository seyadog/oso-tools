// Profiles: "work", "personal". Switching the profile switches the hosts and secrets that are shown, and the
// secrets and accounts the new terminals start with.

import { Backend, Profile } from "./api";
import { clear, confirmBox, el, makeSelect, openModal, promptBox, toast } from "./dom";
import { icon } from "./icons";

/** The drop-down of a server or a folder: no profile of its own, or one of them. */
export function profileOptions(profiles: Profile[], none: string): [string, string][] {
  return [["", none], ...profiles.map((p): [string, string] => [p.id, p.name])];
}

/** Add, rename and delete profiles. `onChange` runs when the window must draw again. */
export function profilesDialog(be: Backend, profiles: () => Profile[], onChange: () => Promise<void>) {
  openModal((close) => {
    const list = el("div", { class: "plist" });
    const draw = () => {
      clear(list);
      for (const p of profiles()) {
        const btn = (name: string, title: string, f: () => void) => {
          const b = el("button", { class: "ib", type: "button", title, "aria-label": title });
          b.append(icon(name, 15));
          b.onclick = f;
          return b;
        };
        const row = el("div", { class: "vrow prow", "data-profile": p.id }, el("div", { class: "who" }, el("span", { class: "txt" }, el("b", {}, p.name), el("small", {}, p.id === "default" ? "always there" : p.id))));
        const acts = el("div", { class: "secret" });
        acts.append(
          btn("edit", "Rename", async () => {
            const n = await promptBox("Rename profile", p.name);
            if (!n?.trim()) return;
            try {
              await be.profileSave(p.id, n);
              await onChange();
              draw();
            } catch (e) {
              toast((e as Error).message);
            }
          }),
        );
        if (p.id !== "default") {
          acts.append(
            btn("trash", "Delete", async () => {
              if (!(await confirmBox(`Delete “${p.name}”?`, "Its hosts, folders and secrets move to the default profile.", "Delete"))) return;
              try {
                await be.profileDelete(p.id);
                await onChange();
              } catch (e) {
                toast((e as Error).message, 6000);
              }
              draw();
            }),
          );
        }
        row.append(acts);
        list.append(row);
      }
    };
    draw();
    const name = el("input", { name: "new-profile", placeholder: "New profile (work, personal…)", autocomplete: "off", spellcheck: "false" });
    const add = el("button", { class: "btn", type: "submit" }, "Add");
    const form = el("form", { class: "filters", style: "grid-template-columns:1fr auto;margin-top:10px" }, name, add);
    form.onsubmit = async (e) => {
      e.preventDefault();
      if (!name.value.trim()) return;
      try {
        await be.profileSave("", name.value);
        name.value = "";
        await onChange();
        draw();
      } catch (x) {
        toast((x as Error).message);
      }
    };
    const done = el("button", { class: "btn primary", type: "button" }, "Done");
    done.onclick = close;
    return el("div", {}, el("h2", {}, "Profiles"), el("div", { class: "hint", style: "margin:0 0 10px" }, "A profile keeps a set of hosts, secrets and accounts together. Switch it from the top of the host list."), list, form, el("div", { class: "actions" }, done));
  });
}

/** The strip above the host list: which profile is active, a switch to see every profile's hosts, and the manager. */
export function profileBar(be: Backend, state: () => { profiles: Profile[]; active: string; all: boolean }, changed: () => Promise<void>): HTMLElement {
  const { profiles, active, all } = state();
  const bar = el("div", { class: "profile-bar" });
  const sel = makeSelect(profiles.map((p): [string, string] => [p.id, p.name]), active, { "data-name": "active-profile", title: "Active profile" });
  sel.onchange = async () => {
    await be.setActiveProfile(sel.value);
    await changed();
  };
  const eye = el("button", { class: `ib${all ? " on" : ""}`, type: "button", title: all ? "Showing every profile's hosts" : "Show every profile's hosts", "aria-pressed": String(all), "data-act": "view-all" });
  eye.append(icon("eye", 15));
  eye.onclick = async () => {
    await be.setViewAll(!all);
    await changed();
  };
  const more = el("button", { class: "ib", type: "button", title: "Profiles", "data-act": "profiles" });
  more.append(icon("more", 15));
  more.onclick = () => profilesDialog(be, () => state().profiles, changed);
  bar.append(sel, eye, more);
  return bar;
}
