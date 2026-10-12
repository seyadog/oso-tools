// `sudo` in a command an agent wants to run is what a person most needs to see, also when it hides inside another
// command (`bash -c "sudo …"`, `ssh host 'sudo …'`, `$(sudo …)`). This marks the word wherever it stands as a word.
// It is a highlight, not a judgement: `grep sudo /etc/group` is marked too (it only names the word, but a person
// glancing at an approval should lose nothing by seeing it), while `pseudo`, `sudoers` and `sudo.txt` are not.

export interface Span {
  text: string;
  sudo: boolean;
}

/** `sudo` as a whole word, also after `/` (`/usr/bin/sudo`); not inside `pseudo`, `sudoers`, `sudo-rs` or `sudo.txt`. */
const SUDO = /(?<![\w.-])sudo(?![\w.-])/g;

/** The command cut into pieces, the `sudo` words being their own pieces. The pieces always join back into the command. */
export function sudoSpans(command: string): Span[] {
  const out: Span[] = [];
  let last = 0;
  for (const m of command.matchAll(SUDO)) {
    if (m.index > last) out.push({ text: command.slice(last, m.index), sudo: false });
    out.push({ text: m[0], sudo: true });
    last = m.index + m[0].length;
  }
  if (last < command.length) out.push({ text: command.slice(last), sudo: false });
  return out.length ? out : [{ text: command, sudo: false }];
}

export function hasSudo(command: string): boolean {
  return new RegExp(SUDO.source).test(command);
}

/** Writes the command into `pre`, with each `sudo` in an amber `<mark>`. Text only: nothing in a command is ever taken as HTML. */
export function writeCommand(pre: HTMLElement, command: string) {
  pre.textContent = "";
  for (const s of sudoSpans(command)) {
    if (s.sudo) {
      const m = document.createElement("mark");
      m.className = "sudo";
      m.textContent = s.text;
      pre.append(m);
    } else pre.append(document.createTextNode(s.text));
  }
  pre.classList.toggle("has-sudo", hasSudo(command));
}
