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
  s.innerHTML = `<defs><g id="oso-fur"><path d="M300 330 C288 380 292 430 304 458 C316 484 360 482 366 450 C372 410 368 365 360 326 Z"/><path d="M62 320 C40 262 44 188 70 142 C86 114 134 112 148 142 C162 176 154 236 154 282 Z"/><path d="M52 336 C40 272 104 214 196 204 C286 194 352 226 374 274 C388 306 382 350 350 370 C300 392 160 396 100 380 C70 372 56 356 52 336 Z"/><g transform="translate(404 262) rotate(-22)"><circle cx="-30" cy="-54" r="22"/><circle cx="20" cy="-62" r="20"/><ellipse cx="60" cy="52" rx="48" ry="22" transform="rotate(24 60 52)"/><circle cx="0" cy="0" r="62"/><ellipse cx="66" cy="2" rx="52" ry="30"/></g></g></defs><g transform="translate(8 22) scale(.88)"><use href="#oso-fur" fill="#3f2616" stroke="#3f2616" stroke-width="18" stroke-linejoin="round"/><use href="#oso-fur" fill="#7a4f32"/><path d="M60 352 C110 392 250 396 340 372 C300 400 160 408 96 388 C74 380 62 366 60 352 Z" fill="#5f3b25"/><path d="M104 296 C112 250 188 230 252 234 C314 238 350 266 340 304 C330 338 244 350 176 344 C128 340 100 322 104 296 Z" fill="#c99a6a"/><path d="M150 276 C180 256 240 252 290 268" stroke="#b78655" stroke-width="7" fill="none" stroke-linecap="round" opacity=".7"/><ellipse cx="102" cy="168" rx="26" ry="32" fill="#c99a6a"/><circle cx="80" cy="128" r="9" fill="#c99a6a"/><circle cx="101" cy="120" r="9" fill="#c99a6a"/><circle cx="122" cy="124" r="9" fill="#c99a6a"/><ellipse cx="333" cy="452" rx="26" ry="20" fill="#c99a6a"/><path d="M314 476 l-4 14 M332 480 l0 15 M350 476 l4 14" stroke="#efe3cf" stroke-width="7" stroke-linecap="round"/><g transform="translate(404 262) rotate(-22)"><circle cx="-30" cy="-54" r="11" fill="#c99a6a"/><circle cx="20" cy="-62" r="10" fill="#c99a6a"/><ellipse cx="62" cy="26" rx="46" ry="24" fill="#3a1a1a" transform="rotate(16 62 26)"/><ellipse cx="60" cy="40" rx="28" ry="10" fill="#d9707a" transform="rotate(20 60 40)"/><path d="M34 12 l6 14 l7 -13 Z M64 10 l6 14 l7 -13 Z" fill="#f4ecdf"/><ellipse cx="68" cy="-4" rx="50" ry="25" fill="#c99a6a"/><ellipse cx="112" cy="-12" rx="15" ry="11" fill="#241410"/><path d="M12 -20 q14 12 30 0" stroke="#241410" stroke-width="7" fill="none" stroke-linecap="round"/></g></g>`;
  return s;
}
