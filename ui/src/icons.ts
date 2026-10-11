// Line icons, all on the same 24-unit grid and stroke, so the interface looks like one thing and not like
// emoji and arrows mixed together.

const P: Record<string, string> = {
  plus: '<path d="M12 5v14M5 12h14"/>',
  folder: '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>',
  "folder-plus": '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/><path d="M12 10v6M9 13h6"/>',
  search: '<circle cx="11" cy="11" r="6.5"/><path d="m20 20-4-4"/>',
  edit: '<path d="M4 20h4L19 9l-4-4L4 16z"/><path d="m13.5 6.5 4 4"/>',
  trash: '<path d="M4 7h16M10 11v6M14 11v6M6 7l1 12a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2l1-12M9 7V4h6v3"/>',
  terminal: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="m7 9 3 3-3 3M13 15h4"/>',
  "split-right": '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M12 4v16"/><path d="M16 10v4M14 12h4"/>',
  "split-down": '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 12h18"/><path d="M12 14.5v3M10.5 16h3"/>',
  grid: '<rect x="3" y="3" width="8" height="8" rx="1.5"/><rect x="13" y="3" width="8" height="8" rx="1.5"/><rect x="3" y="13" width="8" height="8" rx="1.5"/><rect x="13" y="13" width="8" height="8" rx="1.5"/>',
  tabs: '<rect x="3" y="6" width="18" height="14" rx="2"/><path d="M3 10h18M9 6v4"/>',
  broadcast: '<circle cx="12" cy="12" r="2"/><path d="M7.8 7.8a6 6 0 0 0 0 8.4M16.2 7.8a6 6 0 0 1 0 8.4M4.9 4.9a10 10 0 0 0 0 14.2M19.1 4.9a10 10 0 0 1 0 14.2"/>',
  snippet: '<path d="m9 7-5 5 5 5M15 7l5 5-5 5M13.5 5l-3 14"/>',
  lock: '<rect x="5" y="11" width="14" height="9" rx="2"/><path d="M8 11V8a4 4 0 0 1 8 0v3"/>',
  unlock: '<rect x="5" y="11" width="14" height="9" rx="2"/><path d="M8 11V8a4 4 0 0 1 7.5-2"/>',
  key: '<circle cx="8" cy="15" r="4"/><path d="m11 12 9-9M16 7l3 3M14 9l2 2"/>',
  settings: '<path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/><circle cx="12" cy="12" r="3"/>',
  eye: '<path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12z"/><circle cx="12" cy="12" r="3"/>',
  "eye-off": '<path d="M3 3l18 18M10.6 5.1A10 10 0 0 1 12 5c6.5 0 10 7 10 7a17 17 0 0 1-3.2 4M6.5 6.6C3.7 8.4 2 12 2 12s3.5 7 10 7a9.8 9.8 0 0 0 4.3-1M9.9 9.9a3 3 0 0 0 4.2 4.2"/>',
  copy: '<rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15V6a2 2 0 0 1 2-2h9"/>',
  close: '<path d="m6 6 12 12M18 6 6 18"/>',
  "chevron-down": '<path d="m6 9 6 6 6-6"/>',
  "chevron-right": '<path d="m9 6 6 6-6 6"/>',
  play: '<path d="M7 5v14l11-7z"/>',
  more: '<circle cx="5" cy="12" r="1.2"/><circle cx="12" cy="12" r="1.2"/><circle cx="19" cy="12" r="1.2"/>',
};

export function icon(name: string, size = 16): SVGElement {
  const s = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  s.setAttribute("viewBox", "0 0 24 24");
  s.setAttribute("width", String(size));
  s.setAttribute("height", String(size));
  s.setAttribute("fill", "none");
  s.setAttribute("stroke", "currentColor");
  s.setAttribute("stroke-width", "1.7");
  s.setAttribute("stroke-linecap", "round");
  s.setAttribute("stroke-linejoin", "round");
  s.setAttribute("aria-hidden", "true");
  s.innerHTML = P[name] ?? "";
  return s;
}

/** The colours oso gives to servers (the same ones the terminal version used): one per server, far apart. */
export const SERVER_COLORS = ["#6ea8ff", "#ff6e91", "#82e682", "#ffa05a", "#be82ff", "#5ad7f5", "#ffd764", "#50e1b9"];
export const serverColor = (id: number) => (id === 0 ? "#6c7086" : SERVER_COLORS[id % SERVER_COLORS.length]);

/** The logo for now: a plain mark that is easy to replace (the real one comes later). */
export function logoMark(size = 48): SVGElement {
  const s = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  s.setAttribute("viewBox", "0 0 512 512");
  s.setAttribute("width", String(size));
  s.setAttribute("height", String(size));
  s.innerHTML = `<rect width="512" height="512" rx="112" fill="#1D2538"/><g transform="translate(0 14)"><path d="M96 300 C84 250 70 204 52 160" stroke="#74482C" stroke-width="54" stroke-linecap="round" fill="none"/><path d="M170 282 C166 236 160 192 150 140" stroke="#74482C" stroke-width="52" stroke-linecap="round" fill="none"/><path d="M262 286 C262 240 270 196 284 150" stroke="#74482C" stroke-width="50" stroke-linecap="round" fill="none"/><path d="M322 296 C332 254 348 214 372 178" stroke="#74482C" stroke-width="48" stroke-linecap="round" fill="none"/><path d="M44 330 C40 262 110 222 214 220 C320 218 388 262 388 330 C388 390 320 412 214 412 C108 412 48 390 44 330 Z" fill="#623E28"/><ellipse cx="212" cy="286" rx="112" ry="62" fill="#C08A62"/><g fill="#C08A62" transform="rotate(-22 50 150)"><ellipse cx="50" cy="150" rx="22" ry="25"/><circle cx="33.0" cy="112.0" r="7.0"/><circle cx="50.0" cy="105.0" r="7.0"/><circle cx="67.0" cy="112.0" r="7.0"/></g><g fill="#C08A62" transform="rotate(-6 148 128)"><ellipse cx="148" cy="128" rx="22" ry="25"/><circle cx="131.0" cy="90.0" r="7.0"/><circle cx="148.0" cy="83.0" r="7.0"/><circle cx="165.0" cy="90.0" r="7.0"/></g><g fill="#C08A62" transform="rotate(8 286 138)"><ellipse cx="286" cy="138" rx="22" ry="25"/><circle cx="269.0" cy="100.0" r="7.0"/><circle cx="286.0" cy="93.0" r="7.0"/><circle cx="303.0" cy="100.0" r="7.0"/></g><g fill="#C08A62" transform="rotate(22 376 168)"><ellipse cx="376" cy="168" rx="22" ry="25"/><circle cx="359.0" cy="130.0" r="7.0"/><circle cx="376.0" cy="123.0" r="7.0"/><circle cx="393.0" cy="130.0" r="7.0"/></g><g transform="translate(404 284) rotate(-62)"><circle cx="-38" cy="-66" r="29" fill="#6C4530"/><circle cx="-38" cy="-66" r="14" fill="#C08A62"/><circle cx="12" cy="-80" r="26" fill="#6C4530"/><circle cx="12" cy="-80" r="12" fill="#C08A62"/><circle r="80" fill="#6C4530"/><ellipse cx="72" cy="6" rx="58" ry="38" fill="#C08A62"/><ellipse cx="120" cy="0" rx="19" ry="14" fill="#1E2633"/><path d="M52 8 C66 -4 100 -4 112 10 C112 34 88 48 66 48 C48 44 44 24 52 8 Z" fill="#3A1A1A"/><ellipse cx="82" cy="34" rx="20" ry="9" fill="#E07177"/></g><path d="M380 252 q20 18 40 0" stroke="#1E2633" stroke-width="10" fill="none" stroke-linecap="round"/><ellipse cx="410" cy="314" rx="22" ry="12" fill="#E07177" opacity=".55"/><circle cx="470" cy="150" r="27" fill="#CFE3FF" opacity=".92"/><path d="M455 140 q5 -10 15 -12" stroke="#fff" stroke-width="6" fill="none" stroke-linecap="round"/></g>`;
  return s;
}
