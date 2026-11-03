//! `kestrel pdf-text`: a real PDF made here, a broken one, and no file.

use std::process::Command;

/// One page of Helvetica saying `text`, with a correct cross-reference table.
fn pdf(text: &str) -> Vec<u8> {
    let content = format!("BT /F1 12 Tf 72 720 Td ({text}) Tj ET");
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
         /Resources << /Font << /F1 5 0 R >> >> >>"
            .to_string(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut at = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        at.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    let n = objects.len() + 1;
    out.extend_from_slice(format!("xref\n0 {n}\n0000000000 65535 f \n").as_bytes());
    for a in at {
        out.extend_from_slice(format!("{a:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {n} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    out
}

fn kestrel(file: &std::path::Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_kestrel"))
        .arg("pdf-text")
        .arg(file)
        .output()
        .unwrap()
}

#[test]
fn the_text_comes_out_of_a_pdf_and_a_broken_one_only_fails() {
    let d = std::env::temp_dir().join(format!("kestrel_pdf_text_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let good = d.join("passport.pdf");
    std::fs::write(&good, pdf("Passport Z1234567 Date of Expiry 11/03/2029")).unwrap();
    let out = kestrel(&good);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(text.trim(), "Passport Z1234567 Date of Expiry 11/03/2029");
    let bad = d.join("broken.pdf");
    std::fs::write(&bad, "%PDF-1.7\n1 0 obj").unwrap();
    assert!(!kestrel(&bad).status.success());
    assert!(!kestrel(&d.join("nowhere.pdf")).status.success());
    let _ = std::fs::remove_dir_all(&d);
}
