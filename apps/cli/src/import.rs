//! `kestrel model import FILE.gguf` (v4 A3, blueprint §4.3): a catalog model from a file (a USB
//! stick, a download made elsewhere) copied into Kestrel's data folder, where `slot::model_path`
//! finds it first, and kept only when its SHA-256 is the catalog's. The app's onboarding imports
//! the same way.

use std::error::Error;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use kestrel_capability::Catalog;
use sha2::{Digest, Sha256};

#[derive(Debug, PartialEq)]
pub struct Imported {
    pub id: String,
    pub path: PathBuf,
    pub bytes: u64,
}

/// Copies `src` into `data`/models under its catalog name, hashing as it copies; `progress` hears
/// (bytes done, bytes in all). A file whose SHA-256 is no catalog model's is refused and nothing of
/// it is left.
pub fn import(
    src: &Path,
    data: &Path,
    catalog: &Catalog,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<Imported, Box<dyn Error>> {
    let mut from =
        std::fs::File::open(src).map_err(|e| format!("cannot read {}: {e}", src.display()))?;
    let bytes = from.metadata()?.len();
    let dir = data.join("models");
    std::fs::create_dir_all(&dir)?;
    // Copied and hashed in one read; named once its hash says which model it is.
    let part = dir.join(".import.part");
    let copied = (|| -> std::io::Result<String> {
        let mut to = std::fs::File::create(&part)?;
        let (mut hash, mut buf, mut done) = (Sha256::new(), vec![0; 1 << 20], 0);
        loop {
            let n = from.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hash.update(&buf[..n]);
            to.write_all(&buf[..n])?;
            done += n as u64;
            progress(done, bytes);
        }
        to.sync_all()?;
        Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
    })();
    let known = copied.as_ref().ok().and_then(|sha| {
        (catalog.models.iter()).find(|m| m.sha256.as_deref() == Some(sha.as_str()))
    });
    let Some(m) = known else {
        let _ = std::fs::remove_file(&part);
        copied?;
        return Err(format!(
            "{} is not one of Kestrel's models: its SHA-256 is none in the catalog \
             (`kestrel model use FILE.gguf` writes with any GGUF file where it is)",
            src.display()
        )
        .into());
    };
    let path = dir.join(&m.file);
    std::fs::rename(&part, &path)?;
    Ok(Imported {
        id: m.id.clone(),
        path,
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SHA-256 of "abc" (FIPS 180-2, appendix B.1).
    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn catalog() -> Catalog {
        serde_json::from_value(serde_json::json!({"models": [{
            "id": "tiny-writer", "file": "tiny-writer.gguf", "file_mib": 1, "memory_mib": 1,
            "role": "writer", "estimate": false, "sha256": ABC, "source": "test"
        }]}))
        .unwrap()
    }

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kestrel_import_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn left_in(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .map(|r| {
                r.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn a_file_with_a_catalog_hash_is_copied_under_its_catalog_name() {
        let d = dir("ok");
        let src = d.join("downloaded copy.gguf");
        std::fs::write(&src, "abc").unwrap();
        let mut heard = Vec::new();
        let got = import(&src, &d.join("data"), &catalog(), &mut |done, all| {
            heard.push((done, all))
        })
        .unwrap();
        let path = d.join("data/models/tiny-writer.gguf");
        assert_eq!(
            got,
            Imported {
                id: "tiny-writer".into(),
                path: path.clone(),
                bytes: 3
            }
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "abc");
        assert!(src.is_file(), "the original stays");
        assert_eq!(heard.last(), Some(&(3, 3)));
        assert_eq!(left_in(&d.join("data/models")), ["tiny-writer.gguf"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn any_other_file_is_refused_and_nothing_of_it_is_left() {
        let d = dir("other");
        let src = d.join("tiny-writer.gguf");
        std::fs::write(&src, "abd").unwrap();
        let e = import(&src, &d.join("data"), &catalog(), &mut |_, _| {}).unwrap_err();
        assert!(e.to_string().contains("SHA-256"), "{e}");
        assert_eq!(left_in(&d.join("data/models")), Vec::<String>::new());
        let e = import(
            &d.join("nowhere.gguf"),
            &d.join("data"),
            &catalog(),
            &mut |_, _| {},
        );
        assert!(e.unwrap_err().to_string().contains("nowhere.gguf"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
