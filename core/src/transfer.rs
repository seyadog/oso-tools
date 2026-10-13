//! The plain parts of `oso_put` / `oso_get`: which local paths an agent may name, what a folder holds, a file's hash,
//! which remote paths are accepted. Nothing here touches the network or asks anybody.
//!
//! The file's content never goes through the agent: oso runs `scp` from disk to disk. These checks decide what is
//! even worth putting in front of the person.

use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

/// More than this in one transfer is not something an agent should do without the person doing it themselves.
pub const MAX_BYTES: u64 = 1 << 30;
pub const MAX_FILES: usize = 2000;

/// What a local path holds.
#[derive(Debug, PartialEq)]
pub struct Survey {
    pub is_dir: bool,
    /// `(name relative to the path, size)`, sorted. A single file is listed by its own name.
    pub files: Vec<(String, u64)>,
    pub total: u64,
}

/// Where the agent may read from and write to: the folder it was started in and its scratch folder.
/// `cwd` is the working directory of the `oso mcp` process (oso puts it in, the agent does not say it).
pub fn allowed_roots(cwd: Option<&str>) -> Vec<PathBuf> {
    let mut roots = vec![];
    if let Some(c) = cwd.filter(|c| !c.is_empty()) {
        roots.extend(Path::new(c).canonicalize());
    }
    // SAFETY: getuid has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    roots.extend(std::env::temp_dir().join(format!("claude-{uid}")).canonicalize());
    roots
}

/// Folders that hold keys and oso's own data: never a source or a destination, whatever the roots say.
fn forbidden() -> Vec<PathBuf> {
    let mut v = vec![];
    if let Some(home) = directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf()) {
        for d in [".ssh", ".gnupg", ".aws", ".kube", ".config/oso", ".local/share/oso"] {
            v.push(home.join(d));
        }
    }
    if let Some(d) = std::env::var_os("OSO_CONFIG_DIR") {
        v.push(PathBuf::from(d));
    }
    v.into_iter().map(|p| p.canonicalize().unwrap_or(p)).collect()
}

/// The real path of what the agent named (relative names are taken from `cwd`), if it is inside `roots` and not in a
/// forbidden folder. A path that does not exist yet (a download's destination) is judged by its folder, which must exist.
pub fn check_local(path: &str, cwd: Option<&str>, roots: &[PathBuf]) -> Result<PathBuf, String> {
    check_local_with(path, cwd, roots, &forbidden())
}

fn check_local_with(path: &str, cwd: Option<&str>, roots: &[PathBuf], forbidden: &[PathBuf]) -> Result<PathBuf, String> {
    if path.trim().is_empty() || path.contains('\0') {
        return Err("The local path is empty or has a NUL in it.".into());
    }
    let p = Path::new(path);
    let full = if p.is_absolute() { p.to_path_buf() } else { Path::new(cwd.filter(|c| !c.is_empty()).unwrap_or("/")).join(p) };
    let real = match full.canonicalize() {
        Ok(r) => r,
        Err(_) => {
            let name = full.file_name().ok_or("That local path has no file name.")?;
            if name == ".." || name == "." {
                return Err("That local path is not valid.".into());
            }
            let parent = full.parent().ok_or("That local path has no folder.")?;
            let parent = parent.canonicalize().map_err(|_| format!("The folder {} does not exist here.", parent.display()))?;
            parent.join(name)
        }
    };
    if forbidden.iter().any(|f| real.starts_with(f)) {
        return Err(format!("{} is where keys or oso's own data live: oso does not move files from or into it for an agent.", real.display()));
    }
    if !roots.iter().any(|r| real.starts_with(r)) {
        let names: Vec<String> = roots.iter().map(|r| r.display().to_string()).collect();
        return Err(format!("{} is outside the places an agent may transfer from or to ({}). Put the file there first, or ask the person to copy it themselves.", real.display(), if names.is_empty() { "none".into() } else { names.join(", ") }));
    }
    Ok(real)
}

/// What is at `path`: one file, or every file of a folder. Links and anything that is not a plain file or folder are refused
/// (a link inside an allowed folder could point anywhere), and so is too much.
pub fn survey(path: &Path) -> Result<Survey, String> {
    let meta = std::fs::symlink_metadata(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    if meta.is_file() {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        return Ok(Survey { is_dir: false, files: vec![(name, meta.len())], total: meta.len() });
    }
    if !meta.is_dir() {
        return Err(format!("{} is not a plain file or folder.", path.display()));
    }
    let mut files = vec![];
    walk(path, path, &mut files)?;
    files.sort();
    let total = files.iter().map(|f| f.1).sum();
    Ok(Survey { is_dir: true, files, total })
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, u64)>) -> Result<(), String> {
    for entry in std::fs::read_dir(dir).map_err(|e| format!("Cannot read {}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let meta = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if meta.is_dir() {
            walk(root, &path, out)?;
        } else if meta.is_file() {
            out.push((path.strip_prefix(root).unwrap_or(&path).to_string_lossy().into_owned(), meta.len()));
        } else {
            return Err(format!("{} is a link or a special file: oso does not send folders that hold those.", path.display()));
        }
        if out.len() > MAX_FILES {
            return Err(format!("More than {MAX_FILES} files: pack them into one (tar) first."));
        }
    }
    Ok(())
}

/// The refusal for a survey that is too big, if it is.
pub fn too_big(s: &Survey) -> Option<String> {
    if s.files.len() > MAX_FILES {
        return Some(format!("More than {MAX_FILES} files: pack them into one (tar) first."));
    }
    (s.total > MAX_BYTES).then(|| format!("{} is more than the {} an agent may move in one go: ask the person to copy it themselves.", human(s.total), human(MAX_BYTES)))
}

pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// A sha256 as the agent may write it (hex, any case), lowercased; `Err` if it is not one.
pub fn clean_sha256(s: &str) -> Result<String, String> {
    let s = s.trim().to_lowercase();
    if s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit()) { Ok(s) } else { Err("`sha256` must be 64 hexadecimal characters.".into()) }
}

pub fn human(n: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 { format!("{n} B") } else { format!("{v:.1} {}", UNITS[u]) }
}

/// A path on the server that is safe to hand to `scp` and to a remote shell. Anything a shell would expand is refused: an
/// older `scp` still gives the remote path to a shell.
pub fn remote_path_ok(p: &str) -> Result<(), String> {
    if p.trim().is_empty() {
        return Err("The remote path is empty.".into());
    }
    if p.len() > 1024 {
        return Err("The remote path is too long.".into());
    }
    if let Some(c) = p.chars().find(|c| c.is_control() || "`$;|&<>()*?[]{}\\'\"!#~".contains(*c) && !(*c == '~' && p.starts_with("~/"))) {
        return Err(format!("The remote path has {c:?} in it, which a shell would expand: use a plain path (letters, digits, spaces, `/ . _ - + @ : , =`)."));
    }
    Ok(())
}

/// A remote path as one word for a remote shell (`~/x` still means the home folder).
pub fn remote_arg(p: &str) -> String {
    let quote = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
    match p.strip_prefix("~/") {
        Some(rest) => format!("\"$HOME\"/{}", quote(rest)),
        None if p == "~" => "\"$HOME\"".into(),
        None => quote(p),
    }
}

/// The server's answer to the listing command of a download: a first line `FILE`, `DIR` or `MISSING`, then `size<TAB>path`.
pub fn parse_listing(out: &str) -> Result<(bool, Vec<(String, u64)>), String> {
    // a login that went through a terminal may print something first (a banner): the answer starts at its marker
    let mut lines = out.lines().skip_while(|l| !matches!(l.trim(), "DIR" | "FILE" | "MISSING"));
    let is_dir = match lines.next().map(str::trim) {
        Some("DIR") => true,
        Some("FILE") => false,
        Some("MISSING") => return Err("That path does not exist on the server.".into()),
        _ => return Err("The server did not answer the listing in a way oso understands.".into()),
    };
    let mut files = vec![];
    for l in lines {
        let Some((size, path)) = l.split_once('\t') else { continue };
        if let Ok(n) = size.trim().parse::<u64>() {
            files.push((path.to_string(), n));
        }
    }
    Ok((is_dir, files))
}

/// The lines the person reads: at most `show` files, then how many more.
pub fn file_lines(files: &[(String, u64)], show: usize) -> String {
    let mut s = String::new();
    for (name, size) in files.iter().take(show) {
        s.push_str(&format!("  {name}  ({})\n", human(*size)));
    }
    if files.len() > show {
        s.push_str(&format!("  … and {} more\n", files.len() - show));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("oso-transfer-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.canonicalize().unwrap()
    }

    #[test]
    fn only_the_working_folder_and_the_scratch_folder_are_allowed() {
        let d = tmp("roots");
        std::fs::write(d.join("a.txt"), "hi").unwrap();
        let cwd = d.to_str().unwrap();
        let roots = allowed_roots(Some(cwd));
        assert_eq!(check_local("a.txt", Some(cwd), &roots).unwrap(), d.join("a.txt"));
        assert_eq!(check_local(d.join("a.txt").to_str().unwrap(), None, &roots).unwrap(), d.join("a.txt"));
        // a destination that does not exist yet is judged by its folder
        assert_eq!(check_local("new.bin", Some(cwd), &roots).unwrap(), d.join("new.bin"));
        assert!(check_local("/etc/passwd", Some(cwd), &roots).unwrap_err().contains("outside"));
        assert!(check_local("../x", Some(cwd), &roots).unwrap_err().contains("outside"));
        assert!(check_local("nofolder/x", Some(cwd), &roots).unwrap_err().contains("does not exist"));
        assert!(check_local("", Some(cwd), &roots).is_err());
    }

    #[test]
    fn a_link_cannot_lead_out_of_the_allowed_folder() {
        let d = tmp("link");
        let outside = tmp("link-outside");
        std::fs::write(outside.join("secret"), "x").unwrap();
        std::os::unix::fs::symlink(outside.join("secret"), d.join("innocent")).unwrap();
        let cwd = d.to_str().unwrap();
        let roots = allowed_roots(Some(cwd));
        assert!(check_local("innocent", Some(cwd), &roots).unwrap_err().contains("outside"));
        // and a folder that holds a link is not sent
        std::fs::create_dir(d.join("dir")).unwrap();
        std::fs::write(d.join("dir/ok"), "1").unwrap();
        assert!(survey(&d.join("dir")).is_ok());
        std::os::unix::fs::symlink(outside.join("secret"), d.join("dir/l")).unwrap();
        assert!(survey(&d.join("dir")).unwrap_err().contains("link"));
    }

    #[test]
    fn keys_and_oso_data_are_never_moved_even_inside_the_working_folder() {
        let d = tmp("forbidden");
        std::fs::create_dir_all(d.join(".ssh")).unwrap();
        let cwd = d.to_str().unwrap();
        let roots = allowed_roots(Some(cwd));
        let no = vec![d.join(".ssh")];
        assert!(check_local_with(".ssh/id_ed25519", Some(cwd), &roots, &no).unwrap_err().contains("keys"));
        assert!(check_local_with("fine.txt", Some(cwd), &roots, &no).is_ok());
        // the real list names the usual places
        let real = forbidden();
        let home = directories::BaseDirs::new().unwrap().home_dir().to_path_buf();
        assert!(real.iter().any(|f| f.ends_with(".ssh")) && home.exists());
    }

    #[test]
    fn a_folder_is_listed_and_a_single_file_is_its_own_list() {
        let d = tmp("survey");
        std::fs::create_dir_all(d.join("src/deep")).unwrap();
        std::fs::write(d.join("src/b.txt"), "12345").unwrap();
        std::fs::write(d.join("src/deep/a.txt"), "12").unwrap();
        let s = survey(&d.join("src")).unwrap();
        assert_eq!(s, Survey { is_dir: true, files: vec![("b.txt".into(), 5), ("deep/a.txt".into(), 2)], total: 7 });
        let one = survey(&d.join("src/b.txt")).unwrap();
        assert_eq!((one.is_dir, one.files, one.total), (false, vec![("b.txt".into(), 5)], 5));
        assert!(survey(&d.join("nope")).is_err());
    }

    #[test]
    fn too_much_is_refused_in_words() {
        let big = Survey { is_dir: false, files: vec![("x".into(), MAX_BYTES + 1)], total: MAX_BYTES + 1 };
        assert!(too_big(&big).unwrap().contains("1.0 GB"));
        let many = Survey { is_dir: true, files: (0..=MAX_FILES).map(|i| (i.to_string(), 1)).collect(), total: 1 };
        assert!(too_big(&many).unwrap().contains("tar"));
        assert!(too_big(&Survey { is_dir: false, files: vec![], total: 10 }).is_none());
    }

    #[test]
    fn the_hash_is_sha256_of_the_content() {
        let d = tmp("hash");
        std::fs::write(d.join("f"), "abc").unwrap();
        assert_eq!(sha256_file(&d.join("f")).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert!(clean_sha256(" BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD ").is_ok());
        assert!(clean_sha256("abc").is_err());
        assert!(clean_sha256(&"g".repeat(64)).is_err());
    }

    #[test]
    fn remote_paths_that_a_shell_would_expand_are_refused() {
        for ok in ["/srv/app/a.txt", "~/backups/db 1.sql", "rel/file-1_2.tar.gz", "/a/b:c"] {
            assert!(remote_path_ok(ok).is_ok(), "{ok}");
        }
        for bad in ["", "/a/$(reboot)", "/a/`id`", "/a;b", "/a|b", "/a\nb", "/a*", "/a'b", "/a\"b", "/a\\b", "~root/x", "/a b>c"] {
            assert!(remote_path_ok(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_remote_path_becomes_one_shell_word() {
        assert_eq!(remote_arg("/srv/a b"), "'/srv/a b'");
        assert_eq!(remote_arg("~/x y"), "\"$HOME\"/'x y'");
        assert_eq!(remote_arg("~"), "\"$HOME\"");
        assert_eq!(remote_arg("it's"), "'it'\\''s'");
    }

    #[test]
    fn the_server_listing_is_read_back() {
        let (dir, files) = parse_listing("DIR\n10\t/srv/a/x\n2048\t/srv/a/y z\n").unwrap();
        assert!(dir);
        assert_eq!(files, vec![("/srv/a/x".to_string(), 10), ("/srv/a/y z".to_string(), 2048)]);
        assert!(!parse_listing("FILE\n5\t/f\n").unwrap().0);
        assert!(parse_listing("MISSING\n").unwrap_err().contains("does not exist"));
        assert!(parse_listing("garbage").is_err());
        assert!(parse_listing("Welcome!\nFILE\n5\t/f\n").is_ok(), "a banner before the answer is skipped");
    }

    #[test]
    fn sizes_and_lists_read_for_a_person() {
        assert_eq!((human(0).as_str(), human(1500).as_str(), human(5 << 20).as_str()), ("0 B", "1.5 KB", "5.0 MB"));
        let files: Vec<(String, u64)> = (0..5).map(|i| (format!("f{i}"), 1)).collect();
        assert_eq!(file_lines(&files, 3), "  f0  (1 B)\n  f1  (1 B)\n  f2  (1 B)\n  … and 2 more\n");
    }
}
