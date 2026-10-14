import { describe, expect, it } from "vitest";
import { QUICK, changes, health, parseNames, parsePods, quote } from "../src/k8s";

describe("kubernetes helpers", () => {
  it("reads the pods of one namespace", () => {
    const pods = parsePods("web-1   1/1   Running   0   3d\nweb-2   0/1   CrashLoopBackOff   5 (2m ago)   3d\n", false);
    expect(pods.map((p) => [p.name, p.ready, p.status, p.restarts, p.age])).toEqual([["web-1", "1/1", "Running", "0", "3d"], ["web-2", "0/1", "CrashLoopBackOff", "5 (2m ago)", "3d"]]);
  });
  it("reads the pods of every namespace, with the namespace first", () => {
    const pods = parsePods("kube-system coredns-x 1/1 Running 0 12d\nweb web-1 1/1 Running 1 1h", true);
    expect(pods.map((p) => [p.ns, p.name])).toEqual([["kube-system", "coredns-x"], ["web", "web-1"]]);
  });
  it("skips what is not a pod (errors, empty lines, headers)", () => {
    expect(parsePods("", false)).toEqual([]);
    expect(parsePods("No resources found in web namespace.\n", false)).toEqual([]);
    expect(parsePods("NAME READY STATUS RESTARTS AGE", false)).toEqual([]);
  });
  it("says how a pod looks", () => {
    const p = (ready: string, status: string) => ({ ns: "", name: "x", ready, status, restarts: "0", age: "1d" });
    expect(health(p("1/1", "Running"))).toBe("ok");
    expect(health(p("1/1", "Completed"))).toBe("ok");
    expect(health(p("0/1", "Running"))).toBe("wait");
    expect(health(p("0/1", "ContainerCreating"))).toBe("wait");
    expect(health(p("0/1", "Init:0/1"))).toBe("wait");
    expect(health(p("0/1", "CrashLoopBackOff"))).toBe("bad");
    expect(health(p("0/1", "ImagePullBackOff"))).toBe("bad");
    expect(health(p("0/1", "Error"))).toBe("bad");
  });
  it("reads the names of the namespaces", () => {
    expect(parseNames("NAME\ndefault\nkube-system\n\nweb\n")).toEqual(["default", "kube-system", "web"]);
  });
  it("quotes a word only when it has to", () => {
    expect(quote("web-1")).toBe("web-1");
    expect(quote("a b")).toBe("'a b'");
    expect(quote("it's")).toBe("'it'\\''s'");
    expect(quote("$(reboot)")).toBe("'$(reboot)'");
    expect(quote("")).toBe("''");
  });
  it("knows which buttons change the cluster", () => {
    for (const r of ["get pods", "logs -f web-1", "describe pod x", "top nodes", "-n web get pods", "exec -it web -- sh", "cluster-info"]) expect(changes(r), r).toBe(false);
    for (const r of ["delete pod x", "apply -f x.yaml", "scale deploy web --replicas=0", "rollout restart deploy web", "-n web delete pod x"]) expect(changes(r), r).toBe(true);
  });
  it("offers only commands that read", () => {
    expect(new Set(QUICK.map((q) => q.id)).size).toBe(QUICK.length);
    for (const q of QUICK) expect(changes(q.rest), q.id).toBe(false);
  });
});
