import "./style.css";
import { loadBackend } from "./api";
import { App } from "./app";
import { mountUnlock } from "./unlock_ui";

const root = document.getElementById("app")!;
const unlockOnly = new URLSearchParams(location.search).has("unlock");
loadBackend()
  .then((be) => {
    if (unlockOnly) {
      return import("@tauri-apps/api/core").then(({ invoke }) => mountUnlock(be, root, () => void invoke("close_unlock_window")));
    }
    const app = new App(be, root);
    (window as unknown as { oso: App }).oso = app; // for tests and the console
    return app.start();
  })
  .catch((e) => {
    root.textContent = `oso could not start: ${e}`;
    console.error(e);
  });
