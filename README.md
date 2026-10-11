<p align="center">
  <img src="branding/oso.svg" width="160" alt="oso, a bear-shaped shield with a keyhole nose">
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
- **Broadcast:** `Alt+B` (or the broadcast icon): what you type goes to every terminal of *this tab*, on or off. It belongs to the tab: other tabs are not touched, and a tab keeps it across restarts.
- **Groups:** arrange some terminals in a tab, then `Alt+G` (the save icon, or right-click the tab → *Save as group*). A group keeps which server each terminal is, how they are arranged, whether broadcast is on, and an optional command per terminal. Groups are listed on the left; double-click one and all its terminals open together as one tab. The commands are typed only once the terminal shows a prompt, never at a password question. Saved in `~/.config/oso/groups.json`.
- **Reconnecting:** if a connection drops (wifi, a laptop that slept), the terminal counts down and connects again by itself, with growing waits (2 s up to 1 min, 8 tries), keeping what it showed and marking where it reconnected. `Enter` connects at once, `Esc` stops. It never retries after a refused login (a wrong password could lock the account). A new connection is a new shell: programs that ran on the server are gone (use `tmux` on the server to keep them).
- **Changed server key:** if a server shows a different key than the saved one, oso says what that means in plain words, shows the new fingerprint and offers to forget the old key (`ssh-keygen -R`).
- **Tell me when it is done:** after you start a long command, `Alt+A` (the bell). When the terminal is back at its prompt (or has been silent for 20 s, or its connection ended) you get a message, a desktop notification if you are looking elsewhere (`notify-send`), and a mark on the tab. It speaks once.
- **Tunnels:** `Alt+T` (or the tunnel icon, bottom left). Port forwards per server, each with a switch: *local* (a port here that leads to something the server can reach, `ssh -L`), *remote* (`ssh -R`) and *SOCKS proxy* (`ssh -D`). They use the same login and jump hosts as a terminal, and stay on while oso is open. A failure is shown next to the tunnel in words (port in use, login refused…).
- **Files:** drag files or folders from your file manager onto a terminal to upload them to its server, or `Alt+U` to type the paths (it also downloads). Uses `scp` with the same login and jump hosts; progress shows at the bottom right.
- **Open a host:** double-click it in the list, the ▶ on it, drag it onto the screen, or `Alt+K` (type a few letters, Enter). Snippets (saved commands) are in the toolbar; Settings has the hotkeys.
- **Servers:** the same host can be opened as many times as you want. Drag a server onto a folder to file it. Servers can go through a jump host. 
- **Secrets:** the key icon (bottom left) opens the vault: named, typed secrets (password, sudo password, SSH key, API key, token, one-time codes), each with an optional profile and an optional environment variable. Filter by text, type or profile; show, copy (the clipboard is cleared after 30 s), change or delete each one; secrets not changed for 90 days are marked. Names and notes are encrypted too. A server's saved passwords are secrets like the others. A vault from an older oso is migrated when it is opened, and the old file is kept as `vault.json.v1.bak`.
- **Hotkeys:** `Ctrl+,` opens Settings. The **main key** (Alt by default) can be changed to Ctrl, Super or Ctrl+Alt there, and every shortcut that starts with it changes at once; any single shortcut can also be changed. The file is `~/.config/oso/hotkeys.json` (`{"modifier": "Super", "mode.toggle": "Super+M", "tab.close": null}`; `Mod+X` in an entry means "main key + X"). Keys are matched by position, so the same shortcuts work on every keyboard layout.

## Developing

```sh
# once: the system libraries Tauri needs (Fedora)
sudo dnf install -y webkit2gtk4.1-devel gtk3-devel libsoup3-devel javascriptcoregtk4.1-devel \
    librsvg2-devel dbus-devel glib2-devel cairo-devel pango-devel atk-devel gdk-pixbuf2-devel libxdo-devel openssl-devel
cargo install tauri-cli --version "^2" --locked

cargo tauri dev            # the app
cd ui && npm run dev       # the interface alone, in a browser, with the mock engine

cargo test -p oso-core     # engine: servers, vault, sessions, background server; tunnels and file copies
                           # run against a real sshd on a high port (skipped if sshd is not installed)
cd ui && npm test          # tiling tree, tabs, groups, hotkeys, reconnect rules
cd ui && npm run e2e       # the real interface in a real browser, driven like a person would
```

## Author

Made by [seyadog](https://github.com/seyadog).
