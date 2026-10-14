// Kubernetes, the parts that are decisions and not drawing: the ready-made commands, and reading what `kubectl get pods`
// prints. The engine builds the real command line (kubeconfig, context, namespace); these are only the words after kubectl.

export interface Quick {
  id: string;
  label: string;
  /** The words after kubectl. */
  rest: string;
}

/** The commands the panel offers for a cluster, each opening a terminal tab. */
export const QUICK: Quick[] = [
  { id: "pods", label: "Pods", rest: "get pods" },
  { id: "pods-all", label: "Pods, every namespace", rest: "get pods -A" },
  { id: "pods-wide", label: "Pods, with node and IP", rest: "get pods -o wide" },
  { id: "nodes", label: "Nodes", rest: "get nodes -o wide" },
  { id: "deploy", label: "Deployments", rest: "get deployments" },
  { id: "svc", label: "Services", rest: "get services" },
  { id: "ing", label: "Ingresses", rest: "get ingress" },
  { id: "events", label: "Events, newest last", rest: "get events --sort-by=.lastTimestamp" },
  { id: "top-pods", label: "Pods: CPU and memory", rest: "top pods" },
  { id: "top-nodes", label: "Nodes: CPU and memory", rest: "top nodes" },
  { id: "ns", label: "Namespaces", rest: "get namespaces" },
  { id: "storage", label: "Volumes", rest: "get pv,pvc" },
  { id: "info", label: "Cluster info", rest: "cluster-info" },
];

/** A word as the shell takes it: quoted only when it has to be (same rule as the engine). */
export function quote(s: string): string {
  return s !== "" && /^[A-Za-z0-9\-_./=:,@%+]+$/.test(s) ? s : `'${s.replace(/'/g, "'\\''")}'`;
}

export interface Pod {
  ns: string;
  name: string;
  ready: string;
  status: string;
  restarts: string;
  age: string;
}

/** Reads `kubectl get pods --no-headers` (with `-A`, the namespace is the first column). Lines that do not look like a pod are skipped. */
export function parsePods(text: string, withNamespace: boolean): Pod[] {
  const out: Pod[] = [];
  for (const line of text.split("\n")) {
    const t = line.trim().split(/\s+/);
    const at = withNamespace ? 1 : 0;
    if (t.length < at + 5 || !/^\d+\/\d+$/.test(t[at + 1])) continue;
    out.push({ ns: withNamespace ? t[0] : "", name: t[at], ready: t[at + 1], status: t[at + 2], restarts: t.slice(at + 3, -1).join(" "), age: t[t.length - 1] });
  }
  return out;
}

export type Health = "ok" | "wait" | "bad";

/** How a pod looks: running and all ready is fine, starting or finishing is waiting, anything else is wrong. */
export function health(p: Pod): Health {
  const [a, b] = p.ready.split("/").map(Number);
  if (p.status === "Completed" || p.status === "Succeeded") return "ok";
  if (p.status === "Running") return a === b ? "ok" : "wait";
  if (/^(Pending|ContainerCreating|PodInitializing|Init:\d+\/\d+|Terminating)$/.test(p.status)) return "wait";
  return "bad";
}

/** The names of the namespaces from `kubectl get namespaces --no-headers -o custom-columns=NAME:.metadata.name`. */
export function parseNames(text: string): string[] {
  return text.split("\n").map((l) => l.trim().split(/\s+/)[0]).filter((n) => n && n !== "NAME");
}

/** The words that need the person's yes before they run from a button: anything that changes the cluster. */
export function changes(rest: string): boolean {
  const words = rest.trim().split(/\s+/);
  let verb: string | undefined;
  for (let i = 0; i < words.length; i++) {
    if (["-n", "--namespace", "--context", "--cluster", "--user", "--kubeconfig", "-s", "--server"].includes(words[i])) i++;
    else if (!words[i].startsWith("-")) {
      verb = words[i];
      break;
    }
  }
  return !["get", "describe", "logs", "top", "cluster-info", "version", "explain", "api-resources", "exec"].includes(verb ?? "");
}
