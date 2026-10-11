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
  s.innerHTML = `<defs><g id="oso-fur"><path d="M30 392 C14 300 56 196 150 168 C236 142 322 176 356 252 C372 296 372 350 366 396 C300 412 100 414 30 392 Z"/><ellipse cx="74" cy="392" rx="50" ry="30"/><circle cx="340" cy="212" r="38"/><circle cx="446" cy="218" r="34"/><circle cx="394" cy="326" r="100"/><ellipse cx="296" cy="392" rx="52" ry="26"/></g></defs><g transform="translate(5 -36) scale(.94) translate(6 18)"><ellipse cx="260" cy="424" rx="230" ry="16" fill="#000" opacity=".18"/><use href="#oso-fur" fill="#3f2616" stroke="#3f2616" stroke-width="18" stroke-linejoin="round"/><use href="#oso-fur" fill="#8a5a3a"/><path d="M96 340 C100 280 170 250 232 258 C296 266 330 304 316 348 C304 384 150 394 108 372 C98 366 95 354 96 340 Z" fill="#d2a679"/><ellipse cx="60" cy="392" rx="22" ry="16" fill="#d2a679"/><ellipse cx="300" cy="396" rx="26" ry="13" fill="#d2a679"/><path d="M276 388 v8 M298 386 v10 M320 388 v8" stroke="#8a5a3a" stroke-width="4" stroke-linecap="round"/><circle cx="340" cy="214" r="19" fill="#d2a679"/><circle cx="446" cy="220" r="17" fill="#d2a679"/><ellipse cx="418" cy="360" rx="56" ry="44" fill="#d2a679"/><ellipse cx="428" cy="392" rx="23" ry="15" fill="#3a1a1a"/><ellipse cx="426" cy="399" rx="13" ry="7" fill="#d9707a"/><ellipse cx="440" cy="336" rx="19" ry="13" fill="#241410"/><path d="M338 306 q16 14 34 0" stroke="#241410" stroke-width="8" fill="none" stroke-linecap="round"/><path d="M410 300 q16 14 34 0" stroke="#241410" stroke-width="8" fill="none" stroke-linecap="round"/><ellipse cx="338" cy="346" rx="16" ry="9" fill="#d98f7a" opacity=".5"/></g>`;
  return s;
}
