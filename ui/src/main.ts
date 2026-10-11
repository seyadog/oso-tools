import "./style.css";
import { loadBackend } from "./api";
import { App } from "./app";

const root = document.getElementById("app")!;
loadBackend()
  .then((be) => {
    const app = new App(be, root);
    (window as unknown as { oso: App }).oso = app; // for tests and the console
    return app.start();
  })
  .catch((e) => {
    root.textContent = `oso could not start: ${e}`;
    console.error(e);
  });
