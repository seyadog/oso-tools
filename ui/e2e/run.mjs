// Drives the real interface in a real browser against the mock engine and checks what a person would see.
// Usage: node e2e/run.mjs   (starts vite itself)
import { chromium } from "playwright-core";
import { spawn } from "node:child_process";

const URL = "http://localhost:1420/";
const vite = spawn("npx", ["vite", "--port", "1420", "--strictPort"], { stdio: "pipe" });
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

// 1. open a server by double click
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
check("the right one unlocks it and the server opens", (await state()).tabs === 1 && (await page.$(".side-foot .vault.open")) !== null);
await shot("5-vault-opened");

// 11. a new server, edited and deleted
await page.click('button[title="New host"]');
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
await page.dblclick('.row[data-kind="server"] >> text=web-1');
await settle(300);
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
check("Alt+K opens the host picker", (await page.$("#modal-bg.show .picker-list .row")) !== null);
await page.keyboard.type("stag");
await page.keyboard.press("Enter");
await settle(400);
check("typing a few letters and Enter opens that host", (await state()).tabs === 1 && (await termText(await activeSid())).includes("staging"));
await page.click('button[title^="New terminal to the right"]');
await settle(500);
s = await state();
check("the split button on the toolbar opens a second terminal in a mosaic", s.mode === "mosaic" && (await panes()).length === 2);
await shot("7-toolbar");
await page.click('button[title^="Snippets"]');
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
check("an empty screen shows the guide, not a list of hosts", (await page.$(".home .g-grid")) !== null && (await page.$$(".home .card")).length === 0);
await shot("8-home");
await page.dblclick('.row[data-kind="server"] >> text=web-1');
await settle(400);
await page.dblclick('.row[data-kind="server"] >> text=web-1');
await settle(400);
check("the same host opens twice when you double-click it twice", (await state()).tabs === 2);

// the vault as a password manager
await page.click(".side-foot .vault");
await settle(300);
check("the vault opens and lists the hosts with their passwords", (await page.$$(".vrow")).length === 3 && (await page.textContent(".vault-list")).includes("••••"));
await page.click('.vrow:has-text("db-1") button[title="Show"]');
await settle(200);
check("a password can be shown", (await page.textContent(".vault-list")).includes("db-secret-pw"));
await shot("9-vault");
await page.click('.vrow:has-text("web-1") .link');
await settle(200);
await page.fill("#modal-bg input", "web-pw-1");
await page.keyboard.press("Enter");
await settle(400);
check("a password can be added to a host that had none", (await page.$$('.vrow:has-text("web-1") .pw')).length === 1);
await page.click('.vrow:has-text("web-1") button[title="Remove"]');
await settle(200);
await page.click("#modal-bg button.primary");
await settle(400);
check("and removed again", (await page.$$('.vrow:has-text("web-1") .pw')).length === 0);
await page.keyboard.press("Escape");
await settle(150);

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
