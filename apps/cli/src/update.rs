//! Whether this program was replaced while it ran. Installing over a running Kestrel changes nothing
//! until it restarts, and nothing said so: on 2026-11-04 the old service answered for two hours after an
//! install, with a new worker beside it. The page says so now.

use std::path::{Path, PathBuf};

use crate::capabilities::file_id;

/// The file this program was started from, as it was then.
pub struct ThisProgram {
    path: PathBuf,
    started_as: Option<(u64, u64)>,
}

impl ThisProgram {
    /// As it is now, at start. Not known (no such path): it never counts as replaced.
    pub fn at_start() -> Self {
        let path = std::env::current_exe().unwrap_or_default();
        Self::at(&path)
    }

    fn at(path: &Path) -> Self {
        Self {
            started_as: file_id(path),
            path: path.to_path_buf(),
        }
    }

    /// Whether the file is no longer the one that started: changed, or gone.
    pub fn replaced(&self) -> bool {
        self.started_as.is_some() && file_id(&self.path) != self.started_as
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("kestrel_update_{name}_{}", std::process::id()));
        std::fs::write(&p, "the old program").unwrap();
        p
    }

    #[test]
    fn the_same_file_is_not_replaced() {
        let p = file("same");
        let program = ThisProgram::at(&p);
        assert!(!program.replaced());
        std::fs::remove_file(&p).unwrap();
    }

    #[test]
    fn a_new_file_there_or_none_is_replaced() {
        let p = file("new");
        let program = ThisProgram::at(&p);
        std::fs::write(&p, "the new program, a different size").unwrap();
        assert!(program.replaced());
        std::fs::remove_file(&p).unwrap();
        assert!(program.replaced(), "gone counts too");
    }

    #[test]
    fn a_program_whose_file_could_not_be_read_never_says_so() {
        let program = ThisProgram::at(Path::new("/no/such/program"));
        assert!(!program.replaced());
    }
}
