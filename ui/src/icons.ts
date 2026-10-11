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
  s.innerHTML = `<defs><g id="oso-shape"><!-- arm hanging down --><path d="M318 318 L324 424" stroke-width="54" stroke-linecap="round" fill="none"/><!-- leg up --><path d="M112 300 L104 178" stroke-width="58" stroke-linecap="round" fill="none"/><!-- body --><ellipse cx="236" cy="296" rx="168" ry="86" transform="rotate(-2 236 296)"/><!-- head, ear and snout --><circle cx="388" cy="262" r="72"/><circle cx="356" cy="198" r="28"/><ellipse cx="438" cy="292" rx="42" ry="32"/></g></defs><rect width="512" height="512" rx="112" fill="#1d1d20"/><g transform="translate(0 6)"><!-- outline, then fur on top, so overlapping parts blend --><use href="#oso-shape" fill="#4a2e1d" stroke="#4a2e1d" stroke-width="14" stroke-linejoin="round"/><use href="#oso-shape" fill="#8a5a3a" stroke="#8a5a3a" stroke-width="0"/><!-- same width as the arm/leg strokes, painted as fur --><path d="M318 318 L324 424" stroke="#8a5a3a" stroke-width="54" stroke-linecap="round" fill="none"/><path d="M112 300 L104 178" stroke="#8a5a3a" stroke-width="58" stroke-linecap="round" fill="none"/><!-- belly --><ellipse cx="232" cy="262" rx="110" ry="46" fill="#d2a679" transform="rotate(-2 232 262)"/><!-- paw pads --><circle cx="104" cy="182" r="19" fill="#d2a679"/><circle cx="324" cy="422" r="18" fill="#d2a679"/><!-- inner ear and snout --><circle cx="356" cy="198" r="13" fill="#d2a679"/><ellipse cx="438" cy="292" rx="30" ry="22" fill="#d2a679"/><ellipse cx="458" cy="280" rx="12" ry="9" fill="#2a1a10"/><!-- sleeping eye and mouth --><path d="M384 248 q14 14 28 0" stroke="#2a1a10" stroke-width="7" fill="none" stroke-linecap="round"/><path d="M440 304 q10 7 22 0" stroke="#2a1a10" stroke-width="5" fill="none" stroke-linecap="round"/></g><!-- z z --><g fill="none" stroke="#a096ff" stroke-width="9" stroke-linecap="round" stroke-linejoin="round"><path d="M404 110 h30 l-30 34 h30"/><path d="M448 62 h20 l-20 23 h20"/></g>`;
  return s;
}
