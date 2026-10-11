<p align="center">
  <img src="src-tauri/icons/icon.png" width="160" alt="oso, a brown bear sleeping on its back">
</p>

# oso

A graphical SSH workspace for Linux: your servers in a tree, terminals that tile themselves,
**tabs** like a browser's (`Alt+1`…`Alt+9`); a tab is one terminal, or several tiled together, and hotkeys
you can change. Passwords live in an encrypted vault and are typed for you at the prompt they belong to.

Servers, folders and the vault live in `~/.config/oso` (`servers.json`, `vault.json`); layout, hotkeys and snippets are saved there too.

## How it is built

| Part | What it is |
|---|---|
| `core/` (`oso-core`, Rust) | The engine, with no interface in it: servers and folders, the vault, `ssh` command building (jump hosts), SSH sessions, and the **background server** that keeps sessions alive when the window closes. `engine.rs` is everything the window can ask for. |
| `src-tauri/` (Rust, Tauri 2) | The window. Thin glue: one command per engine call, and session output sent to the page as events. The same binary started as `oso daemon` is the background server. |
| `ui/` (TypeScript, Vite, xterm.js) | The interface. `tree.ts` (the tiling tree), `workspace.ts` (tabs and mosaic: all the state), `hotkeys.ts`, `pane.ts` (a terminal), `app.ts` (everything on screen). `api.mock.ts` stands in for the engine so the interface runs in a plain browser. |

## Using it

- **Tabs:** one row, like a browser: `Alt+1`…`Alt+8` go to that tab, `Alt+9` to the last one, `Alt+,` / `Alt+.` to the previous / next. Numbers have no gaps (there is no tab 3 without tab 2). Nothing happens for a number with no tab.
- **Tabs and mosaic:** every terminal is born as a tab (numbered 1, 2, 3…). Drag a tab down onto the screen and it joins the one you are looking at, side by side: the two become *one* tab (it shows a ▦ and the count). Drag a terminal by its title up to the tab bar and it is a tab again. Hosts from the left list can be dragged onto the screen too. `Alt+V` / `Alt+H` open the next terminal inside the current tab (right / below); `Alt+M` joins every tab in one mosaic, and again sets them apart. New terminals inside a mosaic tile automatically (beside if the box is wide, below if tall, never too small).
- **Placing terminals:** `Alt+V` / `Alt+H` open a new one to the right / below. Drag a terminal by its title (or a tab from the bar) to the edge of another: it docks there and takes half; in the middle it trades places. Drag the line between two to resize. `Alt+arrows` move the keyboard between terminals, `Alt+Shift+arrows` move the terminal itself.
- **Closing:** the **×** on a terminal, or `Alt+W`. A session that ended cleanly closes itself; one that failed stays with `Enter: reconnect`.
- **Broadcast:** `Alt+B` / `⇉ all`: what you type goes to every terminal of the tab.
- **Open a host:** double-click it in the list, the ▶ on it, drag it onto the screen, or `Alt+K` (type a few letters, Enter). Snippets (saved commands) are in the toolbar; Settings has the hotkeys.
- **Servers:** the same host can be opened as many times as you want. Drag a server onto a folder to file it. Servers can go through a jump host. The vault (key icon, bottom left) is also a password viewer: read, copy, change or remove each saved password.
- **Hotkeys:** `Ctrl+,` opens Settings. The **main key** (Alt by default) can be changed to Ctrl, Super or Ctrl+Alt there, and every shortcut that starts with it changes at once; any single shortcut can also be changed. The file is `~/.config/oso/hotkeys.json` (`{"modifier": "Super", "mode.toggle": "Super+M", "tab.close": null}`; `Mod+X` in an entry means "main key + X"). Keys are matched by position, so the same shortcuts work on every keyboard layout.

## Developing

```sh
# once: the system libraries Tauri needs (Fedora)
sudo dnf install -y webkit2gtk4.1-devel gtk3-devel libsoup3-devel javascriptcoregtk4.1-devel \
    librsvg2-devel dbus-devel glib2-devel cairo-devel pango-devel atk-devel gdk-pixbuf2-devel libxdo-devel openssl-devel
cargo install tauri-cli --version "^2" --locked

cargo tauri dev            # the app
cd ui && npm run dev       # the interface alone, in a browser, with the mock engine

cargo test -p oso-core     # engine: servers, vault, sessions, background server
cd ui && npm test          # tiling tree, tabs, hotkeys
cd ui && npm run e2e       # the real interface in a real browser, driven like a person would
```

## Author

Made by [seyadog](https://github.com/seyadog).
