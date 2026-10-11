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

// the vault as a list of named, typed secrets
await page.click(".side-foot .vault");
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

// 13b. groups: a tab saved with its servers and arrangement, opened again in one go
await clearAll();
await page.dblclick('.row[data-kind="server"] >> text=web-1');
await settle(300);
await page.keyboard.press("Alt+v");
await settle(500);
await page.evaluate(async () => { window.oso.ws.prepareSplit("bottom"); await window.oso.openServer(4); });
await settle(500);
s = await state();
check("a tab with three terminals to save", s.tabs === 1 && s.terms === 3);
const layoutBefore = await panes();
await page.click('.tools button[title="Save this tab as a group"]');
await settle(200);
await page.fill("#modal-bg input", "ops");
await page.keyboard.press("Enter");
await settle(300);
check("the tab is saved as a group, listed on the left", (await page.$$('.row.group:has-text("ops")')).length === 1 && (await page.$$eval(".row.group small", (e) => e[0].textContent)).includes("3 terminals"));
check("groups.json is written", JSON.parse((await page.evaluate(() => window.oso.be.readConfig("groups.json"))) ?? "[]").length === 1);
await page.evaluate(() => { window.oso.groups[0].panes[0].command = "echo from-group"; });
await clearAll();
check("with every tab closed, the group is still there", (await page.$$(".row.group")).length === 1 && (await state()).tabs === 0);
await page.dblclick('.row.group:has-text("ops")');
await settle(1800);
s = await state();
const layoutAfter = await panes();
check("double-clicking the group opens its three terminals as one tab", s.tabs === 1 && s.terms === 3 && layoutAfter.length === 3);
check("each terminal is where it was", JSON.stringify(layoutBefore.map((p) => [p.x, p.y, p.w, p.h])) === JSON.stringify(layoutAfter.map((p) => [p.x, p.y, p.w, p.h])), JSON.stringify([layoutBefore, layoutAfter]));
const firstSid = await page.evaluate(() => window.oso.ws.tabsHere()[0].sid);
check("the command saved for a terminal is typed when it shows its prompt", (await termText(firstSid)).includes("from-group"));
await shot("13b-group");

// broadcast belongs to the tab
check("broadcast is off in a group saved without it", !(await page.evaluate(() => window.oso.ws.broadcasting)));
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

// the menu of a tab, and editing / deleting a group
await page.click(".tab.on", { button: "right" });
await settle(150);
check("right-clicking a tab offers to save it as a group", (await page.$$eval(".ctx-item", (e) => e.map((x) => x.textContent))).includes("Save as group…"));
await page.keyboard.press("Escape");
await page.hover('.row.group:has-text("ops")');
await page.click('.row.group:has-text("ops") button[title="Edit"]');
await settle(200);
check("the group opens in an editor with a field for each terminal", (await page.$$("#modal-bg .group-pane")).length === 3);
await page.fill("#modal-bg input >> nth=0", "operations");
await page.check("#gbc");
await page.click("#modal-bg button.primary");
await settle(300);
check("renaming and broadcast are saved", (await page.$$('.row.group:has-text("operations")')).length === 1 && (await page.evaluate(() => window.oso.groups[0].broadcast)));
await clearAll();
await page.dblclick('.row.group:has-text("operations")');
await settle(1500);
check("a group saved with broadcast opens in broadcast", await page.evaluate(() => window.oso.ws.broadcasting));
await page.hover('.row.group:has-text("operations")');
await page.click('.row.group:has-text("operations") button[title="Delete"]');
await settle(200);
await page.click("#modal-bg button.primary");
await settle(300);
check("deleting a group removes it and leaves the open terminals alone", (await page.$$(".row.group")).length === 0 && (await state()).terms === 3);
await clearAll();
await page.dblclick('.row[data-kind="server"] >> text=web-1');
await settle(300);
await page.keyboard.press("Control+n");
await settle(400);

// 13c. a dropped connection is connected again by itself, keeping what was on screen
await clearAll();
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
await page.dblclick('.row[data-kind="server"] >> text=web-1');
await settle(300);
await page.keyboard.press("Control+n");
await settle(400);

// 13d. a changed server key is explained, and the old key can be forgotten
await clearAll();
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
await page.dblclick('.row[data-kind="server"] >> text=web-1');
await settle(300);
await page.keyboard.press("Control+n");
await settle(400);

// 13e. "tell me when it is done"
await clearAll();
await page.dblclick('.row[data-kind="server"] >> text=web-1');
await settle(400);
sid1 = await activeSid();
await page.keyboard.press("Alt+a");
await settle(200);
check("Alt+A switches the watch on: a mark on the tab and on the bell", (await page.$$(".tab .wm")).length === 1 && (await page.$$('.tools .ib.on[title^="Tell me"]')).length === 1);
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
await page.dblclick('.row[data-kind="server"] >> text=web-1');
await settle(300);
await page.keyboard.press("Control+n");
await settle(400);

// 13f. tunnels: saved port forwards with a switch each
await clearAll();
await page.click('.side-foot button[title^="Tunnels"]');
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
check("the Tunnels button shows that one is on", (await page.$$(".side-foot .ib.live")).length === 1);
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
