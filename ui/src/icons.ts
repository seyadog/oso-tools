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
  agent: '<rect x="5" y="8" width="14" height="11" rx="3"/><path d="M12 4v4M9 13h.01M15 13h.01M9.5 16.5h5"/>',
  layers: '<path d="m12 3 9 5-9 5-9-5z"/><path d="m3 13 9 5 9-5M3 17.5l9 5 9-5"/>',
  server: '<rect x="3" y="4" width="18" height="6.5" rx="1.8"/><rect x="3" y="13.5" width="18" height="6.5" rx="1.8"/><path d="M7 7.25h.01M7 16.75h.01"/>',
  user: '<circle cx="12" cy="8" r="3.8"/><path d="M4.5 20a7.5 7.5 0 0 1 15 0"/>',
  focus: '<path d="M4 9V5h4M20 9V5h-4M4 15v4h4M20 15v4h-4"/>',
  "chevron-left": '<path d="m15 6-6 6 6 6"/>',
  "chevron-right": '<path d="m9 6 6 6-6 6"/>',
  bell: '<path d="M6 17V11a6 6 0 0 1 12 0v6l1.5 2h-15z"/><path d="M10 21a2 2 0 0 0 4 0"/>',
  tunnel: '<path d="M3 18V12a9 9 0 0 1 18 0v6"/><path d="M3 18h5v-6a4 4 0 0 1 8 0v6h5"/>',
  download: '<path d="M12 4v11M7 11l5 5 5-5M5 20h14"/>',
  upload: '<path d="M12 16V5M7 9l5-5 5 5M5 20h14"/>',
  files: '<path d="M6 3h8l4 4v14H6z"/><path d="M14 3v4h4M9 13h6M9 17h6"/>',
  save: '<path d="M5 4h11l3 3v13H5z"/><path d="M8 4v5h7V4M8 20v-6h8v6"/>',
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

/** The oso mark (a bear-shaped shield with a keyhole nose); `oso-small.svg` is the simplified one for 32 px or less. */
export function logoMark(size = 48): SVGElement {
  const s = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  const small = size <= 32;
  s.setAttribute("viewBox", small ? "0 0 100 100" : "14 9 72 85");
  s.setAttribute("height", String(size));
  s.setAttribute("width", String(small ? size : Math.round((size * 72) / 85)));
  s.innerHTML = small
    ? `<rect width="100" height="100" rx="22" fill="#232327"/>
  <path d="M16 30 L16 21 Q16 11 26 11 Q35 11 36 20 L64 20 Q65 11 74 11 Q84 11 84 21 L84 52 Q84 77 50 92 Q16 77 16 52 Z" fill="#a096ff"/>
  <path d="M28 41 L45 45 L43 52 L29 49 Z" fill="#1d1d20"/>
  <path d="M72 41 L55 45 L57 52 L71 49 Z" fill="#1d1d20"/>
  <path d="M50 53 L64 65 L50 82 L36 65 Z" fill="#e4e4e8"/>
  <circle cx="50" cy="63" r="5" fill="#1d1d20"/>`
    : `<path d="M18 30 L18 22 Q18 13 27 13 Q35 13 36 21 L64 21 Q65 13 73 13 Q82 13 82 22 L82 52 Q82 76 50 90 Q18 76 18 52 Z" fill="#a096ff"/><path d="M50 21 L64 21 Q65 13 73 13 Q82 13 82 22 L82 52 Q82 76 50 90 Z" fill="#8b7ff2"/><path d="M23 24 Q23 18 27.5 18 Q31.5 18 31.5 22.5 Z" fill="#8b7ff2"/><path d="M77 24 Q77 18 72.5 18 Q68.5 18 68.5 22.5 Z" fill="#6f62e6"/><path d="M30 43 L44 46 L42 51 L31 48 Z" fill="#1d1d20" stroke="#1d1d20" stroke-width="1.2" stroke-linejoin="round"/><path d="M70 43 L56 46 L58 51 L69 48 Z" fill="#1d1d20" stroke="#1d1d20" stroke-width="1.2" stroke-linejoin="round"/><path d="M50 53 L62 64 L50 80 L38 64 Z" fill="#e4e4e8" stroke="#e4e4e8" stroke-width="2" stroke-linejoin="round"/><circle cx="50" cy="61" r="3.3" fill="#1d1d20"/><path d="M48.4 62.5 L51.6 62.5 L52.6 69 L47.4 69 Z" fill="#1d1d20"/>`;
  return s;
}
