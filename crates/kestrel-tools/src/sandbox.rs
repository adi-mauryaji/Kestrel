//! Filesystem sandbox (Ch. 15.2, A.13): a path from a tool call is canonicalized FIRST (which
//! resolves `..` and symlinks) and only THEN checked against the approved roots, so traversal,
//! absolute escapes and symlink escapes all fail. Component-wise containment (`Path::starts_with`),
//! so `/notes2` is not inside `/notes`.
//!
//! Change from A.13: a relative path is tried under every root in order, not only the first.
//! What this does not show: that the process cannot reach other files (an OS-level sandbox such
//! as Landlock is defence in depth, Ch. 15 [FUT]).

use std::path::{Path, PathBuf};

use kestrel_core::KestrelError;

pub struct Sandbox {
    /// Canonical, existing directories.
    roots: Vec<PathBuf>,
}

impl Sandbox {
    pub fn new(roots: &[PathBuf]) -> Result<Self, KestrelError> {
        if roots.is_empty() {
            return Err(KestrelError::Sandbox("no approved directory".into()));
        }
        let roots = roots
            .iter()
            .map(|r| match r.canonicalize() {
                Ok(c) if c.is_dir() => Ok(c),
                _ => Err(KestrelError::Sandbox(format!(
                    "approved root {} is not an existing directory",
                    r.display()
                ))),
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { roots })
    }

    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// The canonical path of an existing file or directory inside a root.
    pub fn resolve(&self, user_path: &str) -> Result<PathBuf, KestrelError> {
        if user_path.is_empty() || user_path.contains('\0') {
            return Err(KestrelError::Sandbox("empty path or NUL byte".into()));
        }
        let p = Path::new(user_path);
        let candidates: Vec<PathBuf> = if p.is_absolute() {
            vec![p.to_path_buf()]
        } else {
            self.roots.iter().map(|r| r.join(p)).collect()
        };
        let mut escaped = false;
        for c in candidates {
            let Ok(real) = c.canonicalize() else { continue };
            if self.roots.iter().any(|r| real.starts_with(r)) {
                return Ok(real);
            }
            escaped = true;
        }
        Err(KestrelError::Sandbox(
            if escaped {
                "path escapes the approved directories"
            } else {
                "path not found"
            }
            .into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A fresh directory per test (tests run in parallel in one process).
    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kestrel_sandbox_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn allows_inside_and_blocks_traversal_and_absolute_escapes() {
        let d = tmpdir("a");
        fs::create_dir(d.join("meetings")).unwrap();
        fs::write(d.join("meetings/standup.md"), "hi").unwrap();
        let sb = Sandbox::new(std::slice::from_ref(&d)).unwrap();
        let inside = sb.resolve("meetings/standup.md").unwrap();
        assert_eq!(
            inside,
            d.canonicalize().unwrap().join("meetings/standup.md")
        );
        assert_eq!(sb.resolve(inside.to_str().unwrap()).unwrap(), inside); // absolute, inside
        assert!(sb.resolve("meetings/../meetings/standup.md").is_ok());
        for bad in ["../../../etc/passwd", "/etc/passwd", "meetings/../../x"] {
            assert!(
                matches!(sb.resolve(bad), Err(KestrelError::Sandbox(_))),
                "{bad}"
            );
        }
    }

    #[test]
    fn windows_style_escapes_are_blocked_too() {
        // On Windows these are a traversal, a drive path and a verbatim path out of the folder;
        // elsewhere they are odd file names that do not exist. Either way: refused (v4 §4.1).
        let d = tmpdir("w");
        fs::write(d.join("note.md"), "n").unwrap();
        let sb = Sandbox::new(std::slice::from_ref(&d)).unwrap();
        for bad in [
            r"..\..\Windows\win.ini",
            r"C:\Windows\win.ini",
            r"\\?\C:\Windows\win.ini",
            r"C:..\note.md",
        ] {
            assert!(
                matches!(sb.resolve(bad), Err(KestrelError::Sandbox(_))),
                "{bad}"
            );
        }
        assert!(sb.resolve("note.md").is_ok());
    }

    #[test]
    fn a_sibling_with_a_common_prefix_is_outside() {
        let d = tmpdir("b");
        let sibling = PathBuf::from(format!("{}2", d.display()));
        fs::create_dir_all(&sibling).unwrap();
        fs::write(sibling.join("secret.txt"), "s").unwrap();
        let sb = Sandbox::new(std::slice::from_ref(&d)).unwrap();
        assert!(
            sb.resolve(sibling.join("secret.txt").to_str().unwrap())
                .is_err()
        );
        fs::remove_dir_all(sibling).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_followed_before_the_check() {
        let d = tmpdir("c");
        let outside = tmpdir("c_outside");
        fs::write(outside.join("secret.txt"), "s").unwrap();
        fs::write(d.join("real.md"), "r").unwrap();
        std::os::unix::fs::symlink(&outside, d.join("link")).unwrap();
        std::os::unix::fs::symlink(d.join("real.md"), d.join("alias.md")).unwrap();
        let sb = Sandbox::new(std::slice::from_ref(&d)).unwrap();
        assert!(sb.resolve("link/secret.txt").is_err(), "symlink escape");
        assert_eq!(
            sb.resolve("alias.md").unwrap(),
            d.canonicalize().unwrap().join("real.md")
        );
    }

    #[test]
    fn a_relative_path_is_tried_under_every_root() {
        let (a, b) = (tmpdir("d1"), tmpdir("d2"));
        fs::write(b.join("only_in_b.md"), "b").unwrap();
        let sb = Sandbox::new(&[a, b.clone()]).unwrap();
        assert_eq!(
            sb.resolve("only_in_b.md").unwrap(),
            b.canonicalize().unwrap().join("only_in_b.md")
        );
    }

    #[test]
    fn empty_nul_missing_and_bad_roots_are_errors() {
        let d = tmpdir("e");
        let sb = Sandbox::new(std::slice::from_ref(&d)).unwrap();
        for bad in ["", "a\0b", "missing.md"] {
            assert!(
                matches!(sb.resolve(bad), Err(KestrelError::Sandbox(_))),
                "{bad:?}"
            );
        }
        assert!(Sandbox::new(&[]).is_err());
        assert!(Sandbox::new(&[d.join("no_such_dir")]).is_err());
        fs::write(d.join("file.txt"), "f").unwrap();
        assert!(
            Sandbox::new(&[d.join("file.txt")]).is_err(),
            "a root must be a directory"
        );
    }
}
