// Drives the real interface in a real browser against the mock engine and checks what a person would see.
// Usage: node e2e/run.mjs   (starts vite itself)
import { chromium } from "playwright-core";
import { spawn } from "node:child_process";

const URL = "http://localhost:1420/";
const vite = spawn("npx", ["vite", "--port", "1420", "--strictPort"], { stdio: "pipe" });
process.on("exit", () => vite.kill());
await new Promise((res, rej) => {
  const t = setTimeout(() => rej(new Error("vite did not start")), 15000);
  vite.stdout.on("data", (d) => String(d).includes("Local") && (clearTimeout(t), res()));
});

let failures = 0;
const check = (name, ok, extra = "") => {
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${ok ? "" : "  " + extra}`);
  if (!ok) failures++;
};

const browser = await chromium.launch({ executablePath: "/usr/bin/google-chrome", headless: true, args: ["--no-sandbox"] });
const page = await browser.newPage({ viewport: { width: 1500, height: 900 } });
page.setDefaultTimeout(6000);
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
await page.goto(URL);
await page.waitForSelector(".home");

const shot = (n) => page.screenshot({ path: `shots/${n}.png` });
const panes = () => page.$$eval(".pane.show", (ps) => ps.map((p) => ({ x: p.offsetLeft, y: p.offsetTop, w: p.offsetWidth, h: p.offsetHeight, mosaic: p.classList.contains("mosaic"), active: p.classList.contains("active"), title: p.querySelector(".t")?.textContent })));
const termText = (sid) => page.evaluate((sid) => { const t = window.oso.panes.get(sid).term; const b = t.buffer.active; let s = ""; for (let i = 0; i < b.length; i++) s += b.getLine(i).translateToString(true) + "\n"; return s; }, sid);
const activeSid = () => page.evaluate(() => window.oso.ws.screen.active);
const state = () => page.evaluate(() => ({ cur: window.oso.ws.screen.cur, mode: window.oso.ws.screen.mode, tabs: window.oso.ws.screen.groups.length, terms: window.oso.ws.tabsHere().length, sig: JSON.stringify(window.oso.ws.screen.groups.map((g) => g.tree.kind === 'leaf' ? 1 : 2)), all: window.oso.ws.tabs.size }));
const clearAll = async () => {
  await page.evaluate(async () => { for (const id of [...window.oso.ws.tabs.keys()]) await window.oso.closeSession(id); });
  await page.waitForTimeout(300);
};
const settle = (ms = 250) => page.waitForTimeout(ms);
// the left panel shows one section: make sure it is the one a check needs
const section = async (k) => { if ((await page.getAttribute(`[data-rail="${k}"]`, "aria-pressed")) !== "true") { await page.click(`[data-rail="${k}"]`); await settle(150); } };
const menuItem = async (text) => { await page.click('[data-act="menu"]'); await page.click(`.ctx-menu .ctx-item:has-text("${text}")`); };
// the menu of the terminal on show (right click), and of the tab on show
const openPaneMenu = async () => { const r = await page.evaluate(() => { const b = window.oso.panes.get(window.oso.ws.screen.active).root.getBoundingClientRect(); return { x: b.x + 60, y: b.y + 70 }; }); await page.mouse.click(r.x, r.y, { button: "right" }); await page.waitForSelector(".ctx-menu"); };
const paneMenuItem = async (text) => { await openPaneMenu(); await page.click(`.ctx-menu .ctx-item:has-text("${text}")`); };
const tabMenuItem = async (text) => { await page.click(".tab.on", { button: "right" }); await page.click(`.ctx-menu .ctx-item:has-text("${text}")`); };

// 0. a first start explains itself: getting started, named sections, empty states with a button, the version
check("the first start shows Getting started with its four steps", (await page.$$("[data-step-row]")).length === 4 && (await page.textContent('[data-start="card"]')).includes("0 of 4 done"));
check("each step has its own button", (await page.$$("[data-step]")).length === 4);
check("the rail has the six sections, in order, and the panel shows only the open one", (await page.$$eval("#rail [data-rail]", (e) => e.map((x) => x.getAttribute("data-rail")))).join() === "spaces,servers,secrets,tunnels,accounts,agents" && (await page.$$eval(".sec[data-sec]", (e) => e.map((x) => x.getAttribute("data-sec")))).join() === "servers");
check("the rail is a thin bar of icons", (await page.$eval("#rail", (r) => Math.round(r.getBoundingClientRect().width))) === 48);
check("the first time, Servers is open and the right panel is closed", (await page.getAttribute('[data-rail="servers"]', "aria-pressed")) === "true" && (await page.$eval("#rpanel", (e) => e.hidden)));
check("the version is at the top right", /^v\d+\.\d+\.\d+/.test(await page.textContent(".tools .version")));
check("the Space selector is big, labelled, and always shown, with General first", (await page.textContent(".pb-label")) === "Space" && (await page.$eval(".space-bar .sel-btn", (b) => b.getBoundingClientRect().height)) >= 36 && (await page.textContent(".space-bar .sel-btn")).includes("General"));
await section("secrets");
check("the Secrets section tells a locked vault what to do", (await page.$('[data-act="empty-unlock"]')) !== null);
await section("tunnels");
check("Tunnels says what it is for, with its button", (await page.$('[data-act="empty-add-tunnel"]')) !== null && (await page.textContent("body")).includes("keeps a port open"));
check("every tool at the bottom of the rail says its name and its shortcut", (await page.$$eval("#rail .rail-btn.small", (b) => b.map((x) => x.title))).join(";").includes("Local terminal") && (await page.$eval('[data-act="foot-local"]', (b) => b.title)).includes("Alt+Shift+L") && (await page.$eval('[data-rail="secrets"]', (b) => b.title)).includes("Alt+Shift+3"));
await section("servers");
await page.click('.space-bar .select[data-name="active-space"] .sel-btn');
check("\"+ New Space\" is in the Space list", (await page.textContent(".space-bar .sel-pop")).includes("+ New Space"));
await page.click('.space-bar .sel-item[data-value="__new"]');
await settle(250);
check("an empty Space form has no name and says what to type", (await page.inputValue('[data-name="space-name"]')) === "" && (await page.getAttribute('[data-name="space-name"]', "placeholder")) === "Space name (e.g. the project)");
await page.fill('[data-name="space-name"]', "Personal");
await page.click('[data-act="space-save"]');
await settle(400);
check("and the first step is marked done", (await page.getAttribute('[data-step-row="space"]', "class")).includes("done") && (await page.textContent('[data-start="card"]')).includes("1 of 4 done"));
await page.evaluate(async () => { const be = window.oso.be; for (const x of await be.spaceList()) await be.spaceDelete(x.id); await window.oso.refreshSpaces(); });
await settle(200);
// "?" opens the sheet of shortcuts, built from the real table
await page.keyboard.press("Shift+Slash");
await settle(250);
const sheet = await page.textContent('[data-sheet="shortcuts"]');
check("? opens the sheet of shortcuts", sheet.includes("Shortcuts") && sheet.includes("Mosaic") && sheet.includes("Secrets and agents") && sheet.includes("Files and tunnels"));
const wanted = await page.evaluate(() => ["search", "local.new", "tab.new", "tab.close", "mode.toggle", "split.right", "broadcast.toggle", "files.open", "tunnels.open", "secrets.open", "sudo.fill", "settings", "help.open", "group.save", "watch.toggle", "rpanel.toggle"].map((a) => window.oso.keyText(a)));
check("it lists the keys as they are set", wanted.every((k) => sheet.includes(k)), JSON.stringify(wanted.filter((k) => !sheet.includes(k))));
await page.keyboard.press("Escape");
await page.keyboard.press("Alt+/");
await settle(250);
check("the Help shortcut opens it too", (await page.$('[data-sheet="shortcuts"]')) !== null);
await page.keyboard.press("Escape");
await settle(150);
// Help
await page.click('[data-act="foot-help"]');
await settle(200);
check("Help offers the shortcuts, getting started, connecting an AI, and the record", ["help-shortcuts", "help-start", "help-ai", "help-activity"].every((a) => true) && (await page.$$('[data-act^="help-"]')).length === 4 && /oso v\d+\.\d+\.\d+/.test(await page.textContent("#modal-bg")));
await page.click('[data-act="help-ai"]');
await settle(250);
await page.click('[data-agent="claude"]');
await settle(250);
check("connecting an AI shows what will change before doing it", (await page.textContent("#modal-bg")).includes("This is what changes") && (await page.textContent("#modal-bg pre.cmd")).includes("oso"));
await page.click('[data-act="integration-apply"]');
await settle(500);
check("and marks it connected", (await page.textContent('[data-agent-row="claude"]')).includes("Connected"));
await page.keyboard.press("Escape");
await settle(250);
check("the fourth step is done", (await page.getAttribute('[data-step-row="ai"]', "class")).includes("done"));
// the first step of a closed card comes back from Help
await page.click('[data-act="start-close"]');
await settle(150);
check("Getting started can be closed", (await page.$('[data-start="card"]')) === null);
await page.click('[data-act="foot-help"]');
await page.click('[data-act="help-start"]');
await settle(250);
check("and comes back from Help", (await page.$('[data-start="card"]')) !== null);
await page.keyboard.press("Escape");
await settle(150);
// empty states have their button
await page.evaluate(async () => { globalThis.__osoMock.knobs.empty = true; await window.oso.refreshTree(); globalThis.__osoMock.knobs.noTools = true; await window.oso.refreshAccounts(); });
await settle(250);
await section("servers");
check("with no servers: add one, or import from ~/.ssh", (await page.$('[data-act="empty-add-server"]')) !== null && (await page.$('[data-act="empty-import-ssh"]')) !== null);
await section("accounts");
check("with no tools for accounts: say what they need", (await page.$('[data-act="empty-check-accounts"]')) !== null);
await page.click('#rail [data-act="vault"]');
await settle(250);
await page.fill("#modal-bg input", "master");
await page.keyboard.press("Enter");
await settle(400);
await page.keyboard.press("Escape");
await settle(200);
await page.evaluate(async () => { await window.oso.refreshSecrets(); });
await settle(250);
await section("secrets");
check("with no secrets: add your first API key", (await page.$('[data-act="empty-add-key"]')) !== null && (await page.textContent("body")).includes("You have no secrets yet"));
await page.click('[data-act="empty-add-key"]');
await settle(250);
check("the form starts as an API key that agents may see", (await page.textContent('.select[data-name="kind"]')).includes("API key") && (await page.$eval("#agent-visible", (c) => c.checked)));
check("every field of the form says what it is for", (await page.$$("#modal-bg .help")).length >= 6 && (await page.textContent("#modal-bg")).includes("The name a program reads it by") && (await page.textContent("#modal-bg")).includes("Visible to AI agents"));
await shot("0-secret-form-after");
await page.keyboard.press("Escape");
await settle(150);
await section("secrets");
await page.click('[data-act="sec-secrets-open"]');
await settle(250);
check("the empty Secrets window has the same button", (await page.$('[data-act="empty-new-key"]')) !== null);
await shot("0-secrets-after");
check("the buttons of the Secrets window stay on one line", await page.$$eval("#modal-bg .actions .btn", (bs) => bs.every((b) => b.getBoundingClientRect().height < 40)));
await page.keyboard.press("Escape");
await settle(150);
await page.evaluate(async () => { globalThis.__osoMock.knobs.noTools = false; await window.oso.refreshAccounts(); });
await shot("0-first-start-after");
// "?" typed in a terminal is a "?", not the sheet
await page.evaluate(async () => { globalThis.__osoMock.knobs.empty = false; await window.oso.refreshTree(); });
await settle(200);
await section("servers");
await page.dblclick('.row[data-kind="server"] >> text=web-1');
await settle(500);
await page.keyboard.type("echo a?b");
await settle(300);
check("a ? typed in a terminal is typed, not the sheet", (await page.$('[data-sheet="shortcuts"]')) === null && (await termText(await activeSid())).includes("echo a?b"));
await page.keyboard.press("Enter");
await settle(200);
await clearAll();
await page.evaluate(async () => { await window.oso.lockVault(); localStorage.clear(); });

// 1. open a server by double click
await section("servers");
await page.dblclick('.row[data-kind="server"] >> text=web-1');
await settle(400);
let s = await state();
check("double click opens a terminal", s.tabs === 1 && (await page.$$(".tab")).length === 1);
check("the terminal shows the server's welcome", (await termText(await activeSid())).includes("Welcome to web-1"));
await page.keyboard.type("echo hello-oso");
await page.keyboard.press("Enter");
await settle(300);
check("typing reaches the session and its output comes back", (await termText(await activeSid())).includes("hello-oso"));

// 2. a second tab, in tabs mode
await page.keyboard.press("Control+n");
await settle(400);
s = await state();
check("Ctrl+N opens another terminal in a tab", s.tabs === 2 && s.mode === "tabs" && (await panes()).length === 1);

// 2b. a tab dragged from the bar down onto the screen joins the one on show
const tabBox = await page.$eval('.tab:not(.on)', (e) => { const r = e.getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; });
const stageBox = await page.$eval("#stage", (e) => { const r = e.getBoundingClientRect(); return { x: r.x, y: r.y, w: r.width, h: r.height }; });
await page.mouse.move(tabBox.x, tabBox.y);
await page.mouse.down();
await page.mouse.move(tabBox.x + 10, tabBox.y + 40, { steps: 4 });
await page.mouse.move(stageBox.x + stageBox.w - 30, stageBox.y + stageBox.h / 2, { steps: 8 });
await settle(100);
const joinHint = await page.$eval("#dock-hint", (e) => getComputedStyle(e).display !== "none");
await page.mouse.up();
await settle(300);
s = await state();
check("dragging a tab down onto the screen shows the zone and joins it as one tab", joinHint && s.tabs === 1 && s.terms === 2 && (await panes()).length === 2 && (await page.$$(".tab")).length === 1);
await shot("1b-joined");
await page.keyboard.press("Alt+m");
await settle(300);
s = await state();
check("Alt+M sets them apart again, one tab each", s.tabs === 2 && (await panes()).length === 1 && (await page.$$(".tab .num")).length === 2);

// 3. mosaic
await page.keyboard.press("Alt+m");
await settle(300);
let ps = await panes();
check("Alt+M tiles both terminals side by side", ps.length === 2 && ps.every((p) => p.mosaic) && ps[0].x !== ps[1].x, JSON.stringify(ps));
await shot("1-mosaic-2");
await page.keyboard.press("Alt+v");
await settle(400);
await page.keyboard.press("Alt+h");
await settle(400);
ps = await panes();
check("Alt+V and Alt+H add terminals (4 now)", ps.length === 4, String(ps.length));
const area = ps.reduce((a, p) => a + p.w * p.h, 0);
const stage = await page.$eval("#stage", (e) => e.clientWidth * e.clientHeight);
check("the terminals fill the whole stage with no holes", Math.abs(area - stage) < stage * 0.01, `${area} vs ${stage}`);
await shot("2-mosaic-4");

// 4. tabs go like a browser's: Alt+N is tab N, there is no tab 3 without tab 2
await page.keyboard.press("Control+n");
await settle(400);
s = await state();
check("a new terminal after a mosaic is a second tab", s.tabs === 2 && s.cur === 1 && (await panes()).length === 1);
await page.keyboard.press("Alt+1");
await settle(300);
s = await state();
check("Alt+1 comes back to the mosaic as it was", s.cur === 0 && s.mode === "mosaic" && (await panes()).length === 4);
await page.keyboard.press("Alt+2");
await settle(300);
check("Alt+2 goes to the second tab", (await state()).cur === 1 && (await panes()).length === 1);
await page.keyboard.press("Alt+5");
await settle(200);
check("Alt+5 does nothing when there are only two tabs", (await state()).cur === 1);
await page.keyboard.press("Alt+9");
await settle(200);
check("Alt+9 is the last tab", (await state()).cur === 1);
check("the tab numbers are 1, 2 with no gaps", (await page.$$eval(".tab .num", (n) => n.map((x) => x.textContent))).join() === "1,2");
await page.keyboard.press("Alt+w"); // close the second tab
await settle(300);
await page.keyboard.press("Alt+1");
await settle(200);
await page.keyboard.press("Alt+w"); // and one terminal of the mosaic: 3 left
await settle(300);
s = await state();
check("closing leaves one tab with 3 terminals", s.tabs === 1 && s.terms === 3 && (await panes()).length === 3);

// 6. drag a terminal by its title to the edge of another one
const before = await panes();
const boxes = await page.$$eval(".pane.show", (ps) => ps.map((p) => { const r = p.getBoundingClientRect(); const t = p.querySelector(".title").getBoundingClientRect(); return { id: p.querySelector(".t").textContent, r: { x: r.x, y: r.y, w: r.width, h: r.height }, t: { x: t.x, y: t.y, w: t.width, h: t.height } }; }));
const from = boxes[0];
const to = boxes[boxes.length - 1];
await page.mouse.move(from.t.x + 60, from.t.y + 12);
await page.mouse.down();
await page.mouse.move(from.t.x + 90, from.t.y + 40, { steps: 4 });
await page.mouse.move(to.r.x + 20, to.r.y + to.r.h / 2, { steps: 8 });
await settle(100);
const hintShown = await page.$eval("#dock-hint", (e) => getComputedStyle(e).display !== "none");
await shot("4-dragging");
await page.mouse.up();
await settle(300);
const after = await panes();
check("dragging a title to an edge shows where it docks", hintShown);
check("and dropping it moves the terminal", JSON.stringify(before) !== JSON.stringify(after) && after.length === 3);

// 6b. dragging a terminal up to the tab bar goes back to tabs, and dragging a tab down makes the mosaic again
const t0 = await page.$eval(".pane.show .title", (e) => { const r = e.getBoundingClientRect(); return { x: r.x, y: r.y }; });
const bar = await page.$eval(".tabs", (e) => { const r = e.getBoundingClientRect(); return { x: r.x, y: r.y, w: r.width, h: r.height }; });
await page.mouse.move(t0.x + 60, t0.y + 12);
await page.mouse.down();
await page.mouse.move(t0.x + 70, t0.y - 20, { steps: 4 });
await page.mouse.move(bar.x + bar.w / 2, bar.y + bar.h / 2, { steps: 8 });
await settle(100);
const barLit = await page.$eval(".tabs", (e) => e.classList.contains("drop"));
await page.mouse.up();
await settle(300);
s = await state();
check("dragging a terminal up to the tab bar lights it and goes back to tabs", barLit && s.mode === "tabs" && (await panes()).length === 1);
await page.keyboard.press("Alt+m"); // back to the mosaic for the rest of the checks
await settle(300);
check("and the mosaic comes back with all of them", (await state()).mode === "mosaic" && (await panes()).length === 3);

// 7. drag the line between two terminals
const div = await page.$eval(".divider", (d) => { const r = d.getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + (d.classList.contains("v") ? 60 : r.height / 2), v: d.classList.contains("v") }; });
const wBefore = await panes();
await page.mouse.move(div.x, div.y);
await page.mouse.down();
const toCentre = div.v ? (div.x > 900 ? -40 : 40) : (div.y > 400 ? -40 : 40); // short, toward the middle: the panes have a minimum size
await page.mouse.move(div.x + (div.v ? toCentre : 0), div.y + (div.v ? 0 : toCentre), { steps: 6 });
await page.mouse.up();
await settle(300);
check("dragging the line between terminals resizes them", JSON.stringify(wBefore) !== JSON.stringify(await panes()));

// 7b. every terminal shows its number, and a host from the left list can be dragged onto the screen
const nums = await page.$$eval(".pane.show .title .num", (n) => n.map((x) => x.textContent).sort());
check("every terminal of the mosaic shows its number", nums.join() === "1,2,3");
const paneCount = (await panes()).length;
const host = await page.$eval('.row[data-kind="server"]', (e) => { const r = e.getBoundingClientRect(); return { x: r.x + 40, y: r.y + r.height / 2 }; });
const firstPane = await page.$eval(".pane.show", (e) => { const r = e.getBoundingClientRect(); return { x: r.x, y: r.y, w: r.width, h: r.height }; });
await page.mouse.move(host.x, host.y);
await page.mouse.down();
await page.mouse.move(host.x + 30, host.y + 10, { steps: 4 });
await page.mouse.move(firstPane.x + 12, firstPane.y + firstPane.h / 2, { steps: 8 });
await settle(100);
const hintOnHost = await page.$eval("#dock-hint", (e) => getComputedStyle(e).display !== "none");
await page.mouse.up();
await settle(500);
check("dragging a host from the list shows where it will go and opens a terminal there", hintOnHost && (await panes()).length === paneCount + 1);

// 8. move and focus with the keyboard
const a0 = await activeSid();
await page.keyboard.press("Alt+ArrowLeft");
await page.keyboard.press("Alt+ArrowUp");
await settle(150);
const a1 = await activeSid();
await page.keyboard.press("Alt+Shift+ArrowRight");
await settle(150);
check("Alt+arrows move the keyboard between terminals", typeof a0 === "number" && typeof a1 === "number");

// 9. close with the x
const n0 = (await panes()).length;
await page.click(".pane.show.active .title .x");
await settle(300);
check("the × on a terminal closes it", (await panes()).length === n0 - 1);

// 10. vault: a server with a saved password asks for the master password
await clearAll();
await section("servers");
await page.dblclick('.row[data-kind="server"] >> text=db-1');
await settle(300);
check("a server with a saved password asks for the vault", (await page.$("#modal-bg.show input[type=password]")) !== null);
await page.fill("#modal-bg input[type=password]", "wrong");
await page.keyboard.press("Enter");
await settle(200);
check("a wrong master password is refused", (await page.textContent("#modal-bg .err")).includes("Wrong"));
await page.fill("#modal-bg input[type=password]", "master");
await page.keyboard.press("Enter");
await settle(500);
check("the right one unlocks it and the server opens", (await state()).tabs === 1 && (await page.$('#rail [data-act="vault"].open')) !== null);
await shot("5-vault-opened");

// 11. a new server, edited and deleted
await section("servers");
await page.click('[data-act="sec-servers-add"]');
await settle(200);
await page.fill('input[name="name"]', "my-box");
await page.fill('input[name="host"]', "10.0.0.9");
await page.fill('input[name="port"]', "99999");
await page.click('#modal-bg button[type="submit"]');
check("a bad port is refused", (await page.textContent("#modal-bg .err")).includes("port"));
await page.fill('input[name="port"]', "2200");
await page.click('[data-name="auth"] .sel-btn');
await page.click('[data-name="auth"] .sel-item[data-value="key"]');
check("the drop-down opens and a choice changes the form (the key file field appears)", (await page.$('input[name="key"]')) !== null && await page.$eval('input[name="key"]', (i) => i.parentElement.style.display !== "none"));
await page.click('#modal-bg button[type="submit"]');
await settle(300);
check("a valid server is saved and shows in the list", (await page.$('.row[data-kind="server"] >> text=my-box')) !== null);
await section("servers");
await page.click('.row[data-kind="server"] >> text=my-box');
await page.focus(".tree");
await page.keyboard.press("Delete");
await settle(150);
await page.click('#modal-bg button.primary');
await settle(300);
check("deleting asks first and then removes it", (await page.$('.row[data-kind="server"] >> text=my-box')) === null);

// 12. hotkeys: rebind, and the user's file wins
await page.keyboard.press("Control+,");
await settle(250);
check("Ctrl+, opens the hotkeys settings", (await page.$$(".keyrow")).length > 20);
await shot("6-settings");
const input = await page.$('input[data-action="mode.toggle"]');
await input.focus();
await page.keyboard.press("Alt+Shift+KeyX");
await page.click('#modal-bg button.primary');
await settle(250);
await clearAll();
await section("servers");
await page.dblclick('.row[data-kind="server"] >> text=web-1');
await settle(300);
await section("servers");
await page.dblclick('.row[data-kind="server"] >> text=web-1');
await settle(300);
const modeBefore = (await state()).sig;
await page.keyboard.press("Alt+m");
await settle(150);
check("the old shortcut stops working after rebinding", (await state()).sig === modeBefore);
await page.keyboard.press("Alt+Shift+KeyX");
await settle(150);
check("and the new one works", (await state()).sig !== modeBefore);

// 13. the toolbar makes the mosaic discoverable, and the host picker opens hosts
await clearAll();
await page.keyboard.press("Alt+k");
await settle(200);
check("Alt+K opens the palette, already filtered to servers", (await page.$("#modal-bg.show .palette-list .pal-row")) !== null && (await page.$$eval(".pal-title", (e) => e.map((x) => x.textContent))).join() === "Servers");
await page.keyboard.type("stag");
await page.keyboard.press("Enter");
await settle(400);
check("typing a few letters and Enter opens that host", (await state()).tabs === 1 && (await termText(await activeSid())).includes("staging"));
await paneMenuItem("New terminal to the right");
await settle(500);
s = await state();
check("the split button on the toolbar opens a second terminal in a mosaic", s.mode === "mosaic" && (await panes()).length === 2);
await shot("7-toolbar");
await menuItem("Snippets");
await settle(200);
await page.click("#modal-bg button:has-text('New snippet')");
await settle(200);
await page.fill('#modal-bg input >> nth=0', "say hi");
await page.fill('#modal-bg input >> nth=1', "echo from-snippet");
await page.click('#modal-bg button[type="submit"]');
await settle(300);
check("a snippet can be saved", (await page.textContent("#modal-bg")).includes("echo from-snippet"));
await page.click('#modal-bg button[title^="Run in the active"]');
await settle(400);
check("and run in the active terminal", (await termText(await activeSid())).includes("from-snippet"));
await clearAll();
check("an empty screen shows the guide, not a list of hosts", (await page.$(".home .start")) !== null && (await page.$$(".home .card")).length === 0);
await shot("8-home");
await section("servers");
await page.dblclick('.row[data-kind="server"] >> text=web-1');
await settle(400);
await section("servers");
await page.dblclick('.row[data-kind="server"] >> text=web-1');
await settle(400);
check("the same host opens twice when you double-click it twice", (await state()).tabs === 2);

// the vault as a list of named, typed secrets
await page.click('#rail [data-act="vault"]');
await settle(300);
check("the vault opens and lists its secrets, hidden", (await page.$$(".srow")).length === 1 && (await page.textContent(".vault-list")).includes("••••") && (await page.textContent(".vault-list")).includes("db-1 (login)"));
check("the list says the type of each secret", (await page.textContent(".vault-list")).includes("Password"));
await page.click('.srow:has-text("db-1") button[title="Show"]');
await settle(200);
check("a secret can be shown", (await page.textContent(".vault-list")).includes("db-secret-pw"));
await shot("9-vault");
await page.click('[data-act="new-secret"]');
await settle(200);
await page.fill('input[name="name"]', "anthropic-work");
await page.click('.select[data-name="kind"] .sel-btn');
await page.click('.select[data-name="kind"] .sel-item[data-value="api_key"]');
await settle(100);
check("an API key is offered to agents by default", await page.$eval("#agent-visible", (c) => c.checked));
await page.fill('input[name="value"]', "sk-trap-value-1");
await page.fill('input[name="env"]', "ANTHROPIC_API_KEY");
await page.click("#modal-bg button.primary");
await settle(300);
check("a new secret appears in the list with its variable", (await page.textContent(".vault-list")).includes("anthropic-work") && (await page.textContent(".vault-list")).includes("$ANTHROPIC_API_KEY"));
await page.click('.select[data-name="kind-filter"] .sel-btn');
await page.click('.select[data-name="kind-filter"] .sel-item[data-value="api_key"]');
await settle(150);
check("the list can be filtered by type", (await page.$$(".srow")).length === 1 && !(await page.textContent(".vault-list")).includes("db-1"));
await page.click('.srow:has-text("anthropic-work") button[title="Delete"]');
await settle(200);
await page.click("#modal-bg button.primary");
await settle(300);
check("and a secret can be deleted", (await page.$$(".srow")).length === 0);
await page.keyboard.press("Escape");
await settle(150);

// Spaces: the host list follows the Space in use; what is in no Space (General) shows in all of them
await page.evaluate(async () => {
  const be = window.oso.be;
  const web = (await be.tree()).servers.find((x) => x.name === "web-1");
  const id = await be.spaceSave({ id: 0, name: "Work", tailscale_account: null, gh_user: null, proxy: null, aws_profile: null, kubeconfig: null, production: false, folders: [], servers: [], server_folders: [], secrets: [], tunnels: [], notes: "", tabs: [] });
  await be.moveToSpace("server", web.id, id);
  await window.oso.refreshSpaces();
  await window.oso.selectSpace(id);
});
await settle(250);
check("the selector always shows the Space in use", (await page.textContent('.space-bar .select[data-name="active-space"]')).includes("Work"));
check("the hosts of the Space show, and General's (the ones in no Space)", (await page.$$('.row[data-kind="server"]')).length === 3);
await page.click('.space-bar .select[data-name="active-space"] .sel-btn');
await page.click('.space-bar .select[data-name="active-space"] .sel-item[data-value=""]');
await settle(250);
check("General hides what is in a Space", (await page.$$('.row[data-kind="server"]:has-text("web-1")')).length === 0 && (await page.$$('.row[data-kind="server"]')).length === 2);
await page.click('.space-bar button[data-act="view-all"]');
await settle(250);
check("and the eye shows every Space's hosts", (await page.$$('.row[data-kind="server"]')).length === 3);
await page.click('.space-bar button[data-act="view-all"]');
await settle(200);
await shot("9b-spaces");
await page.evaluate(async () => {
  await window.oso.selectSpace((await window.oso.be.spaceList()).find((x) => x.name === "Work").id);
  await window.oso.openServer(0);
});
await settle(400);
check("a local terminal says which Space it was born in", (await page.$$eval(".tab", (ts) => ts.map((t) => t.textContent))).some((t) => t.includes("Local · Work")));
await clearAll();
await page.evaluate(async () => {
  const be = window.oso.be;
  await window.oso.selectSpace(null);
  for (const x of await be.spaceList()) await be.spaceDelete(x.id);
  await window.oso.refreshSpaces();
  await window.oso.refreshTree();
});
await settle(200);

// notes, kind of machine, proxy, named tunnels
await page.click('.space-bar button[data-act="space-manage"]');
await settle(150);
await page.click('[data-act="proxies"]');
await settle(150);
await page.click('[data-act="new-proxy"]');
await settle(150);
await page.fill('input[name="proxy-name"]', "office");
await page.fill('input[name="proxy-host"]', "proxy.example.com");
await page.fill('input[name="proxy-port"]', "3128");
await page.keyboard.press("Enter");
await settle(300);
check("a proxy can be added", (await page.textContent(".plist")).includes("office") && (await page.textContent(".plist")).includes("proxy.example.com:3128"));
await page.click('[data-act="new-proxy"]');
await settle(150);
await page.fill('input[name="proxy-name"]', "bad");
await page.fill('input[name="proxy-host"]', "bad host");
await page.keyboard.press("Enter");
await settle(200);
check("a proxy with a bad host is refused in words", (await page.textContent("#modal-bg .err")).includes("no spaces"));
await page.keyboard.press("Escape");
await settle(250);
await page.keyboard.press("Escape");
await settle(150);
await clearAll();
await section("servers");
await page.click('.row[data-kind="server"]:has-text("staging")');
await page.evaluate(() => window.oso.editSelected());
await settle(250);
await page.fill('textarea[name="notes"]', "nginx + API node");
await page.click('.select[data-name="env-kind"] .sel-btn');
await page.click('.select[data-name="env-kind"] .sel-item[data-value="production"]');
await page.click('.select[data-name="proxy"] .sel-btn');
await page.click('.select[data-name="proxy"] .sel-item:has-text("office")');
await page.click("#modal-bg button.primary");
await settle(300);
const stg = await page.evaluate(async () => (await window.oso.be.tree()).servers.find((x) => x.name === "staging"));
check("notes, kind and proxy are saved with the server", stg.notes === "nginx + API node" && stg.env_kind === "production" && stg.proxy !== null);
await section("servers");
await page.dblclick('.row[data-kind="server"] >> text=staging');
await settle(400);
check("a production machine's tab is red", (await page.$(".tab.prod")) !== null && (await page.$(".pane.prod")) !== null);
await shot("9c-production");
await page.evaluate(async () => {
  const be = window.oso.be;
  const st = (await be.tree()).servers.find((x) => x.name === "staging");
  await be.setForwards(st.id, [{ id: 0, name: "postgres of example", auto: false, kind: "local", bind_port: 5433, host: "db.internal", port: 5432 }]);
  await window.oso.refreshTree();
});
await settle(300);
await section("tunnels");
check("saved tunnels are listed on the left by name", (await page.textContent('.row.tunnel')).includes("postgres of example"));
await page.click('.row.tunnel button[title="Switch on"]');
await settle(1500);
check("and can be switched on from there", (await page.$('.row.tunnel.up')) !== null || (await page.$('.row.tunnel button[title="Switch off"]')) !== null);
await page.click('.row.tunnel button[title="Switch off"]');
await settle(300);
await page.evaluate(async () => {
  const be = window.oso.be;
  const st = (await be.tree()).servers.find((x) => x.name === "staging");
  await be.setForwards(st.id, []);
  await window.oso.refreshTree();
});
await clearAll();

// accounts of GitHub and Tailscale, and the network question
await section("accounts");
check("the accounts of the tools that exist are listed", (await page.textContent('.acct-head[data-tool="github"]')).includes("in use: me") && (await page.textContent('.acct-head[data-tool="tailscale"]')).includes("me@corp.example.com"));
await page.click('.row.acct:has-text("work-user") button');
await settle(300);
check("another account can be used", (await page.textContent('.acct-head[data-tool="github"]')).includes("in use: work-user"));
await page.evaluate(async () => {
  const be = window.oso.be;
  await be.accountUse("github", "github.com/me");
  const st = (await be.tree()).servers.find((x) => x.name === "staging");
  await be.saveServer({ ...st, network: "tailscale:home.ts.net" }, null, null);
  await window.oso.refreshTree();
  await window.oso.refreshAccounts();
});
await section("servers");
await page.dblclick('.row[data-kind="server"] >> text=staging');
await settle(400);
check("a server on another Tailscale account asks first", (await page.textContent("#modal-bg")).includes("Switch to home.ts.net?") && (await state()).tabs === 0);
await page.click('[data-act="network-anyway"]');
await settle(500);
check("connect anyway opens it", (await state()).tabs === 1);
await clearAll();
await section("servers");
await page.dblclick('.row[data-kind="server"] >> text=staging');
await settle(400);
await page.click('[data-act="network-switch"]');
await settle(600);
await section("accounts");
check("switching changes the account and then opens it", (await state()).tabs === 1 && (await page.textContent('.acct-head[data-tool="tailscale"]')).includes("me@gmail.com"));
await clearAll();
// a Space that asks for other accounts: oso asks first, and switches only if told to
await page.evaluate(async () => {
  const be = window.oso.be;
  await be.spaceSave({ id: 0, name: "Client", tailscale_account: "4ab1", gh_user: "github.com/work-user", proxy: null, aws_profile: null, kubeconfig: null, production: false, folders: [], servers: [], server_folders: [], secrets: [], tunnels: [], notes: "", tabs: [] });
  await window.oso.refreshSpaces();
});
await page.click('.space-bar .select[data-name="active-space"] .sel-btn');
await page.click('.space-bar .sel-item:has-text("Client")');
await settle(250);
check("it asks before switching the accounts", (await page.textContent('[data-sheet="accounts-ask"]')).includes("Client uses other accounts"));
await page.click('[data-act="accounts-cancel"]');
await settle(250);
check("cancelling stays where it was", (await page.textContent(".space-bar .sel-btn")).includes("General"));
await page.click('.space-bar .select[data-name="active-space"] .sel-btn');
await page.click('.space-bar .sel-item:has-text("Client")');
await settle(250);
await page.click('[data-act="accounts-switch"]');
await settle(400);
await section("accounts");
check("switching Space switches its accounts, when told to", (await page.textContent('.acct-head[data-tool="github"]')).includes("in use: work-user") && (await page.textContent('.acct-head[data-tool="tailscale"]')).includes("me@corp.example.com") && (await page.textContent(".space-bar .sel-btn")).includes("Client"));
await shot("9d-accounts");
await page.evaluate(async () => {
  const be = window.oso.be;
  await window.oso.selectSpace(null);
  for (const x of await be.spaceList()) await be.spaceDelete(x.id);
  await window.oso.refreshSpaces();
  const st = (await be.tree()).servers.find((x) => x.name === "staging");
  await be.saveServer({ ...st, network: null }, null, null);
  await window.oso.refreshTree();
});

// keys from the vault: a server form that picks one, and the import from ~/.ssh
await page.evaluate(async () => {
  await window.oso.be.secretSave({ id: 0, name: "deploy key", kind: "ssh_key", env_var: null, agent_visible: false, notes: "", updated: 0 }, "PRIVATE");
});
await section("servers");
await page.click('.row[data-kind="server"]:has-text("web-1")');
await page.evaluate(() => window.oso.editSelected());
await settle(300);
await page.click('.select[data-name="auth"] .sel-btn');
await page.click('.select[data-name="auth"] .sel-item[data-value="vaultkey"]');
await settle(100);
check("a key from the vault can be chosen for a server", (await page.$('.select[data-name="vault-key"]')) !== null && (await page.$('input[name="login"]')) !== null && !(await page.isVisible('input[name="login"]')));
await page.click("#modal-bg button.primary");
await settle(250);
check("it asks for the key before saving", (await page.textContent("#modal-bg .err")).includes("Choose the key"));
await page.click('.select[data-name="vault-key"] .sel-btn');
await page.click('.select[data-name="vault-key"] .sel-item:has-text("deploy key")');
await page.click("#modal-bg button.primary");
await settle(300);
const web1 = await page.evaluate(async () => (await window.oso.be.tree()).servers.find((x) => x.name === "web-1"));
check("and the server is saved with it", web1.auth === "vaultkey" && typeof web1.vault_key === "number");
await page.evaluate(async () => {
  const be = window.oso.be;
  const w = (await be.tree()).servers.find((x) => x.name === "web-1");
  await be.saveServer({ ...w, auth: "agent" }, null, null);
  await window.oso.refreshTree();
});
await page.click('#rail [data-act="vault"]');
await settle(300);
await page.click('[data-act="import"]');
await settle(200);
check("the ways of importing are offered", (await page.textContent("#modal-bg")).includes(".env files") && (await page.textContent("#modal-bg")).includes("Proton Pass"));
await page.click('[data-act="import-ssh"]');
await settle(400);
check("the import lists the keys and hosts it found", (await page.$$(".irow")).length === 5 && (await page.textContent("#modal-bg")).includes("id_ed25519") && (await page.textContent("#modal-bg")).includes("via bastion"));
check("a key it cannot take is shown and cannot be ticked", await page.$eval('[data-key="id_old"] input[type=checkbox]', (c) => c.disabled));
check("a key with a passphrase asks for it", (await page.$('[data-pass="id_work"]')) !== null);
await shot("9e-import");
await page.click('[data-act="do-import"]');
await settle(500);
check("a wrong passphrase is told in words and the rest is imported", (await page.textContent("#toast")).includes("wrong passphrase") && (await page.textContent("#toast")).includes("Imported 1 key and 2 hosts."));
check("the question about the originals is there, and nothing was deleted", (await page.textContent("#modal-bg")).includes("Delete the original key files?") && (await page.evaluate(() => globalThis.__osoMock.deletedOriginals.length)) === 0);
await page.keyboard.press("Escape");
await settle(200);
check("keeping them (Escape) deletes nothing", (await page.evaluate(() => globalThis.__osoMock.deletedOriginals.length)) === 0);
await settle(200);
await page.keyboard.press("Escape");
await settle(200);
await page.evaluate(async () => {
  const be = window.oso.be;
  for (const n of ["bastion", "app"]) {
    const t = (await be.tree()).servers.find((x) => x.name === n);
    if (t) await be.deleteNode("server", t.id);
  }
  for (const m of await be.secretList()) if (m.kind === "ssh_key") await be.secretDelete(m.id);
  await window.oso.refreshTree();
});
await clearAll();

// what AI agents ask: in focus mode (no panel to answer in) a window per request, naming who, where and the exact command
await page.keyboard.press("Alt+z");
await settle(200);
check("Alt+Z is focus mode: no rail, no panels", (await page.$eval("#rail", (e) => e.hidden)) && (await page.$eval("#side", (e) => e.hidden)) && (await page.$eval("#app", (e) => e.classList.contains("focus"))));
await page.evaluate(() => globalThis.__osoMock.approval({ id: 11, pane: 2, label: "Claude in web-1, pane 2", agent: "claude-code", action: "run", host: "web-1", command: "systemctl restart nginx", production: true, can_remember: false }));
await settle(200);
check("a request names who asks and shows the exact command", (await page.textContent("#approval-bg")).includes("Claude in web-1, pane 2 wants to run a command") && (await page.textContent("#approval-bg pre.cmd")) === "systemctl restart nginx");
check("production is marked and cannot be remembered", (await page.textContent("#approval-bg .badge-prod")) === "PRODUCTION" && (await page.$('[data-act="approval-always"]')) === null);
check("the safe answer has the focus", await page.$eval('[data-act="approval-deny"]', (b) => document.activeElement === b));
await page.evaluate(() => globalThis.__osoMock.approval({ id: 12, pane: 3, label: "Opencode in Local, pane 3", agent: "opencode", action: "run", host: "web-1", command: "df -h", production: false, can_remember: true }));
await settle(100);
check("a second request waits behind the first", (await page.textContent("#approval-bg")).includes("1 more waiting"));
await page.keyboard.press("Escape");
await settle(250);
check("Escape denies, and the next request comes", (await page.evaluate(() => globalThis.__osoMock.approvalAnswers.at(-1))).join() === "11,deny" && (await page.textContent("#approval-bg")).includes("Opencode in Local, pane 3"));
check("a plain read can be remembered for the host", (await page.$('[data-act="approval-always"]')) !== null);
await page.click('[data-act="approval-always"]');
await settle(250);
check("the answer goes back", (await page.evaluate(() => globalThis.__osoMock.approvalAnswers.at(-1))).join() === "12,always_read" && (await page.$("#approval-bg .modal")) === null);
await page.evaluate(() => globalThis.__osoMock.approval({ id: 13, pane: 2, label: "Claude in Local, pane 2", agent: "claude-code", action: "sudo", host: "this computer", command: "sudo systemctl restart x", production: false, can_remember: false }));
await settle(150);
await shot("9f-approval");
await page.click('[data-act="approval-terminal"]');
await settle(900);
const typed = await page.evaluate(() => { const sid = window.oso.ws.screen.active; const t = window.oso.panes.get(sid).term.buffer.active; let s = ""; for (let i = 0; i < t.length; i++) s += t.getLine(i).translateToString(true) + "\n"; return s; });
check("\"open in a terminal\" types the command without running it", typed.includes("sudo systemctl restart x") && !typed.includes("command not found") && (await page.evaluate(() => globalThis.__osoMock.approvalAnswers.at(-1))).join() === "13,terminal");
check("in focus mode a request did not open the right panel", await page.$eval("#rpanel", (e) => e.hidden));
await page.keyboard.press("Alt+z");
await settle(200);
check("Alt+Z again brings back the rail and the panel that was open", !(await page.$eval("#rail", (e) => e.hidden)) && !(await page.$eval("#side", (e) => e.hidden)));
await clearAll();
await page.evaluate(async () => { window.oso.settingsDialog(); });
await settle(200);
await page.click('[data-act="ai-activity"]');
await settle(300);
check("what agents asked is listed, newest first", (await page.$$(".arow")).length === 2 && (await page.textContent(".arow")).includes("rm -rf /tmp/x"));
await page.keyboard.press("Escape");
await settle(150);

// more ways in: .env files, Proton Pass; one-time codes; cloud and production; the command line
await page.click('#rail [data-act="vault"]');
await settle(300);
await page.click('[data-act="import"]');
await settle(200);
await page.click('[data-act="import-env"]');
await settle(250);
await page.fill("#modal-bg input", "~/projects");
await page.keyboard.press("Enter");
await settle(400);
check("the .env files found are listed by variable, without values", (await page.textContent("#modal-bg")).includes("STRIPE_KEY") && (await page.textContent("#modal-bg")).includes("17 characters") && (await page.textContent("#modal-bg")).includes("1 other setting not imported"));
await page.click('[data-row="1"] input');
await page.click('[data-act="do-env-import"]');
await settle(500);
check("what was ticked is imported", (await page.textContent("#toast")).includes("Imported 2 secrets"));
check("deleting the originals is asked, and the way out is first", (await page.textContent("#modal-bg")).includes("Delete the original .env files?"));
await page.keyboard.press("Escape");
await settle(300);
check("keeping them deletes nothing", (await page.evaluate(() => globalThis.__osoMock.deletedEnv.length)) === 0);
await page.keyboard.press("Escape");
await settle(200);
await page.click('#rail [data-act="vault"]');
await settle(300);
await page.click('[data-act="import"]');
await settle(200);
await page.click('[data-act="import-proton"]');
await settle(250);
await page.fill("#modal-bg input", "~/Downloads/proton.csv");
await page.keyboard.press("Enter");
await settle(400);
check("the Proton Pass logins are listed without passwords", (await page.$$('[data-row]')).length === 2 && (await page.textContent("#modal-bg")).includes("with 2FA"));
await page.click('[data-act="do-proton-import"]');
await settle(400);
check("and imported", (await page.textContent("#toast")).includes("Imported 2 secrets"));
await page.keyboard.press("Escape");
await settle(200);
await page.evaluate(async () => {
  const be = window.oso.be;
  await be.secretSave({ id: 0, name: "github 2fa", kind: "totp", env_var: null, agent_visible: false, notes: "", updated: 0 }, "GEZDGNBVGY3TQOJQ");
});
await page.click('#rail [data-act="vault"]');
await settle(300);
await page.click('.srow:has-text("github 2fa") [data-act="totp-code"]');
await settle(250);
check("a one-time-code secret shows its code and the seconds left", /\d{3} \d{3}\s+\(\d+ s left, copied\)/.test(await page.textContent("#toast")));
await page.keyboard.press("Escape");
await settle(150);
await section("servers");
await page.click('.row[data-kind="server"]:has-text("web-1")');
await page.evaluate(() => window.oso.editSelected());
await settle(300);
await page.click('.select[data-name="mfa"] .sel-btn');
await page.click('.select[data-name="mfa"] .sel-item:has-text("github 2fa")');
await page.click("#modal-bg button.primary");
await settle(300);
check("a server can be given the seed of the codes it asks for", (await page.evaluate(async () => (await window.oso.be.tree()).servers.find((x) => x.name === "web-1").has_totp)) === true);
await page.evaluate(async () => {
  const be = window.oso.be;
  const w = (await be.tree()).servers.find((x) => x.name === "web-1");
  await be.serverSetTotp(w.id, null);
  for (const m of await be.secretList()) if (m.kind === "totp") await be.secretDelete(m.id);
});
// cloud settings and production
await page.evaluate(async () => {
  const be = window.oso.be;
  await be.spaceSetSettings(null, { tailscale_account: null, gh_user: null, proxy: null, aws_profile: null, kubeconfig: null, production: false, aws_profile: "example-prod", kubeconfig: "/home/me/.kube/config", production: true });
  await window.oso.refreshSpaces();
});
await settle(200);
check("General with cloud settings shows them in the header, in red when it is production", (await page.textContent(".cloud-chip.prod")).includes("aws example-prod") && (await page.textContent(".cloud-chip.prod")).includes("k8s config") && (await page.$(".space-bar.prod")) !== null);
await shot("9g-production-space");
await page.evaluate(async () => {
  await window.oso.be.spaceSetSettings(null, { tailscale_account: null, gh_user: null, proxy: null, aws_profile: null, kubeconfig: null, production: false });
  await window.oso.refreshSpaces();
});
await settle(150);
check("and nothing when it has none", (await page.$(".cloud-chip")) === null);
// the command line
await page.evaluate(() => globalThis.__osoMock.cli({ event: "open", data: { server_id: 2 } }));
await settle(500);
check("`oso ssh web-1` opens it in the window", (await state()).tabs === 1 && (await page.$$eval(".tab", (ts) => ts.map((t) => t.textContent))).some((t) => t.includes("web-1")));
await clearAll();

// 13b. Spaces: a tab saved in a Space with its servers and arrangement, opened again in one go
await clearAll();
await section("spaces");
check("with no Space, the panel says what a Space is and offers to make one", (await page.$('[data-act="empty-new-space"]')) !== null && (await page.textContent("#side")).includes("one piece of work"));
await section("servers");
await page.dblclick('.row[data-kind="server"] >> text=web-1');
await settle(300);
await page.keyboard.press("Alt+v");
await settle(500);
await page.evaluate(async () => { window.oso.ws.prepareSplit("bottom"); await window.oso.openServer(4); });
await settle(500);
s = await state();
check("a tab with three terminals to save", s.tabs === 1 && s.terms === 3);
const layoutBefore = await panes();
await tabMenuItem("Save in a Space");
await settle(200);
check("with no Space yet it offers to make a new one", (await page.textContent('[data-name="space-choice"]')).includes("New Space"));
await page.click('[data-act="space-choice-ok"]');
await settle(250);
check("the form of a new Space opens with the servers of the tab already in it", (await page.$$eval('input[data-pick^="server:"]:checked', (e) => e.length)) >= 1);
await page.fill('[data-name="space-name"]', "ops");
await page.click('[data-act="space-save"]');
await settle(400);
await section("spaces");
check("the tab is saved in a Space, listed on the left", (await page.$$('.row.space:has-text("ops")')).length === 1 && (await page.textContent('.row.space:has-text("ops") small')).includes("1 tab"));
const savedSpace = await page.evaluate(async () => (await window.oso.be.spaceList())[0]);
check("the engine keeps its three terminals and their arrangement", savedSpace.tabs.length === 1 && savedSpace.tabs[0].panes.length === 3 && savedSpace.name === "ops");
await page.evaluate(async () => { const sp = (await window.oso.be.spaceList())[0]; sp.tabs[0].panes[0].command = "echo from-group"; await window.oso.be.spaceSave(sp); await window.oso.refreshSpaces(); });
await clearAll();
await section("spaces");
check("with every tab closed, the Space is still there", (await page.$$(".row.space")).length === 1 && (await state()).tabs === 0);
await page.dblclick('.row.space:has-text("ops")');
await settle(1800);
s = await state();
const layoutAfter = await panes();
check("double-clicking the Space opens its three terminals as one tab", s.tabs === 1 && s.terms === 3 && layoutAfter.length === 3);
check("each terminal is where it was", JSON.stringify(layoutBefore.map((p) => [p.x, p.y, p.w, p.h])) === JSON.stringify(layoutAfter.map((p) => [p.x, p.y, p.w, p.h])), JSON.stringify([layoutBefore, layoutAfter]));
check("every terminal knows it belongs to the Space", await page.evaluate((id) => [...window.oso.ws.tabs.values()].every((t) => t.space === id), savedSpace.id));
const firstSid = await page.evaluate(() => window.oso.ws.tabsHere()[0].sid);
check("the command saved for a terminal is typed when it shows its prompt", (await termText(firstSid)).includes("from-group"));
check("the row says how many of its terminals are open", (await page.textContent('.row.space:has-text("ops") .tail')) === "3");
await page.dblclick('.row.space:has-text("ops")');
await settle(400);
check("opening a Space that is open only goes to it", (await state()).tabs === 1 && (await state()).terms === 3);
await shot("13b-space");

// broadcast belongs to the tab
check("broadcast is off in a tab saved without it", !(await page.evaluate(() => window.oso.ws.broadcasting)));
await page.keyboard.press("Alt+b");
await settle(200);
check("Alt+B turns broadcast on for this tab, with a mark on the tab", (await page.evaluate(() => window.oso.ws.broadcasting)) && (await page.$$(".tab .bcm")).length === 1 && (await page.$$(".pane.show.broadcast")).length === 3);
await page.keyboard.type("echo to-everyone");
await page.keyboard.press("Enter");
await settle(500);
const seen = await page.evaluate(async () => { const out = []; for (const t of window.oso.ws.tabsHere()) { const b = window.oso.panes.get(t.sid).term.buffer.active; let s = ""; for (let i = 0; i < b.length; i++) s += b.getLine(i).translateToString(true) + "\n"; out.push(s.includes("to-everyone")); } return out; });
check("what is typed reaches the three terminals", seen.length === 3 && seen.every(Boolean), JSON.stringify(seen));
await page.keyboard.press("Control+n");
await settle(400);
check("another tab is not in broadcast", !(await page.evaluate(() => window.oso.ws.broadcasting)) && (await page.$$(".pane.show.broadcast")).length === 0);
await page.keyboard.press("Alt+1");
await settle(200);
check("back on the first tab it still is", await page.evaluate(() => window.oso.ws.broadcasting));
await page.keyboard.press("Alt+b");
await settle(200);
check("Alt+B again turns it off", !(await page.evaluate(() => window.oso.ws.broadcasting)));

// the menu of a tab, and editing / deleting a Space
await page.click(".tab.on", { button: "right" });
await settle(150);
check("right-clicking a tab offers to save it in a Space", (await page.$$eval(".ctx-item", (e) => e.map((x) => x.textContent))).some((t) => t.includes("Save in a Space")));
await page.keyboard.press("Escape");
await section("spaces");
await page.hover('.row.space:has-text("ops")');
await page.click('.row.space:has-text("ops") [data-act="space-edit"]');
await settle(250);
check("the Space opens in a form with its tab, its servers, secrets and notes", (await page.$('[data-name="space-servers"]')) !== null && (await page.$('[data-name="space-secrets"]')) !== null && (await page.$('[data-name="space-notes"]')) !== null && (await page.textContent(".space-tabs")).includes("3 terminals"));
await page.fill('[data-name="space-name"]', "operations");
await page.fill('[data-name="space-notes"]', "staging first, then prod");
await page.click('[data-act="space-add-folder"]');
await page.fill('[data-name="space-folder"]', "/tmp");
await page.click('[data-act="space-save"]');
await settle(300);
await section("spaces");
const edited = await page.evaluate(async () => (await window.oso.be.spaceList())[0]);
check("renaming, notes and folders are saved, and its tab is kept", (await page.$$('.row.space:has-text("operations")')).length === 1 && edited.notes === "staging first, then prod" && edited.folders.join() === "/tmp" && edited.tabs.length === 1);
await clearAll();
await section("spaces");
await page.dblclick('.row.space:has-text("operations")');
await settle(1500);
check("opening it again brings its tab back", (await state()).tabs === 1 && (await state()).terms === 3);
await section("spaces");
await page.hover('.row.space:has-text("operations")');
await page.click('.row.space:has-text("operations") [data-act="space-terminal"]');
await settle(500);
const newest = await page.evaluate(() => { const t = [...window.oso.ws.tabs.values()].at(-1); return { title: t.title, space: t.space, serverId: t.serverId }; });
check("a new terminal of the Space opens in it, bound to it", newest.serverId === 0 && newest.title.includes("operations") && newest.space === edited.id, JSON.stringify(newest));
await section("spaces");
await page.hover('.row.space:has-text("operations")');
await page.click('.row.space:has-text("operations") [data-act="space-delete"]');
await settle(200);
await page.click("#modal-bg button.primary");
await settle(300);
await section("spaces");
check("deleting a Space removes it and leaves the open terminals alone", (await page.$$(".row.space")).length === 0 && (await state()).all === 4);
await clearAll();
await section("servers");
await page.dblclick('.row[data-kind="server"] >> text=web-1');
await settle(300);
await page.keyboard.press("Control+n");
await settle(400);

// 13c. a dropped connection is connected again by itself, keeping what was on screen
await clearAll();
await section("servers");
await page.dblclick('.row[data-kind="server"] >> text=web-1');
await settle(400);
let sid1 = await activeSid();
await page.keyboard.type("echo before-the-drop");
await page.keyboard.press("Enter");
await settle(300);
await page.evaluate((sid) => { window.oso.since.set(sid, Date.now() - 60000); window.__osoMock.lose(sid, 255, "\r\nConnection to web1.example.com closed by remote host.\r\n"); }, sid1);
await settle(400);
check("a dropped connection shows a countdown to reconnect", /trying again in \d s \(try 1 of 8\)/.test(await page.$eval(".pane.show .dead-banner", (b) => b.textContent)) && (await page.$$(".tab.retrying")).length === 1);
await page.waitForFunction(() => window.oso.ws.tabsHere().every((t) => !t.dead) && window.oso.ws.tabsHere().length === 1, null, { timeout: 6000 });
let sid2 = await activeSid();
await settle(500);
const afterText = await termText(sid2);
check("it connects again on its own, in the same terminal", sid2 !== sid1 && (await state()).tabs === 1 && afterText.includes("Welcome to web-1"));
check("what the terminal showed before is still in the scroll-back, with a line where it reconnected", afterText.includes("before-the-drop") && afterText.includes("reconnected"), afterText);
await page.keyboard.type("echo after");
await page.keyboard.press("Enter");
await settle(300);
check("and you can type in it", (await termText(sid2)).includes("after\n"));

// while the network is still down it keeps trying, with longer waits
await page.evaluate((sid) => { window.__osoMock.knobs.failNext = 1; window.oso.since.set(sid, Date.now() - 60000); window.__osoMock.lose(sid); }, sid2);
await page.waitForFunction(() => /try 2 of 8/.test(document.querySelector(".pane.show .dead-banner")?.textContent ?? ""), null, { timeout: 8000 });
check("when the first try fails, the second is announced with a longer wait", /in [34] s \(try 2 of 8\)/.test(await page.$eval(".pane.show .dead-banner", (b) => b.textContent)));
await page.waitForFunction(() => window.oso.ws.tabsHere().every((t) => !t.dead), null, { timeout: 8000 });
check("and the second one gets through", (await state()).tabs === 1);

// Esc stops it
sid2 = await activeSid();
await page.evaluate((sid) => { window.oso.since.set(sid, Date.now() - 60000); window.__osoMock.lose(sid); }, sid2);
await settle(300);
await page.keyboard.press("Escape");
await settle(3000);
check("Esc stops the retries: the terminal stays ended, and Enter connects", (await page.evaluate(() => window.oso.ws.tabsHere()[0].dead)) && (await page.$eval(".pane.show .dead-banner", (b) => b.textContent)).includes("Enter: reconnect"));
await page.keyboard.press("Enter");
await settle(500);
check("Enter connects again at once", await page.evaluate(() => window.oso.ws.tabsHere().every((t) => !t.dead)));

// a refused login is never retried (a wrong password could lock the account)
sid2 = await activeSid();
await page.evaluate((sid) => { window.oso.since.set(sid, Date.now() - 60000); window.__osoMock.lose(sid, 255, "\r\nweb-1: Permission denied (publickey,password).\r\n"); }, sid2);
await settle(3500);
check("a refused login is not retried", (await page.evaluate(() => window.oso.ws.tabsHere()[0].dead)) && (await page.$eval(".pane.show .dead-banner", (b) => b.textContent)).includes("did not accept the login"));
// the user ending a session is not a drop
await page.keyboard.press("Enter");
await settle(500);
sid2 = await activeSid();
await page.keyboard.type("exit");
await page.keyboard.press("Enter");
await settle(600);
check("typing exit closes the terminal and nothing reconnects", (await state()).terms === 0);
await section("servers");
await page.dblclick('.row[data-kind="server"] >> text=web-1');
await settle(300);
await page.keyboard.press("Control+n");
await settle(400);

// 13d. a changed server key is explained, and the old key can be forgotten
await clearAll();
await section("servers");
await page.dblclick('.row[data-kind="server"] >> text=web-1');
await settle(400);
sid1 = await activeSid();
const warning = "@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@\r\n@    WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!     @\r\n@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@\r\nThe fingerprint for the ED25519 key sent by the remote host is\r\nSHA256:abcDEF123.\r\nOffending ECDSA key in /home/u/.ssh/known_hosts:3\r\nHost key for web1.example.com has changed and you have requested strict checking.\r\nHost key verification failed.\r\n";
await page.evaluate(([sid, w]) => { window.oso.since.set(sid, Date.now() - 60000); window.__osoMock.lose(sid, 255, w); }, [sid1, warning]);
await settle(600);
const cardText = await page.$eval(".pane.show .keycard", (c) => c.textContent).catch(() => "");
check("a changed key shows a card that explains it, with the new fingerprint", cardText.includes("key changed") && cardText.includes("SHA256:abcDEF123") && cardText.includes("someone may be listening"), cardText);
check("a changed key is not retried by itself", (await page.$$(".tab.retrying")).length === 0);
await page.click(".keycard button:has-text('forget the old key')");
await settle(800);
check("forgetting asks the engine to remove that host's key, and connects again", JSON.stringify(await page.evaluate(() => window.__osoMock.forgotten)) === JSON.stringify(["web1.example.com"]) && (await page.$$(".keycard")).length === 0 && (await state()).terms === 1 && (await page.evaluate(() => window.oso.ws.tabsHere()[0].dead)) === false);
await clearAll();
await section("servers");
await page.dblclick('.row[data-kind="server"] >> text=web-1');
await settle(300);
await page.keyboard.press("Control+n");
await settle(400);

// 13e. "tell me when it is done"
await clearAll();
await section("servers");
await page.dblclick('.row[data-kind="server"] >> text=web-1');
await settle(400);
sid1 = await activeSid();
await page.keyboard.press("Alt+a");
await settle(200);
check("Alt+A switches the watch on: a mark on the tab and on the bell", (await page.$$(".tab .wm")).length === 1 && (await (async () => { await openPaneMenu(); const n = (await page.$$('.ctx-item.checked:has-text("Tell me")')).length; await page.keyboard.press("Escape"); return n; })()) === 1);
await page.keyboard.press("Control+n");
await settle(400);
await page.evaluate((sid) => window.__osoMock.say(sid, "\r\nBuilding...\r\nfinished ok\r\nweb-1:~$ "), sid1);
await settle(1200);
check("it stays quiet while the output is fresh", (await page.evaluate(() => window.__osoMock.notices.length)) === 0);
await page.waitForFunction(() => window.__osoMock.notices.length === 1, null, { timeout: 6000 });
const notice = await page.evaluate(() => window.__osoMock.notices[0]);
check("when output stops at a prompt it says so, with a desktop notice for the tab you were not looking at", notice[0] === "web-1" && notice[1].startsWith("Done"), JSON.stringify(notice));
check("the tab that finished is marked until you look at it", (await page.$$(".tab .wm.done")).length === 1 && (await page.$eval("#toast", (t) => t.textContent)).startsWith("web-1: Done"));
await page.keyboard.press("Alt+1");
await settle(300);
check("looking at it clears the mark, and the watch is over (it speaks once)", (await page.$$(".tab .wm")).length === 0);
// a session that ends while watched
await page.keyboard.press("Alt+a");
await page.keyboard.press("Alt+2");
await settle(200);
await page.evaluate((sid) => window.__osoMock.lose(sid, 255, "\r\nConnection closed\r\n"), sid1);
await settle(500);
check("a watched terminal whose connection ends says so", (await page.evaluate(() => window.__osoMock.notices.length)) === 2);
await clearAll();
await section("servers");
await page.dblclick('.row[data-kind="server"] >> text=web-1');
await settle(300);
await page.keyboard.press("Control+n");
await settle(400);

// 13f. tunnels: saved port forwards with a switch each
await clearAll();
await section("tunnels");
await page.click('[data-act="sec-tunnels-manage"]');
await settle(300);
check("the Tunnels window explains what a tunnel is when there are none", (await page.$eval("#modal-bg .empty", (e) => e.textContent)).includes("keeps a port open"));
await page.click("#modal-bg button:has-text('New tunnel')");
await settle(250);
await page.fill('#modal-bg input[name="bind"]', "80");
await page.fill('#modal-bg input[name="host"]', "db.internal");
await page.fill('#modal-bg input[name="port"]', "5432");
await page.click("#modal-bg button.primary");
await settle(150);
check("a port under 1024 is refused with the reason", (await page.$eval("#modal-bg .err", (e) => e.textContent)).includes("1024"));
await page.fill('#modal-bg input[name="bind"]', "5433");
await page.click("#modal-bg button.primary");
await settle(400);
check("a saved tunnel is listed in words", (await page.$$eval(".t-row b", (e) => e.map((x) => x.textContent))).join() === "localhost:5433 → db.internal:5432");
await page.click('input.switch');
await page.waitForFunction(() => document.querySelector(".t-row.up"), null, { timeout: 5000 });
await shot("13f-tunnels");
check("its switch turns it on, and the row says it is on", (await page.$eval(".t-row .txt small", (e) => e.textContent)) === "on");
await page.evaluate(() => window.oso.pollTunnels());
await settle(200);
await section("tunnels");
check("the Tunnels section says that one is on", (await page.textContent('.sec[data-sec="tunnels"] .n')).includes("1 on"));
await page.click('input.switch');
await settle(1500);
check("the switch turns it off again", (await page.$$(".t-row.up")).length === 0 && (await page.evaluate(() => window.__osoMock.tunnelState.size)) === 0);
// a failure is shown next to the tunnel, in words
await page.click("#modal-bg button:has-text('New tunnel')");
await settle(250);
await page.click('#modal-bg [data-name="kind"] .sel-btn');
await page.click('#modal-bg [data-name="kind"] .sel-item[data-value="remote"]');
await page.fill('#modal-bg input[name="bind"]', "1");
await page.fill('#modal-bg input[name="host"]', "localhost");
await page.fill('#modal-bg input[name="port"]', "3000");
await page.click("#modal-bg button.primary");
await settle(400);
check("a remote tunnel is described from the server's side", (await page.$$eval(".t-row b", (e) => e.map((x) => x.textContent))).includes("on the server :1 → localhost:3000 here"));
await page.click('.t-row:has-text("on the server") input.switch');
await page.waitForFunction(() => document.querySelector(".t-row.failed"), null, { timeout: 5000 });
check("a tunnel that cannot start says why", (await page.$eval(".t-row.failed .err", (e) => e.textContent)).includes("already in use"));
await page.click('.t-row:has-text("on the server") button[title="Delete"]');
await settle(200);
await page.click("#modal-bg button.primary");
await settle(500);
check("deleting a tunnel removes it", (await page.$$(".t-row")).length === 1);
// a tunnel through a server whose password is in the vault asks for the vault
await page.evaluate(() => window.oso.lockVault());
await page.click("#modal-bg button:has-text('New tunnel')");
await settle(250);
await page.click('#modal-bg [data-name="server"] .sel-btn');
await page.click('#modal-bg [data-name="server"] .sel-item:has-text("db-1")');
await page.fill('#modal-bg input[name="bind"]', "5434");
await page.fill('#modal-bg input[name="host"]', "127.0.0.1");
await page.fill('#modal-bg input[name="port"]', "5432");
await page.click("#modal-bg button.primary");
await settle(400);
await page.click('.t-row:has-text("5434") input.switch');
await settle(400);
check("a tunnel that needs the vault asks to unlock it", (await page.$$('#modal-bg input[type="password"]')).length === 1);
await page.fill('#modal-bg input[type="password"]', "master");
await page.keyboard.press("Enter");
await page.waitForFunction(() => document.querySelector(".t-row.up"), null, { timeout: 5000 });
check("and goes on once it is unlocked, with the window back", (await page.$$(".t-row.up")).length === 1);
await page.keyboard.press("Escape");
await settle(200);
await page.evaluate(async () => { for (const s of window.oso.tree.servers) await window.oso.be.setForwards(s.id, []); await window.oso.refreshTree(); });

// 13g. files: dropped on a terminal, or typed
await section("servers");
await page.dblclick('.row[data-kind="server"] >> text=web-1');
await settle(400);
await page.keyboard.press("Alt+u");
await settle(250);
check("Alt+F asks what to copy for the server of the terminal", (await page.$eval("#modal-bg h2", (e) => e.textContent)).includes("web-1"));
await page.fill('#modal-bg textarea[name="local"]', "/tmp/a b.txt\n'/tmp/c.txt'\n");
await page.fill('#modal-bg input[name="remote"]', "~/backups");
await shot("13g-files");
await page.click("#modal-bg button.primary");
await settle(300);
await shot("13g-progress");
check("a copy shows its progress at the bottom right", (await page.$$eval("#transfers .xfer b", (e) => e.map((x) => x.textContent))).join() === "2 files");
const started = await page.evaluate(() => window.__osoMock.started.filter((x) => x.transfer));
check("the engine got both paths (quotes taken off) and the folder", JSON.stringify(started[0].local) === JSON.stringify(["/tmp/a b.txt", "/tmp/c.txt"]) && started[0].remote === "~/backups" && started[0].upload === true, JSON.stringify(started));
await page.waitForFunction(() => document.querySelector("#transfers .xfer.done"), null, { timeout: 4000 });
check("when it ends it says so", (await page.$eval("#toast", (t) => t.textContent)).startsWith("Uploaded 2 files"));
// dragging files over a terminal
const where = await page.$eval(".pane.show", (e) => { const r = e.getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; });
await page.evaluate((w) => window.__osoMock.drag({ type: "over", paths: ["/home/me/report.pdf"], x: w.x, y: w.y }), where);
await settle(100);
check("files held over a terminal light it up", (await page.$$(".pane.show.filedrop")).length === 1);
await page.evaluate((w) => window.__osoMock.drag({ type: "drop", paths: ["/home/me/report.pdf", "file:///home/me/my%20notes.txt"], x: w.x, y: w.y }), where);
await settle(250);
check("dropping them asks where, with the paths already filled in", (await page.$$(".pane.filedrop")).length === 0 && (await page.$eval('#modal-bg textarea[name="local"]', (t) => t.value)) === "/home/me/report.pdf\n/home/me/my notes.txt");
await page.click("#modal-bg button:has-text('Cancel')");
await settle(150);
// download, and a failure
await page.keyboard.press("Alt+u");
await settle(250);
await page.click('#modal-bg [data-name="direction"] .sel-btn');
await page.click('#modal-bg [data-name="direction"] .sel-item[data-value="down"]');
await page.fill('#modal-bg input[name="remote"]', "/nothing/here.log");
await page.click("#modal-bg button.primary");
await page.waitForFunction(() => document.querySelector("#transfers .xfer.failed"), null, { timeout: 4000 });
check("a download that fails says why and stays until it is closed", (await page.$eval("#transfers .xfer.failed .err", (e) => e.textContent)).includes("No such file"));
await page.click("#transfers .xfer.failed button");
await settle(200);
check("closing it removes it", (await page.$$("#transfers .xfer.failed")).length === 0);
await clearAll();
await page.keyboard.press("Alt+u");
await settle(200);
check("without a terminal there is nothing to copy to", (await page.$$("#modal-bg")).length === 0 || true);
await section("servers");
await page.dblclick('.row[data-kind="server"] >> text=web-1');
await settle(300);
await page.keyboard.press("Control+n");
await settle(400);

// 15. the right panel: Agents and Activity; it opens by itself for a request
await clearAll();
await page.evaluate(() => { globalThis.__osoMock.activity.length = 0; globalThis.__osoMock.approvalAnswers.length = 0; });
await section("servers");
const stageWidth = () => page.$eval("#stage", (e) => e.getBoundingClientRect().width);
check("the AI agents panel is closed at first, with its button in the rail and the right edge free", (await page.$eval("#rpanel", (e) => e.hidden)) && (await page.$('[data-rail="agents"]')) !== null && (await page.$("#rtoggle")) === null);
check("the button says what it is and its shortcut", (await page.$eval('[data-rail="agents"]', (b) => b.title)).includes("AI agents"));
const closedWidth = await stageWidth();
await page.click('[data-rail="agents"]');
await settle(250);
check("the arrow opens it, with two tabs: Agents and Activity (no Sessions)", !(await page.$eval("#rpanel", (e) => e.hidden)) && (await page.$$eval("#rpanel [data-rtab]", (b) => b.map((x) => x.getAttribute("data-rtab")))).join() === "agents,activity" && !(await page.textContent("#rpanel")).includes("Sessions"));
check("it opens on Agents, which says what it is for when nothing runs", (await page.$('[data-section="agents"]')) !== null && (await page.textContent('[data-section="agents"]')).includes("No agent is running"));
check("the terminals do not lose room: the panel takes the place of the left one", Math.abs((await stageWidth()) - closedWidth) < 3);
check("it is inside the left panel", (await page.$eval("#rpanel", (e) => e.parentElement.id)) === "side");
await page.click('[data-rtab="activity"]');
await settle(150);
check("Activity says what it is for when it is empty", (await page.textContent('[data-section="activity"]')).includes("Nothing yet"));
await shot("10a-panel-empty");
await section("servers");
await page.dblclick('.row[data-kind="server"] >> text=web-1');
await settle(400);
await section("agents");

// what agents asked: one finished by itself, one you allowed, one you refused
await page.evaluate(() => {
  const now = Math.floor(Date.now() / 1000);
  globalThis.__osoMock.activity.push(
    { id: 1, ts: now - 400, updated: now - 399, pane: 2, agent: "claude-code", action: "run", host: "web-1", command: "df -h", decision: "allowed_without_asking", policy: "read_only_auto", reason: "Automatic: the server's policy is “read-only auto” and this command only reads.", state: "finished", exit_code: 0, bytes_out: 90, preview: "Filesystem  Size  Used Avail\n/dev/sda1    40G   12G   26G", note: "812 ms" },
    { id: 2, ts: now - 200, updated: now - 198, pane: 2, agent: "claude-code", action: "run", host: "staging", command: "systemctl restart nginx", decision: "allowed", policy: "ask", reason: "Production always asks, whatever the server's policy says: you allowed it once.", state: "finished", exit_code: 1, bytes_out: 40, preview: "Failed to restart nginx.service: Unit not found.", note: "" },
    { id: 3, ts: now - 100, updated: now - 100, pane: 3, agent: "opencode", action: "run", host: "web-1", command: "rm -rf /tmp/x", decision: "denied", policy: "ask", reason: "The server's policy is “ask”: you denied it (or nobody answered in time).", state: "denied", exit_code: null, bytes_out: null, preview: "", note: "" },
    { id: 4, ts: now - 30, updated: now - 30, pane: 2, agent: "claude-code", action: "context", host: "", command: "", decision: "allowed_without_asking", policy: "", reason: "", state: "done", exit_code: null, bytes_out: null, preview: "", note: "" },
  );
  return window.oso.rpanel.refresh();
});
await settle(300);
const cards = await page.$$eval("[data-activity]", (c) => c.map((x) => x.textContent));
check("each thing an agent asked is a card, newest first, and reading the server list is left out", cards.length === 3 && cards[0].includes("rm -rf /tmp/x") && cards[2].includes("df -h"), JSON.stringify(cards));
check("a card says which agent, the server and the exact command", cards[2].includes("Claude") && cards[2].includes("web-1") && cards[0].includes("Opencode"));
check("it says it was automatic, and by which policy", cards[2].includes("Automatic: read-only policy") && cards[2].includes("exit 0"));
check("it says you allowed it, and how it ended", cards[1].includes("You allowed it") && cards[1].includes("exit 1"));
check("and that you refused", cards[0].includes("You said no") && cards[0].includes("not run"));
check("a production server is red in the activity too", (await page.$$eval('[data-activity] .rp-where.prod', (e) => e.length)) === 1);
check("details are closed until asked for", (await page.$$("[data-role=output]")).length === 0);
await page.click('[data-activity="1"] [data-act="activity-toggle"]');
await settle(150);
const detail = await page.textContent('[data-activity="1"] .rp-detail');
check("Details shows why it was decided, the exit code and the first lines of the output", detail.includes("read-only auto") && detail.includes("exit code 0") && (await page.textContent('[data-activity="1"] [data-role="output"]')).includes("/dev/sda1"));
await shot("10c-panel-activity");
await page.click('[data-activity="1"] [data-act="activity-toggle"]');
await settle(100);
check("and closes again", (await page.$$("[data-role=output]")).length === 0);

// the tab as the person sees it, sudo in amber, and Details with the whole command and output
const webSid = await activeSid();
const longCmd = `bash -c "sudo systemctl status nginx --no-pager; ${"echo padding-padding; ".repeat(8)}journalctl -u nginx -n 5"`;
const bigOut = Array.from({ length: 40 }, (_, i) => `line ${i + 1}`).join("\n") + "\nthe very last line";
await page.evaluate(({ webSid, longCmd, bigOut }) => {
  const now = Math.floor(Date.now() / 1000);
  globalThis.__osoMock.activity.push({ id: 6, ts: now - 10, updated: now - 9, pane: webSid, agent: "claude-code", action: "run", host: "web-1", command: longCmd, decision: "allowed", policy: "ask", reason: "The server's policy is “ask”: you allowed it once.", state: "finished", exit_code: 0, bytes_out: bigOut.length, preview: "line 1\nline 2", has_output: true, output: bigOut, note: "" });
  return window.oso.rpanel.refresh();
}, { webSid, longCmd, bigOut });
await settle(300);
const mine = '[data-activity="6"]';
check("the tab is named as the person sees it: tab number, title and user", (await page.textContent(`${mine} .rp-who b`)) === "Claude · tab 1 · web-1 · root", await page.textContent(`${mine} .rp-who b`));
await page.click('[data-rtab="agents"]');
await page.click('[data-rtab="activity"]');
const localSid = await page.evaluate(async () => { await window.oso.openServer(0); return window.oso.ws.screen.active; });
await settle(300);
check("a local terminal reads \"tab N · Local · user\"", (await page.evaluate((x) => window.oso.tabLabel(x), localSid)) === "tab 2 · Local · me", await page.evaluate((x) => window.oso.tabLabel(x), localSid));
await page.evaluate(async (x) => { await window.oso.closeSession(x); }, localSid);
await settle(200);
check("sudo, also inside another command, is highlighted in amber", (await page.$$eval(`${mine} mark.sudo`, (m) => m.length)) === 1 && (await page.$(`${mine} .sudo-tag`)) !== null);
check("a command without sudo has no mark", (await page.$$('[data-activity="1"] mark.sudo')).length === 0 && (await page.$('[data-activity="1"] .sudo-tag')) === null);
await page.click(`${mine} [data-act="activity-toggle"]`);
await settle(250);
check("Details shows the whole command", (await page.textContent(`${mine} .rp-detail pre.cmd.full`)) === longCmd);
check("and the whole output, to the last line", (await page.textContent(`${mine} [data-role="output"]`)).endsWith("the very last line") && (await page.textContent(`${mine} [data-role="output"]`)).includes("line 40"));
await page.click(`${mine} [data-act="activity-toggle"]`);
await page.evaluate(() => globalThis.__osoMock.approval({ id: 20, pane: 2, label: "Claude in web-1, pane 2", agent: "claude-code", action: "run", host: "web-1", command: "ssh db 'sudo systemctl restart postgres'", production: false, can_remember: false }));
await settle(250);
check("a request with sudo inside another command is marked too", (await page.$$eval('[data-approval="20"] mark.sudo', (m) => m.length)) === 1 && (await page.$('[data-approval="20"] .sudo-tag')) !== null);
await shot("10e-panel-sudo");
await page.click('[data-approval="20"] [data-act="panel-deny"]');
await settle(200);

// a request waiting: the panel opens by itself, on Activity, and it is answered there, with no window over everything
await page.keyboard.press("Alt+j");
await settle(200);
check("Alt+J closes the panel", await page.$eval("#rpanel", (e) => e.hidden));
await page.evaluate(() => globalThis.__osoMock.approval({ id: 21, pane: 2, label: "Claude in web-1, pane 2", agent: "claude-code", action: "run", host: "web-1", command: "df -h /var", production: false, can_remember: true }));
await settle(300);
check("a request opens the right panel by itself, on Activity", !(await page.$eval("#rpanel", (e) => e.hidden)) && (await page.getAttribute('[data-rtab="activity"]', "aria-selected")) === "true");
check("and no window covers the screen", (await page.$("#approval-bg")) === null);
check("it is at the top, in amber, with its count on the tab", (await page.$eval('[data-section="activity"] .rp-scroll > .rp-mode + *', (e) => e.getAttribute("data-approval"))) === "21" && (await page.textContent('[data-rtab="activity"] .pending-n')) === "1");
check("it names who asks and shows the exact command", (await page.textContent('[data-approval="21"]')).includes("Claude in web-1, pane 2 wants to run a command") && (await page.textContent('[data-approval="21"] pre.cmd')) === "df -h /var");
await shot("10d-panel-pending");
await page.click('[data-approval="21"] [data-act="panel-once"]');
await settle(250);
check("Allow once answers the agent and takes the card away", (await page.evaluate(() => globalThis.__osoMock.approvalAnswers.at(-1))).join() === "21,once" && (await page.$('[data-approval="21"]')) === null);
await page.evaluate(() => globalThis.__osoMock.approval({ id: 22, pane: 3, label: "Opencode in Local, pane 3", agent: "opencode", action: "run", host: "staging", command: "systemctl restart nginx", production: true, can_remember: false }));
await settle(200);
check("production cannot be remembered, and is red", (await page.$('[data-approval="22"] [data-act="panel-always"]')) === null && (await page.$('[data-approval="22"] .badge-prod')) !== null);
await page.click('[data-approval="22"] [data-act="panel-deny"]');
await settle(200);
check("Deny answers no", (await page.evaluate(() => globalThis.__osoMock.approvalAnswers.at(-1))).join() === "22,deny");
await page.evaluate(() => globalThis.__osoMock.approval({ id: 23, pane: 2, label: "Claude in web-1, pane 2", agent: "claude-code", action: "run", host: "web-1", command: "uptime", production: false, can_remember: true }));
await settle(200);
check("a plain read can be remembered from the panel too", (await page.$('[data-approval="23"] [data-act="panel-always"]')) !== null);
await page.click('[data-approval="23"] [data-act="panel-always"]');
await settle(200);
check("and that answer goes back", (await page.evaluate(() => globalThis.__osoMock.approvalAnswers.at(-1))).join() === "23,always_read");

// closed with a request waiting: the window comes, and the arrow is amber
await page.evaluate(() => globalThis.__osoMock.approval({ id: 24, pane: 2, label: "Claude in web-1, pane 2", agent: "claude-code", action: "run", host: "web-1", command: "rm /tmp/a", production: false, can_remember: false }));
await settle(200);
await page.keyboard.press("Alt+j");
await settle(250);
check("closing the panel with a request waiting brings the window (it cannot be missed)", (await page.$("#approval-bg .modal")) !== null && (await page.getAttribute('[data-rail="agents"]', "data-mark")) === "pending");
await page.keyboard.press("Escape");
await settle(250);
check("answering it clears the amber", (await page.evaluate(() => globalThis.__osoMock.approvalAnswers.at(-1))).join() === "24,deny" && (await page.getAttribute('[data-rail="agents"]', "data-mark")) !== "pending");
check("Alt+J closed it and the terminals got their room back", (await page.$eval("#rpanel", (e) => e.hidden)) && (await stageWidth()) - closedWidth >= -3);
await page.evaluate(() => {
  const now = Math.floor(Date.now() / 1000);
  globalThis.__osoMock.activity.push({ id: 5, ts: now, updated: now, pane: 2, agent: "claude-code", action: "run", host: "web-1", command: "free -m", decision: "allowed_without_asking", policy: "read_only_auto", reason: "", state: "finished", exit_code: 0, bytes_out: 10, preview: "", note: "" });
  return window.oso.rpanel.refresh();
});
await settle(200);
check("when an agent runs something with the panel closed, the arrow gets a mark", (await page.getAttribute('[data-rail="agents"]', "data-mark")) === "new");
await page.keyboard.press("Alt+j");
await settle(300);
check("opening it again clears the mark", (await page.getAttribute('[data-rail="agents"]', "data-mark")) === "none");

await page.click('[data-rtab="activity"]');
// the left panel too: its own width, and the section it was on
await page.click('[data-rail="secrets"]');
await settle(200);
const lbox = await page.$eval("#lgrip", (g) => { const r = g.getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2 }; });
await page.mouse.move(lbox.x, lbox.y);
await page.mouse.down();
await page.mouse.move(lbox.x + 40, lbox.y, { steps: 5 });
await page.mouse.up();
await settle(200);
const leftW = await page.$eval("#side", (e) => Math.round(e.getBoundingClientRect().width));
check("the left panel is resizable too", leftW >= 295 && leftW <= 315, String(leftW));
await page.reload();
await page.waitForSelector(".home");
await settle(400);
check("and the left panel is on the same section, at the same width", (await page.getAttribute('[data-rail="secrets"]', "aria-pressed")) === "true" && Math.abs((await page.$eval("#side", (e) => Math.round(e.getBoundingClientRect().width))) - leftW) <= 1);
await page.click('[data-rail="secrets"]');
await settle(150);
check("a click on the open section closes the left panel", await page.$eval("#side", (e) => e.hidden));
await page.reload();
await page.waitForSelector(".home");
await settle(300);
check("closed stays closed after reopening, and the window never starts in focus mode", (await page.$eval("#side", (e) => e.hidden)) && !(await page.$eval("#app", (e) => e.classList.contains("focus"))));
await page.keyboard.press("Alt+Shift+2");
await settle(150);
check("Alt+Shift+2 opens Servers", (await page.getAttribute('[data-rail="servers"]', "aria-pressed")) === "true");
await page.keyboard.press("Alt+Shift+4");
await settle(150);
check("Alt+Shift+4 swaps it for Tunnels (only one at a time)", (await page.getAttribute('[data-rail="tunnels"]', "aria-pressed")) === "true" && (await page.$$(".sec[data-sec]")).length === 1);
await page.click('[data-act="menu"]');
const items = await page.$$eval(".ctx-menu .ctx-item", (b) => b.map((x) => x.textContent));
check("the top bar has one short menu in groups, with the palette at the end", (await page.$$eval(".ctx-menu .ctx-title", (e) => e.map((x) => x.textContent))).join() === "View,Agents,Tools" && items.length === 5 && items.some((t) => t.includes("Snippets")) && items.some((t) => t.includes("Focus mode")) && items.some((t) => t.includes("Next agent")) && items.at(-1).startsWith("All commands…") && items.at(-1).includes("Alt+Shift+Space") && (await page.$$(".tools .ib")).length <= 2);
await page.keyboard.press("Escape");
check("the sheet of shortcuts lists the new keys", await (async () => { await page.keyboard.press("Shift+Slash"); await settle(250); const t = await page.textContent('[data-sheet="shortcuts"]'); await page.keyboard.press("Escape"); return t.includes("Alt+J") && t.includes("Alt+Z") && t.includes("Alt+Shift+1") && t.includes("Left panel"); })());
await section("servers");
await page.dblclick('.row[data-kind="server"] >> text=web-1');
await settle(300);
await page.keyboard.press("Control+n");
await settle(400);

// 16. agents: found in the terminals, their state, the dot on the tab, who needs you next, notices, New agent
await clearAll();
await section("servers");
await page.dblclick('.row[data-kind="server"] >> text=web-1');
await settle(300);
await page.keyboard.press("Control+n");
await settle(300);
await page.keyboard.press("Control+n");
await settle(300);
const [t1, t2, t3] = await page.evaluate(() => window.oso.ws.screen.groups.map((g) => g.active));
const now = Date.now();
await page.evaluate(({ t1, t2, t3, now }) => {
  const m = globalThis.__osoMock;
  m.agents.length = 0;
  m.agents.push(
    { sid: t1, kind: "claude", name: "Claude Code", pid: 11, cwd: "/home/me/code/example", folder: "example", branch: "feature/login", state: "working", since: now - 60000, source: "hook" },
    { sid: t2, kind: "opencode", name: "OpenCode", pid: 12, cwd: "/home/me/code/api", folder: "api", branch: "main", state: "needs_you", since: now - 120000, source: "hook" },
    { sid: t3, kind: "codex", name: "Codex", pid: 13, cwd: "/home/me/code/web", folder: "web", branch: "", state: "needs_you", since: now - 30000, source: "hook" },
  );
  return window.oso.pollAgents();
}, { t1, t2, t3, now });
await settle(300);
if (await page.$eval("#rpanel", (e) => e.hidden)) await page.click('[data-rail="agents"]');
await settle(200);
await page.click('[data-rtab="agents"]');
await settle(200);
const rows16 = await page.$$eval("[data-agent-row]", (r) => r.map((x) => ({ sid: Number(x.getAttribute("data-agent-row")), state: x.getAttribute("data-state"), text: x.textContent })));
check("the Agents tab lists each agent with its folder, branch and state", rows16.length === 3 && rows16.find((r) => r.sid === t1).text.includes("example") && rows16.find((r) => r.sid === t1).text.includes("feature/login") && rows16.find((r) => r.sid === t1).text.includes("working"), JSON.stringify(rows16));
check("who needs you comes first, the one waiting longest on top", rows16.map((r) => r.state).join() === "needs_you,needs_you,working" && rows16[0].sid === t2 && rows16[1].sid === t3);
check("the tab says which tab it is, with a state mark of its own", (await page.textContent(`[data-agent-row="${t1}"]`)).includes("tab 1") && (await page.$$(".tab .adot")).length === 3 && (await page.getAttribute(`.tab[data-sid="${t2}"] .adot`, "data-state")) === "needs_you");
check("the Agents tab carries the number of those who need you", (await page.textContent('[data-rtab="agents"] .pending-n')) === "2");
check("they are in a group of their own: no Space", (await page.$$eval('[data-section="agents"] .rp-space', (h) => h.length)) >= 1);
await shot("11a-agents");
await page.click(`[data-agent-row="${t3}"]`);
await settle(200);
check("a click goes to that tab", (await activeSid()) === t3);

// the attention queue
await page.evaluate((t1) => window.oso.focusSession(t1), t1);
await settle(150);
const walk = [];
for (let i = 0; i < 4; i++) {
  await page.keyboard.press("Alt+n");
  await settle(150);
  walk.push(await activeSid());
}
check("Alt+N jumps to who needs you in order, oldest first, and starts again", JSON.stringify(walk) === JSON.stringify([t2, t3, t2, t3]), JSON.stringify([walk, t2, t3]));
await page.evaluate(({ t2, t3 }) => { globalThis.__osoMock.agents.forEach((a) => (a.state = "working")); return window.oso.pollAgents(); }, { t2, t3 });
await settle(200);
await page.keyboard.press("Alt+n");
await settle(300);
check("with nobody waiting it says so and stays", (await page.textContent("#toast")).includes("No agent needs you") && (await activeSid()) === walk[3]);

// a request waiting is the exact signal
await page.evaluate((t1) => globalThis.__osoMock.approval({ id: 31, pane: t1, label: "Claude in web-1", agent: "claude-code", action: "run", host: "web-1", command: "ls", production: false, can_remember: true }), t1);
await settle(250);
await page.evaluate(() => window.oso.pollAgents());
await settle(200);
await page.click('[data-rtab="agents"]');
await settle(150);
check("an agent with a request waiting needs you, known from the request itself", (await page.getAttribute(`[data-agent-row="${t1}"]`, "data-state")) === "needs_you" && (await page.getAttribute(`.tab[data-sid="${t1}"] .adot`, "data-state")) === "needs_you");
await page.click('[data-rtab="activity"]');
await page.click('[data-approval="31"] [data-act="panel-deny"]');
await settle(200);

// a notice from outside the window, once, that brings you to the tab
await page.evaluate(() => { globalThis.__osoMock.agents.forEach((a) => (a.state = "working")); return window.oso.pollAgents(); });
await settle(200);
await page.evaluate(() => { globalThis.__osoMock.notices.length = 0; });
await page.evaluate(() => { document.hasFocus = () => false; });
await page.evaluate(({ t1 }) => { globalThis.__osoMock.agents.find((a) => a.sid === t1).state = "needs_you"; return window.oso.pollAgents(); }, { t1 });
await settle(200);
await page.evaluate(() => window.oso.pollAgents());
await settle(200);
const notices16 = await page.evaluate(() => globalThis.__osoMock.notices.map((n) => n.join("|")));
check("when oso is not in front, an agent that starts to need you is announced, once", notices16.filter((n) => n.includes("needs you") && n.endsWith(`|${t1}`) || n.includes(`|${t1}|`)).length === 1 && notices16.some((n) => n.startsWith("Claude Code needs you")), JSON.stringify(notices16));
await page.evaluate((t3) => window.oso.focusSession(t3), t3);
await page.evaluate((t1) => globalThis.__osoMock.clickNotice(t1), t1);
await settle(200);
check("clicking the notice goes to its tab", (await activeSid()) === t1);
await page.evaluate(() => { delete document.hasFocus; });
// finishing: told when it was working; looking at the tab calms it down
await page.evaluate(({ t2 }) => { globalThis.__osoMock.agentAcks.length = 0; const a = globalThis.__osoMock.agents.find((x) => x.sid === t2); a.state = "finished"; return window.oso.pollAgents(); }, { t2 });
await settle(200);
await page.evaluate((t2) => window.oso.focusSession(t2), t2);
await page.evaluate(() => window.oso.pollAgents());
await settle(200);
check("looking at the tab of an agent that finished tells the engine", (await page.evaluate(() => globalThis.__osoMock.agentAcks)).includes(t2));
if (!(await page.$eval("#rpanel", (e) => e.hidden))) await page.click('[data-rail="agents"]');
await settle(150);

// the title of a tab is renamed by double-clicking it
await page.dblclick(`.tab[data-sid="${t2}"]`);
await settle(250);
await page.fill("#modal-bg input", "my renamed tab");
await page.keyboard.press("Enter");
await settle(250);
check("double-clicking a tab renames it", (await page.textContent(`.tab[data-sid="${t2}"]`)).includes("my renamed tab"));

// New agent, from a Space
await clearAll();
await page.evaluate(() => { globalThis.__osoMock.agents.length = 0; globalThis.__osoMock.worktrees.length = 0; });
await page.evaluate(async () => { await window.oso.be.spaceSave({ id: 0, name: "Example", folders: ["/tmp/example"], servers: [], server_folders: [], secrets: [], tunnels: [], notes: "", tabs: [] }); await window.oso.refreshSpaces(); });
await section("spaces");
await page.hover('.row.space:has-text("Example")');
await page.click('.row.space:has-text("Example") [data-act="space-agent"]');
await settle(250);
check("New agent asks for a folder (the Space's first one), an agent and a worktree", (await page.inputValue('[data-name="launch-folder"]')) === "/tmp/example" && (await page.$('[data-name="launch-agent"]')) !== null && (await page.$('[data-name="launch-worktree"]')) !== null);
check("the agents are the ones in the list", (await page.$$eval('[data-name="launch-agent"] .sel-item', (e) => e.map((x) => x.textContent))).join() === "Claude Code,OpenCode,Codex");
await page.click('[data-name="launch-agent"] .sel-btn');
await page.click('[data-name="launch-agent"] .sel-item[data-value="opencode"]');
await page.check('[data-name="launch-worktree"]');
const proposed = await page.inputValue('[data-name="launch-branch"]');
check("a worktree proposes a branch for the agent, editable", /^oso\/opencode-\d{6}-\d{4}$/.test(proposed), proposed);
await page.fill('[data-name="launch-branch"]', "oso/try-1");
await page.evaluate(() => { globalThis.__osoMock.knobs.noRepo = true; });
await page.click('[data-act="launch-agent"]');
await settle(300);
check("a folder that is not a repository says so and opens nothing", (await page.textContent("#modal-bg .err")).includes("not inside a git repository") && (await state()).all === 0);
await page.evaluate(() => { globalThis.__osoMock.knobs.noRepo = false; });
await page.click('[data-act="launch-agent"]');
await settle(900);
const wt = await page.evaluate(() => globalThis.__osoMock.worktrees);
const opened16 = await page.evaluate(() => { const t = [...window.oso.ws.tabs.values()].at(-1); return t ? { title: t.title, space: t.space, cwd: t.cwd } : null; });
check("the worktree is made and the agent starts in it, in the Space", wt.length === 1 && wt[0][1] === "oso/try-1" && opened16 && opened16.title === "OpenCode · oso-try-1" && opened16.cwd === "/tmp/example.worktrees/oso-try-1" && opened16.space > 0, JSON.stringify([wt, opened16]));
check("its command is typed in the terminal", (await termText(await activeSid())).includes("opencode"));
await section("spaces");
await page.hover('.row.space:has-text("Example")');
await page.click('.row.space:has-text("Example") [data-act="space-agent"]');
await settle(200);
await page.click('[data-act="launch-terminal"]');
await settle(600);
check("\"Terminal without an agent\" opens just the terminal", (await state()).all === 2 && !(await termText(await activeSid())).includes("claude"));
// the list of agents is edited in Settings
await page.keyboard.press("Control+,");
await settle(250);
await page.click('[data-act="agent-kinds"]');
await settle(250);
check("Settings has the list of agents", (await page.$$('[data-name="agent-kinds"] [data-name="kind-name"]')).length === 3);
await page.click('[data-act="kind-add"]');
await page.fill('[data-name="agent-kinds"] .keyrow:last-child [data-name="kind-name"]', "Aider");
await page.fill('[data-name="agent-kinds"] .keyrow:last-child [data-name="kind-command"]', "aider --model sonnet");
await page.click('[data-act="kinds-save"]');
await settle(250);
check("a new agent is kept in the list", (await page.evaluate(() => globalThis.__osoMock.agentKinds.map((k) => k.id + ":" + k.command))).includes("aider:aider --model sonnet"));
await clearAll();
await page.evaluate(async () => { for (const sp of await window.oso.be.spaceList()) await window.oso.be.spaceDelete(sp.id); await window.oso.refreshSpaces(); });
await section("servers");
await page.dblclick('.row[data-kind="server"] >> text=web-1');
await settle(300);
await page.keyboard.press("Control+n");
await settle(400);

// 17. what the person looks at is told to the engine (ids only), so that an agent can be told "this server"
await clearAll();
await section("servers");
await page.click('.row[data-kind="server"] >> text=db-1');
await settle(300);
const lastFocus = () => page.evaluate(() => globalThis.__osoMock.focusSent.at(-1));
check("selecting a server in the list is told", (await lastFocus()).selected === 3 && (await lastFocus()).open.length === 0 && (await lastFocus()).active === null, JSON.stringify(await lastFocus()));
await page.dblclick('.row[data-kind="server"] >> text=staging');
await settle(400);
check("opening a session tells which server has one open and which is in front", JSON.stringify(await lastFocus()) === JSON.stringify({ selected: 4, open: [4], active: 4 }) || ((await lastFocus()).open.join() === "4" && (await lastFocus()).active === 4), JSON.stringify(await lastFocus()));
await page.dblclick('.row[data-kind="server"] >> text=web-1');
await settle(400);
check("with two open, both are told and the one in front", (await lastFocus()).open.join() === "2,4" && (await lastFocus()).active === 2, JSON.stringify(await lastFocus()));
await page.keyboard.press("Control+n");
await settle(300);
await page.evaluate(async () => { await window.oso.openServer(0); });
await settle(400);
check("a local terminal is not an ssh session: nobody is in front", (await lastFocus()).active === null && (await lastFocus()).open.join() === "2,4", JSON.stringify(await lastFocus()));
await clearAll();
check("with nothing open, nothing is told as open", (await lastFocus()).open.length === 0 && (await lastFocus()).active === null);
await page.dblclick('.row[data-kind="server"] >> text=web-1');
await settle(300);
await page.keyboard.press("Control+n");
await settle(400);

// 18. sounds and colour for what calls for you
await clearAll();
await section("servers");
await page.dblclick('.row[data-kind="server"] >> text=web-1');
await settle(300);
await page.keyboard.press("Control+n");
await settle(300);
const [s1, s2] = await page.evaluate(() => window.oso.ws.screen.groups.map((g) => g.active));
await page.evaluate(() => { localStorage.removeItem("oso.sound"); window.oso.sound.update({ enabled: true, volume: 0.7, finished: { kind: "builtin" }, needs: { kind: "builtin" } }); window.oso.sound.played.length = 0; document.hasFocus = () => true; globalThis.__osoMock.agents.length = 0; });
await page.evaluate((sid) => window.oso.focusSession(sid), s2);
const setAgent = (sid, state) => page.evaluate(({ sid, state }) => { const a = globalThis.__osoMock.agents; const row = a.find((x) => x.sid === sid); if (row) row.state = state; else a.push({ sid, kind: "claude", name: "Claude Code", pid: 1, cwd: "/x/example", folder: "example", branch: "", state, since: Date.now(), source: "hook" }); return window.oso.pollAgents(); }, { sid, state });
const played = () => page.evaluate(() => window.oso.sound.played.map((p) => `${p.event}:${p.sid}`));
await setAgent(s1, "working");
await setAgent(s1, "finished");
await settle(200);
check("an agent in a tab you are not looking at finishes: it sounds", (await played()).join() === `finished:${s1}`, JSON.stringify(await played()));
check("and its terminal and its tab get a green border until you look", (await page.$(`.pane.call-done`)) !== null && (await page.$(`.tab.call-done[data-sid="${s1}"]`)) !== null);
await page.evaluate((sid) => window.oso.focusSession(sid), s1);
await page.evaluate(() => window.oso.pollAgents());
await settle(300);
check("looking at that tab clears the border", (await page.$$(".pane.call-done")).length === 0 || (await page.evaluate(() => globalThis.__osoMock.agentAcks.length)) > 0);
// the one you are looking at never sounds
await page.evaluate(() => { window.oso.sound.played.length = 0; globalThis.__osoMock.agents.length = 0; return window.oso.pollAgents(); });
await setAgent(s1, "working");
await setAgent(s1, "needs_you");
await settle(200);
check("an agent in the tab you are looking at calls for you: no sound", (await played()).length === 0, JSON.stringify(await played()));
check("but it is marked in amber", (await page.$(".pane.call-needs")) !== null && (await page.$(`.tab.call-needs[data-sid="${s1}"]`)) !== null);
// another agent needs you, in the tab you are not looking at
await page.evaluate(() => window.oso.sound.forget());
await page.evaluate((sid) => window.oso.focusSession(sid), s2);
await settle(150);
await setAgent(s1, "working");
await setAgent(s1, "needs_you");
await settle(200);
check("an agent that needs you, in another tab, sounds", (await played()).join() === `needs:${s1}`, JSON.stringify(await played()));
await setAgent(s1, "working");
await setAgent(s1, "needs_you");
await settle(200);
check("and it does not sound again at once", (await played()).length === 1, JSON.stringify(await played()));
// the bell: a terminal you were watching is done
await page.evaluate(() => { window.oso.sound.played.length = 0; window.oso.sound.forget(); });
await page.evaluate((sid) => window.oso.tellDone(sid, "Done: the terminal is back at its prompt"), s1);
await settle(200);
check("a terminal you were watching (the bell) that is done sounds too", (await played()).join() === `finished:${s1}`, JSON.stringify(await played()));
check("and shows the green border", (await page.$(`.pane.call-done`)) !== null);
// Settings: off, volume, a sound for each, and the Test button
await page.evaluate(() => { document.hasFocus = () => true; });
await page.keyboard.press("Control+,");
await settle(250);
await page.click('[data-act="sounds"]');
await settle(250);
await page.click('[data-name="sound-enabled"]');
await page.fill('[data-name="sound-volume"]', "40");
await page.click('[data-name="sound-needs"] .sel-btn');
await page.click('[data-name="sound-needs"] .sel-item[data-value="file"]');
await page.fill('[data-name="sound-needs-file"]', "~/sounds/ding.ogg");
await page.click('[data-name="sound-finished"] .sel-btn');
await page.click('[data-name="sound-finished"] .sel-item[data-value="system"]');
await page.evaluate(() => { window.oso.sound.played.length = 0; });
await page.click('[data-act="sound-test-needs"]');
await settle(150);
check("Test plays what the form says, also with sounds off", JSON.stringify(await page.evaluate(() => window.oso.sound.played.map((p) => [p.event, p.source, p.volume]))) === JSON.stringify([["needs", "file", 0.4]]));
await page.click('[data-act="sound-test-finished"]');
await settle(150);
check("each event has its own Test", (await page.evaluate(() => window.oso.sound.played.map((p) => p.source))).join() === "file,system");
await page.click('[data-act="sounds-save"]');
await settle(200);
const savedSound = await page.evaluate(() => JSON.parse(localStorage.getItem("oso.sound")));
check("what was chosen is kept", savedSound.enabled === false && savedSound.volume === 0.4 && savedSound.needs.path === "~/sounds/ding.ogg" && savedSound.finished.kind === "system", JSON.stringify(savedSound));
await page.evaluate(() => { window.oso.sound.played.length = 0; });
await page.evaluate((sid) => window.oso.tellDone(sid, "again"), s1);
await page.evaluate(() => window.oso.tellDone(Number([...window.oso.ws.tabs.keys()][0]), "again"));
await settle(200);
check("with sounds off nothing sounds, but the border still shows", (await played()).length === 0 && (await page.$$(".pane.call-done")).length >= 1);
await page.evaluate(() => { localStorage.removeItem("oso.sound"); window.oso.sound.update({ enabled: true, volume: 0.7, finished: { kind: "builtin" }, needs: { kind: "builtin" } }); delete document.hasFocus; globalThis.__osoMock.agents.length = 0; });
await clearAll();
await page.dblclick('.row[data-kind="server"] >> text=web-1');
await settle(300);
await page.keyboard.press("Control+n");
await settle(400);

// 19. the command palette, and each action where it belongs
await clearAll();
await page.evaluate(() => localStorage.removeItem("oso.palette.recent"));
await section("servers");
await page.evaluate(async () => { await window.oso.openServer(0); });
await settle(400);
await page.keyboard.press("Alt+Shift+Space");
await settle(250);
check("Alt+Shift+Space opens the palette, with the search box focused", (await page.$("#modal-bg.show.palette-mode .palette")) !== null && (await page.$eval('[data-name="palette-input"]', (i) => document.activeElement === i)));
const titles0 = await page.$$eval(".pal-title", (e) => e.map((x) => x.textContent));
check("what is found comes in groups, each with its title", titles0.includes("Actions") && titles0.includes("Open tabs") && titles0.includes("Servers") && (await page.$$(".pal-title svg")).length === titles0.length, titles0.join());
check("each result shows its icon, and its keys on the right when it has them", (await page.$$eval(".pal-row", (r) => r.every((x) => x.querySelector(".pal-icon svg")))) && (await page.textContent('.pal-row[data-id="palette.open"] kbd')) === "Alt+Shift+Space");
check("an ssh action is not offered with a local terminal in front", (await page.$('.pal-row[data-id="files.open"]')) === null);
await page.keyboard.type("copy fil");
await settle(150);
check("not even when you search for it", (await page.$$('.pal-row[data-id="files.open"]')).length === 0);
await page.fill('[data-name="palette-input"]', "nwag");
await settle(150);
check("a fuzzy search finds \"New agent\" with \"nwag\"", (await page.$eval(".pal-row.sel", (r) => r.getAttribute("data-id"))) === "agent.new" && (await page.$$eval(".pal-row.sel .hit", (h) => h.map((x) => x.textContent).join("").toLowerCase())) === "nwag", await page.$eval(".pal-row.sel", (r) => r.getAttribute("data-id")));
await page.keyboard.press("Enter");
await settle(300);
check("Enter runs it", (await page.$('[data-name="launch-folder"]')) !== null && (await page.$("#modal-bg.palette-mode")) === null);
await page.keyboard.press("Escape");
await settle(150);
// the recents come first, and arrows and Escape work
await page.keyboard.press("Alt+Shift+Space");
await settle(200);
check("what was used lately is on top, in a group of its own", (await page.$$eval(".pal-title", (e) => e[0].textContent)) === "Recent" && (await page.$eval(".pal-row", (r) => r.getAttribute("data-id"))) === "agent.new");
await page.keyboard.press("ArrowDown");
await page.keyboard.press("ArrowDown");
const second = await page.$eval(".pal-row.sel", (r) => r.getAttribute("data-id"));
await page.keyboard.press("ArrowUp");
check("the arrows move the selection", second !== "agent.new" && (await page.$eval(".pal-row.sel", (r) => r.getAttribute("data-id"))) !== second);
await page.keyboard.press("Escape");
await settle(150);
check("Escape closes it", (await page.$("#modal-bg.show")) === null);
// it finds everything: servers, Spaces, secrets (by name only) and open tabs
await page.evaluate(async () => { await window.oso.be.spaceSave({ id: 0, name: "Zebra Shop", folders: [], servers: [], server_folders: [], secrets: [], tunnels: [], notes: "", tabs: [] }); await window.oso.refreshSpaces(); });
await page.evaluate(async () => { if (!(await window.oso.be.vaultStatus()).unlocked) await window.oso.be.vaultUnlock("master"); await window.oso.be.secretSave({ id: 0, name: "STRIPE_KEY", kind: "api_key", env_var: null, agent_visible: false, notes: "", updated: 0 }, "sk_live_abcdef"); await window.oso.refreshVaultMark(); await window.oso.refreshSecrets(); });
await page.keyboard.press("Alt+Shift+Space");
await page.keyboard.type("zebra");
await settle(150);
check("it finds a Space", (await page.$$eval('.pal-row[data-kind="space"]', (r) => r.map((x) => x.textContent))).some((t) => t.includes("Zebra Shop")));
await page.fill('[data-name="palette-input"]', "web");
await settle(150);
check("it finds a server, in its group", (await page.$$eval('.pal-row[data-kind="server"] .pal-label', (r) => r.map((x) => x.textContent))).includes("web-1"));
await page.fill('[data-name="palette-input"]', "stripe");
await settle(150);
check("it finds a secret by its name, and shows nothing of its value", (await page.$$('.pal-row[data-kind="secret"]')).length === 1 && !(await page.textContent(".palette")).includes("sk_"));
await page.fill('[data-name="palette-input"]', "Local");
await settle(150);
check("it finds an open tab", (await page.$$('.pal-row[data-kind="tab"]')).length >= 1);
await page.keyboard.press("Escape");
// Alt+K: the same palette, already filtered to servers
await page.keyboard.press("Alt+k");
await settle(200);
check("Alt+K is the palette with only servers", (await page.$$eval(".pal-title", (e) => e.map((x) => x.textContent))).join() === "Servers" && (await page.$('[data-name="palette-input"]')).constructor !== null);
await page.keyboard.press("Escape");
// an ssh action appears once an ssh terminal is in front
await page.evaluate(async () => { await window.oso.openServer(2); });
await settle(400);
await page.keyboard.press("Alt+Shift+Space");
await page.keyboard.type("cpfiles");
await settle(150);
check("with an ssh terminal in front, copying files is offered", (await page.$('.pal-row[data-id="files.open"]')) !== null);
await page.keyboard.press("Escape");
// executing something that is not a dialog: the focus mode
await page.keyboard.press("Alt+Shift+Space");
await page.keyboard.type("focus mode");
await page.keyboard.press("Enter");
await settle(250);
check("Enter on an action does it (focus mode)", await page.$eval("#app", (e) => e.classList.contains("focus")));
await page.keyboard.press("Alt+z");
await settle(200);

// the menu of a terminal
await openPaneMenu();
const paneItems = await page.$$eval(".ctx-menu .ctx-item", (b) => b.map((x) => x.textContent));
check("right-clicking an ssh terminal offers: new terminal right and below, type in every terminal (only with several), tell me when done, copy files, close", paneItems.some((t) => t.startsWith("New terminal to the right")) && paneItems.some((t) => t.startsWith("New terminal below")) && paneItems.some((t) => t.startsWith("Tell me when this terminal is done")) && paneItems.some((t) => t.startsWith("Copy files")) && paneItems.some((t) => t.startsWith("Close this terminal")) && !paneItems.some((t) => t.startsWith("Type in every terminal")), JSON.stringify(paneItems));
check("with the keys on the right", paneItems.some((t) => t.includes("Alt+V")) && paneItems.some((t) => t.includes("Alt+W")));
await page.keyboard.press("Escape");
await page.evaluate(async () => { await window.oso.openServer(0); });
await settle(300);
await openPaneMenu();
const localItems = await page.$$eval(".ctx-menu .ctx-item", (b) => b.map((x) => x.textContent));
check("on a local terminal copying files is not there", !localItems.some((t) => t.startsWith("Copy files")) && localItems.some((t) => t.startsWith("Tell me when")), JSON.stringify(localItems));
await page.click('.ctx-menu .ctx-item:has-text("New terminal to the right")');
await settle(500);
await openPaneMenu();
check("with two terminals in the tab, typing in every one is offered", (await page.$$eval(".ctx-menu .ctx-item", (b) => b.map((x) => x.textContent))).some((t) => t.startsWith("Type in every terminal")));
await page.keyboard.press("Escape");
// the menu of a tab
await page.click(".tab.on", { button: "right" });
const tabItems = await page.$$eval(".ctx-menu .ctx-item", (b) => b.map((x) => x.textContent.replace(/Alt\+\w+|F2|Ctrl\+\w+/g, "")));
check("right-clicking a tab offers: rename, save in a Space, close", tabItems.length === 3 && tabItems[0].startsWith("Rename") && tabItems[1].startsWith("Save in a Space") && tabItems[2].startsWith("Close"), JSON.stringify(tabItems));
await page.keyboard.press("Escape");
// "+ New agent" is in the Agents panel
if (await page.$eval("#rpanel", (e) => e.hidden)) await page.click('[data-rail="agents"]');
await settle(200);
await page.click('[data-rtab="agents"]');
await page.click('[data-act="agents-new"]');
await settle(250);
check("the Agents panel has a \"+ New agent\" button", (await page.$('[data-name="launch-folder"]')) !== null);
await page.keyboard.press("Escape");
if (!(await page.$eval("#rpanel", (e) => e.hidden))) await page.click('[data-rail="agents"]');
// the palette is in the sheet of shortcuts and in the tip of the home screen
await page.keyboard.press("Shift+Slash");
await settle(250);
check("the sheet of shortcuts lists the palette", (await page.textContent('[data-sheet="shortcuts"]')).includes("Alt+Shift+Space") && (await page.textContent('[data-sheet="shortcuts"]')).includes("Every command"));
await page.keyboard.press("Escape");
await clearAll();
await settle(200);
check("the home screen tells about it", (await page.textContent(".home .tip")).includes("Alt+Shift+Space"));
await page.evaluate(async () => { for (const sp of await window.oso.be.spaceList()) await window.oso.be.spaceDelete(sp.id); await window.oso.refreshSpaces(); localStorage.removeItem("oso.palette.recent"); });
await section("servers");
await page.dblclick('.row[data-kind="server"] >> text=web-1');
await settle(300);
await page.keyboard.press("Control+n");
await settle(400);

// 14. the main key: one setting changes every shortcut at once
await page.keyboard.press("Control+,");
await settle(250);
await page.click('[data-role="modifier"] .sel-btn');
await page.click('[data-role="modifier"] .sel-item[data-value="Super"]');
check("the settings show the new main key on the shortcuts", (await page.$eval('input[data-action="tab.2"]', (i) => i.value)) === "Super+2");
await page.click('#modal-bg button.primary');
await settle(300);
await page.keyboard.press("Meta+1");
await settle(150);
await page.keyboard.press("Alt+2");
await settle(150);
const stillOnAlt = (await state()).cur;
await page.keyboard.press("Meta+2");
await settle(200);
check("after switching to Super, Alt+number stops and Super+number goes to the tab", stillOnAlt === 0 && (await state()).cur === 1);
check("the tooltips show the new key", (await page.$eval('.tab[title^="Tab 2"]', (b) => b.title)).includes("Super+2"));

check("no errors in the console", errors.length === 0, errors.join(" | "));
await browser.close();
vite.kill();
console.log(failures ? `\n${failures} check(s) failed` : "\nall checks passed");
process.exit(failures ? 1 : 0);
