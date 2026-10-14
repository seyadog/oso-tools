// Kubernetes: the clusters in the left panel, and the console of each one as a tab of the top bar (ready-made commands,
// a free command and its pods). Everything that is a command opens a terminal tab; the lists are read with the engine
// (reads only).

import { Backend, Cluster, Server, TunnelStatus } from "./api";
import { clear, confirmBox, el, field, makeSelect, openModal, toast } from "./dom";
import { serverColor } from "./icons";
import { copyButton } from "./tunnels_ui";
import { Pod, QUICK, changes, health, parseNames, parsePods, quote } from "./k8s";

export interface K8sCtx {
  be: Backend;
  servers(): Server[];
  clusters(): Cluster[];
  /** Opens a terminal tab for the cluster and types `rest` (the words after kubectl) in it; "" opens a plain terminal. */
  run(c: Cluster, rest: string, title: string): void;
  /** Reloads the clusters and draws the panel again. */
  changed(): Promise<void>;
  rerender(): void;
  /** Opens (or shows) the console of a cluster as a tab of the top bar. */
  openTab(c: Cluster): void;
  /** How the tunnel of a cluster that goes through a server is doing (nothing: it is off). */
  tunnel(c: Cluster): TunnelStatus | undefined;
  /** Switches that tunnel on or off. */
  switchTunnel(c: Cluster, on: boolean): Promise<void>;
  emptyBox(text: string, buttons: [string, () => void, boolean, string][]): HTMLElement;
  ibtn(name: string, tip: string, run: () => void, primary: boolean, act: string): HTMLElement;
}

interface Open {
  /** "" = the cluster's own namespace; "*" = every namespace. */
  ns: string;
  namespaces: string[];
  pods: Pod[] | null;
  all: boolean;
  error: string;
  loading: boolean;
  /** Only the newest answer counts. */
  token: number;
}

/** The consoles that are open (one tab each): what their pod list shows, and how to draw them again. */
const consoles = new Map<number, { box: HTMLElement; o: Open }>();

/** The cluster rows of the left panel. Clicking one opens its console as a tab of the top bar. */
export function renderClusters(root: HTMLElement, ctx: K8sCtx) {
  const list = ctx.clusters();
  if (!list.length) {
    root.append(ctx.emptyBox("Add a Kubernetes cluster to see its pods and run kubectl from here. It can be reached from this computer (a kubeconfig) or through one of your servers (k3s, microk8s).", [["Add a cluster", () => clusterForm(ctx, null), true, "empty-add-cluster"]]));
    return;
  }
  for (const c of list) {
    const via = c.via_server !== null ? ctx.servers().find((s) => s.id === c.via_server)?.name ?? "a server that is gone" : "";
    const row = el("div", { class: `row cluster${consoles.has(c.id) ? " sel" : ""}`, "data-cluster": String(c.id) });
    const st = c.via_server !== null ? ctx.tunnel(c) : undefined;
    const addr = `https://127.0.0.1:${c.local_port || 16443 + c.id}`;
    const sub = st?.state === "up" ? `${addr} · via ${via}` : st?.state === "failed" ? st.message : [via ? `via ${via}` : c.kubeconfig ? c.kubeconfig.split("/").pop() : "this computer", c.context, c.namespace ? `ns ${c.namespace}` : ""].filter(Boolean).join(" · ");
    const dot = el("span", { class: "dot", style: `background:${c.env_kind === "production" ? "var(--red)" : serverColor(100000 + c.id)}` });
    row.append(dot, el("span", { class: "txt", title: sub }, el("b", {}, c.name), el("small", {}, sub)));
    const edit = ctx.ibtn("edit", "Edit this cluster", () => clusterForm(ctx, c), false, "edit");
    edit.style.cssText = "width:24px;height:24px";
    edit.onclick = (e) => {
      e.stopPropagation();
      clusterForm(ctx, c);
    };
    const term = ctx.ibtn("terminal", "Open a terminal for this cluster", () => ctx.run(c, "", c.name), false, "go");
    term.style.cssText = "width:24px;height:24px";
    term.onclick = (e) => {
      e.stopPropagation();
      ctx.run(c, "", c.name);
    };
    row.append(...(st?.state === "up" ? [copyButton(addr)] : []), edit, term);
    if (c.via_server !== null) {
      const running = !!st && st.state !== "failed";
      const sw = ctx.ibtn(running ? "close" : "play", running ? "Close the tunnel to this cluster" : "Open a tunnel: this computer's kubectl (or k9s) can reach the cluster", () => {}, false, "cluster-tunnel");
      sw.style.cssText = "width:24px;height:24px";
      sw.onclick = (e) => {
        e.stopPropagation();
        void ctx.switchTunnel(c, !running);
      };
      row.append(sw);
      if (st?.state === "up") row.classList.add("tunnel-up");
      if (st?.state === "failed") row.classList.add("tunnel-failed");
    }
    row.title = "Open the console of this cluster";
    row.onclick = () => ctx.openTab(c);
    root.append(row);
  }
}

/** The console of a cluster, to put in a tab: ready-made commands, a free command and the pods. Draws itself again when asked. */
export function consoleView(ctx: K8sCtx, clusterId: number): HTMLElement {
  const old = consoles.get(clusterId);
  if (old) return old.box;
  const box = el("div", { class: "k8s-console", "data-console": String(clusterId) });
  const o: Open = { ns: "", namespaces: [], pods: null, all: false, error: "", loading: false, token: 0 };
  consoles.set(clusterId, { box, o });
  const c = ctx.clusters().find((x) => x.id === clusterId);
  if (c) void load(ctx, c, o, true);
  else redrawConsole(ctx, clusterId);
  return box;
}

/** The tab of a cluster is gone: forget what it showed. */
export function dropConsole(clusterId: number) {
  const k = consoles.get(clusterId);
  if (k) k.o.token++;
  consoles.delete(clusterId);
}

export function redrawConsole(ctx: K8sCtx, clusterId: number) {
  const k = consoles.get(clusterId);
  if (!k) return;
  clear(k.box);
  const c = ctx.clusters().find((x) => x.id === clusterId);
  if (!c) return void k.box.append(el("div", { class: "empty k8s-note" }, "This cluster was deleted."));
  k.box.append(el("h2", { class: "k8s-title" }, c.name, el("small", {}, [c.via_server !== null ? `via ${ctx.servers().find((s) => s.id === c.via_server)?.name ?? "a server that is gone"}` : "this computer", c.context, c.namespace ? `ns ${c.namespace}` : ""].filter(Boolean).join(" · "))), detail(ctx, c, k.o));
}

/** Reads the pods (and, the first time, the namespaces) of a cluster. */
async function load(ctx: K8sCtx, c: Cluster, o: Open, withNamespaces: boolean) {
  const mine = ++o.token;
  o.loading = true;
  o.error = "";
  redrawConsole(ctx, c.id);
  const all = o.ns === "*";
  const args = ["get", "pods", "--no-headers", ...(all ? ["-A"] : o.ns ? ["-n", o.ns] : [])];
  try {
    const [pods, names] = await Promise.all([ctx.be.clusterRead(c.id, args), withNamespaces ? ctx.be.clusterRead(c.id, ["get", "namespaces", "--no-headers", "-o", "custom-columns=NAME:.metadata.name"]).catch(() => "") : Promise.resolve(null)]);
    if (mine !== o.token) return;
    o.pods = parsePods(pods, all);
    o.all = all;
    if (names !== null) o.namespaces = parseNames(names);
    // a namespace that is not there (a capital letter is enough) would only say "no pods": say what is wrong
    if (!o.ns && c.namespace && o.namespaces.length && !o.namespaces.includes(c.namespace)) {
      const alike = o.namespaces.find((n) => n.toLowerCase() === c.namespace.toLowerCase());
      o.pods = null;
      o.error = alike ? `There is no namespace “${c.namespace}” in this cluster. Did you mean “${alike}”? Edit the cluster (the pencil) to change it.` : `There is no namespace “${c.namespace}” in this cluster. The ones that exist: ${o.namespaces.join(", ")}.`;
    }
  } catch (e) {
    if (mine !== o.token) return;
    o.pods = null;
    o.error = (e as Error).message;
  }
  o.loading = false;
  redrawConsole(ctx, c.id);
}

function detail(ctx: K8sCtx, c: Cluster, o: Open): HTMLElement {
  const box = el("div", { class: "k8s-detail", "data-detail": String(c.id) });
  // the ready-made commands
  const quick = el("div", { class: "k8s-quick" });
  for (const q of QUICK) {
    const b = el("button", { class: "chip", type: "button", title: `kubectl ${q.rest}`, "data-quick": q.id }, q.label);
    b.onclick = () => ctx.run(c, q.rest, `${c.name} · ${q.label.toLowerCase()}`);
    quick.append(b);
  }
  box.append(quick);
  // any other command
  const free = el("input", { class: "k8s-free", placeholder: "kubectl …  (Enter)", spellcheck: "false", autocomplete: "off", "data-name": "k8s-free" }) as HTMLInputElement;
  free.onkeydown = (e) => {
    // the window's own keys must not see what is typed here
    e.stopPropagation();
    if (e.key !== "Enter") return;
    const rest = free.value.trim().replace(/^kubectl\s+/, "");
    if (!rest) return;
    free.value = "";
    ctx.run(c, rest, `${c.name} · ${rest.split(/\s+/).slice(0, 2).join(" ")}`);
  };
  box.append(free);
  // the pods
  const head = el("div", { class: "k8s-podhead" });
  const sel = makeSelect([["", c.namespace ? `Namespace ${c.namespace}` : "Default namespace"], ["*", "Every namespace"], ...o.namespaces.filter((n) => n !== c.namespace).map((n): [string, string] => [n, n])], o.ns, { "data-name": "k8s-ns" });
  sel.onchange = () => {
    o.ns = sel.value;
    void load(ctx, c, o, false);
  };
  const refresh = ctx.ibtn("refresh", "Read the pods again", () => void load(ctx, c, o, false), false, "k8s-refresh");
  refresh.style.cssText = "width:24px;height:24px";
  head.append(el("span", { class: "k8s-h" }, "Pods"), sel, refresh);
  box.append(head);
  if (o.loading && !o.pods) box.append(el("div", { class: "empty k8s-note" }, "Reading…"));
  else if (o.error) box.append(el("div", { class: "err k8s-note", "data-k8s-error": "1" }, o.error));
  else if (o.pods && !o.pods.length) box.append(el("div", { class: "empty k8s-note" }, "No pods here."));
  for (const p of o.pods ?? []) box.append(podRow(ctx, c, p, o));
  return box;
}

function podRow(ctx: K8sCtx, c: Cluster, p: Pod, o: Open): HTMLElement {
  const ns = p.ns || (o.ns && o.ns !== "*" ? o.ns : "");
  const nsFlag = ns ? `-n ${quote(ns)} ` : "";
  const name = quote(p.name);
  const h = health(p);
  const row = el("div", { class: `row pod ${h}`, "data-pod": p.name, title: `${p.ns ? p.ns + "/" : ""}${p.name}\n${p.status}, ready ${p.ready}, restarts ${p.restarts || 0}, ${p.age}` });
  row.append(el("span", { class: `pdot ${h}` }), el("span", { class: "txt" }, el("b", {}, p.name), el("small", {}, `${p.ns ? p.ns + " · " : ""}${p.status} · ${p.ready} · ${p.age}${p.restarts && p.restarts !== "0" ? ` · ↻ ${p.restarts}` : ""}`)));
  const act = (iconName: string, tip: string, act: string, run: () => void) => {
    const b = ctx.ibtn(iconName, tip, run, false, act);
    b.style.cssText = "width:22px;height:22px";
    b.classList.add("pod-act");
    b.onclick = (e) => {
      e.stopPropagation();
      run();
    };
    row.append(b);
  };
  act("log", "Follow its logs", "pod-logs", () => ctx.run(c, `logs -f --tail 200 ${nsFlag}${name}`, `${p.name} · logs`));
  act("terminal", "Open a shell inside it", "pod-shell", () => ctx.run(c, `exec -it ${nsFlag}${name} -- sh`, `${p.name} · shell`));
  act("info", "Describe it (events, why it does not start)", "pod-describe", () => ctx.run(c, `describe pod ${nsFlag}${name}`, `${p.name} · describe`));
  act("trash", "Delete this pod (a deployment starts a new one)", "pod-delete", async () => {
    const rest = `delete pod ${nsFlag}${name}`;
    if (changes(rest) && !(await confirmBox(`Delete the pod “${p.name}”?`, c.env_kind === "production" ? `This is a PRODUCTION cluster (${c.name}). A deployment or a replica set will start a new pod; a pod on its own is gone.` : "A deployment or a replica set will start a new pod; a pod on its own is gone.", "Delete"))) return;
    ctx.run(c, rest, `${p.name} · delete`);
    window.setTimeout(() => void load(ctx, c, o, false), 2500);
  });
  return row;
}

/** Add (`null`) or edit a cluster. */
export function clusterForm(ctx: K8sCtx, existing: Cluster | null) {
  const c: Cluster = existing ?? { id: 0, name: "", via_server: null, kubectl: "", kubeconfig: "", context: "", namespace: "", ai_policy: "ask", env_kind: null, notes: "", api_port: 0, local_port: 0 };
  openModal((close) => {
    const name = el("input", { value: c.name, name: "name", autocomplete: "off", placeholder: "for example: k3s lab" });
    const via = makeSelect([["", "This computer (a kubeconfig)"], ...ctx.servers().map((s): [string, string] => [String(s.id), `${s.name} (kubectl runs there, over ssh)`])], c.via_server === null ? "" : String(c.via_server), { "data-name": "cluster-via" });
    const kubeconfig = el("input", { value: c.kubeconfig, name: "kubeconfig", placeholder: "~/.kube/config (empty: kubectl's own)", spellcheck: "false" });
    const kubeRow = field("Kubeconfig file", kubeconfig);
    const program = el("input", { value: c.kubectl, name: "kubectl", placeholder: "kubectl   (k3s: k3s kubectl)", spellcheck: "false" });
    const context = el("input", { value: c.context, name: "context", placeholder: "empty: the one kubectl has selected", spellcheck: "false" });
    const namespace = el("input", { value: c.namespace, name: "namespace", placeholder: "empty: default", spellcheck: "false" });
    const env = makeSelect([["", "Not set"], ["production", "Production"], ["staging", "Staging"], ["dev", "Development"]], c.env_kind ?? "", { "data-name": "cluster-env" });
    const policy = makeSelect([["ask", "Ask me every time"], ["read_only_auto", "Let commands that only read go through"], ["blocked", "Never let an agent use it"]], c.ai_policy, { "data-name": "cluster-ai" });
    const policyRow = field("When an AI agent asks to use it", policy);
    const apiPort = el("input", { value: c.api_port ? String(c.api_port) : "", name: "api_port", inputmode: "numeric", placeholder: "6443" });
    const localPort = el("input", { value: c.local_port ? String(c.local_port) : "", name: "local_port", inputmode: "numeric", placeholder: `${16443 + c.id}` });
    const portsRow = el("div", { class: "grid2" }, field("API port on the server", apiPort), field("Tunnel port here", localPort));
    const notes = el("textarea", { name: "notes", rows: "2", placeholder: "What it is, for agents and for you" }, c.notes) as HTMLTextAreaElement;
    const sync = () => {
      kubeRow.style.display = via.value ? "none" : "";
      policyRow.style.display = via.value ? "none" : "";
      portsRow.style.display = via.value ? "" : "none";
      program.placeholder = via.value ? "kubectl   (k3s: k3s kubectl, microk8s: microk8s kubectl)" : "kubectl";
    };
    via.onchange = sync;
    sync();
    const err = el("div", { class: "err" });
    const form = el("form", {}, el("h2", {}, existing ? `Edit ${c.name}` : "New cluster"), field("Name", name), field("Where kubectl runs", via), kubeRow, field("Program", program), el("div", { class: "grid2" }, field("Context", context), field("Namespace", namespace)), el("div", { class: "grid2" }, field("What it is for", env), policyRow), portsRow, el("div", { class: "hint" }, via.value ? "Through a server, the server's own AI rule applies. A production machine always asks." : "A production cluster always asks."), field("Notes (agents read them)", notes), err);
    via.addEventListener("change", () => {
      (form.querySelector(".hint") as HTMLElement).textContent = via.value ? "Through a server, the server's own AI rule applies. A production machine always asks." : "A production cluster always asks.";
    });
    const ok = el("button", { class: "btn primary", type: "submit" }, "Save");
    const no = el("button", { class: "btn", type: "button" }, "Cancel");
    no.onclick = close;
    const actions = el("div", { class: "actions" });
    if (existing) {
      const del = el("button", { class: "btn danger", type: "button", "data-act": "cluster-delete" }, "Delete");
      del.onclick = async () => {
        if (!(await confirmBox(`Delete the cluster “${c.name}”?`, "Only oso forgets it: the cluster itself is not touched.", "Delete"))) return;
        await ctx.be.clusterDelete(c.id);
        close();
        await ctx.changed();
      };
      actions.append(del);
    }
    actions.append(no, ok);
    form.append(actions);
    form.onsubmit = async (e) => {
      e.preventDefault();
      if (!name.value.trim()) return void (err.textContent = "The cluster needs a name");
      const next: Cluster = { ...c, name: name.value.trim(), via_server: via.value ? Number(via.value) : null, kubeconfig: via.value ? "" : kubeconfig.value.trim(), kubectl: program.value.trim(), context: context.value.trim(), namespace: namespace.value.trim(), env_kind: (env.value || null) as Cluster["env_kind"], ai_policy: policy.value as Cluster["ai_policy"], notes: notes.value.trim(), api_port: via.value ? Number(apiPort.value) || 0 : 0, local_port: via.value ? Number(localPort.value) || 0 : 0 };
      for (const v of [next.api_port, next.local_port]) if (v && (v < 1 || v > 65535)) return void (err.textContent = "A port is a number from 1 to 65535");
      try {
        const id = await ctx.be.clusterSave(next);
        close();
        await ctx.changed();
        if (!existing) {
          toast(`“${next.name}” added`);
          ctx.openTab({ ...next, id });
        }
      } catch (x) {
        err.textContent = (x as Error).message;
      }
    };
    return form;
  });
}

export function clearK8s(root: HTMLElement) {
  clear(root);
}
