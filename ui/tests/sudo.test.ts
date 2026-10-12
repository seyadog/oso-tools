import { describe, expect, it } from "vitest";
import { hasSudo, sudoSpans, writeCommand } from "../src/sudo";

describe("sudo in a command", () => {
  it("is found where it runs, also inside another command", () => {
    for (const c of ["sudo ls", "bash -c 'sudo ls'", 'ssh host "sudo systemctl restart nginx"', "echo $(sudo id)", "ls; sudo reboot", "/usr/bin/sudo id", "xargs sudo rm", "ls | sudo tee /x", "echo `sudo id`", "  sudo   id", "df -h\nsudo du -x /"]) {
      expect(hasSudo(c), c).toBe(true);
    }
  });
  it("is not found inside other words", () => {
    for (const c of ["pseudo ls", "cat /etc/sudoers", "cat sudo.txt", "sudo-rs ls", "df -h", "", "visudo"]) {
      expect(hasSudo(c), c).toBe(false);
    }
  });
  it("cuts the command into pieces that join back into it, with each sudo apart", () => {
    const c = "ssh h 'sudo a' && sudo b";
    const spans = sudoSpans(c);
    expect(spans.map((s) => s.text).join("")).toBe(c);
    expect(spans.filter((s) => s.sudo).map((s) => s.text)).toEqual(["sudo", "sudo"]);
    expect(sudoSpans("df -h")).toEqual([{ text: "df -h", sudo: false }]);
  });
  it("writes text, never HTML, and marks the sudo words", () => {
    const pre = document.createElement("pre");
    writeCommand(pre, "sudo echo '<img src=x onerror=alert(1)>'");
    expect(pre.querySelectorAll("mark.sudo").length).toBe(1);
    expect(pre.querySelector("img")).toBeNull();
    expect(pre.textContent).toBe("sudo echo '<img src=x onerror=alert(1)>'");
    expect(pre.classList.contains("has-sudo")).toBe(true);
    writeCommand(pre, "ls");
    expect(pre.querySelectorAll("mark").length).toBe(0);
    expect(pre.classList.contains("has-sudo")).toBe(false);
  });
});
