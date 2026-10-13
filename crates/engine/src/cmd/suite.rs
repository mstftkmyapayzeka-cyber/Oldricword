//! The Craft suite: hand the document to PdfCraft as a PDF, and keep that PDF in step with it.
//!
//! `file.openInPdfCraft` writes the document as a PDF and asks the front end to open it in
//! PdfCraft (a `{"launch": "pdfcraft", "path": …}` UI request; headless sessions ignore it, so
//! agents get the PDF and nothing starts). The PDF stays *linked*: every save of the document
//! writes it again (`file.save`), the desktop app also refreshes it after a pause in editing, and
//! PdfCraft reloads a file that changes on disk, so the two windows stay in sync.
//!
//! The PDF is never written over a file the link didn't make: the first export picks a free name
//! (`Report.pdf`, `Report (2).pdf`, …) beside the document.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::{CmdError, CmdResult, CommandSpec, Session, p};

/// The sibling app the PDF opens in.
pub const PDF_APP: &str = "pdfcraft";

/// How many numbered names (`Name (2).pdf` …) are tried before giving up.
const MAX_NAME_TRIES: u32 = 99;

/// Longest file stem made from the document's title, in characters.
const MAX_STEM_CHARS: usize = 80;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("file.openInPdfCraft", "Open in PdfCraft", "File › Export", open_in_pdfcraft)
            .params(
                r#"{"path"?: string (where the PDF goes; default: a free name beside the document, or in the temporary folder for an unsaved one), "sync"?: bool (keep the PDF updated as the document changes; default true), "markup"?: bool (as file.exportPdf)}"#,
            )
            .pure(),
        CommandSpec::new("file.pdfSync", "Keep PDF in Sync", "File › Export", pdf_sync)
            .params(r#"{"value"?: bool (omit to toggle), "path"?: string (the linked PDF when turning it on)} — the linked PDF is written again on every save"#)
            .pure(),
        CommandSpec::new("file.updatePdf", "Update Linked PDF", "File › Export", update_pdf).params(r#"{"markup"?: bool (as file.exportPdf)}"#).pure(),
    ]
}

/// Export the PDF, link it (unless `sync` is false) and ask the front end to open it in PdfCraft.
fn open_in_pdfcraft(s: &mut Session, v: &Value) -> CmdResult {
    let path = match p::str(v, "path") {
        Some(given) => with_pdf_ext(given),
        None => match &s.pdf_link {
            Some(linked) => linked.clone(),
            None => fresh_pdf_path(s)?,
        },
    };
    write_pdf(s, &path, v)?;
    let sync = p::bool(v, "sync").unwrap_or(true);
    s.pdf_link = sync.then(|| path.clone());
    let shown = path.to_string_lossy().into_owned();
    s.ui_requests.push(json!({"launch": PDF_APP, "path": shown}));
    Ok(json!({"path": shown, "sync": sync}))
}

/// Turn the PDF link on (exporting the PDF) or off.
fn pdf_sync(s: &mut Session, v: &Value) -> CmdResult {
    let on = p::bool(v, "value").unwrap_or(s.pdf_link.is_none());
    if !on {
        s.pdf_link = None;
        return Ok(json!({"sync": false}));
    }
    let path = match (p::str(v, "path"), &s.pdf_link) {
        (Some(given), _) => with_pdf_ext(given),
        (None, Some(linked)) => linked.clone(),
        (None, None) => fresh_pdf_path(s)?,
    };
    write_pdf(s, &path, v)?;
    s.pdf_link = Some(path.clone());
    Ok(json!({"sync": true, "path": path.to_string_lossy()}))
}

/// Write the linked PDF again now.
fn update_pdf(s: &mut Session, v: &Value) -> CmdResult {
    let Some(path) = s.pdf_link.clone() else {
        return Err(CmdError::Failed("no linked PDF: use Open in PdfCraft or Keep PDF in Sync first".into()));
    };
    write_pdf(s, &path, v)?;
    Ok(json!({"path": path.to_string_lossy()}))
}

/// After a save: write the linked PDF too. A failure doesn't fail the save (the document is
/// safe); it is reported in the status bar and the result.
pub(crate) fn after_save(s: &mut Session) -> Option<Value> {
    let path = s.pdf_link.clone()?;
    Some(match write_pdf(s, &path, &json!({})) {
        Ok(()) => json!({"path": path.to_string_lossy()}),
        Err(e) => {
            s.status = format!("The linked PDF wasn't updated: {e}");
            json!({"path": path.to_string_lossy(), "error": e.to_string()})
        }
    })
}

fn write_pdf(s: &Session, path: &Path, v: &Value) -> Result<(), CmdError> {
    let opts = super::file::export_options(s, v);
    crate::io::save_path_with(path, &s.doc, None, &opts).map_err(CmdError::Failed)
}

fn with_pdf_ext(path: &str) -> PathBuf {
    if path.to_ascii_lowercase().ends_with(".pdf") { PathBuf::from(path) } else { PathBuf::from(format!("{path}.pdf")) }
}

/// A PDF path no file has yet: beside the saved document, else in the temporary folder.
fn fresh_pdf_path(s: &Session) -> Result<PathBuf, CmdError> {
    let (dir, stem) = match &s.path {
        Some(doc) => {
            let dir = doc.parent().map(Path::to_path_buf).unwrap_or_default();
            let stem = doc.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            (dir, stem)
        }
        None => (temp_folder()?, safe_stem(&s.doc.core.title)),
    };
    let stem = if stem.trim().is_empty() { "Document".to_string() } else { stem };
    free_name(&dir, &stem, |p| p.exists()).ok_or_else(|| CmdError::Failed(format!("no free PDF name for {stem} in {}", dir.display())))
}

/// `<dir>/<stem>.pdf`, else the first free `<dir>/<stem> (n).pdf`.
fn free_name(dir: &Path, stem: &str, exists: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let first = dir.join(format!("{stem}.pdf"));
    if !exists(&first) {
        return Some(first);
    }
    (2..=MAX_NAME_TRIES).map(|n| dir.join(format!("{stem} ({n}).pdf"))).find(|p| !exists(p))
}

/// A title made safe as a file name: path separators and characters Windows refuses become
/// spaces, control characters go, and it is cut to [`MAX_STEM_CHARS`].
fn safe_stem(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .filter(|c| !c.is_control())
        .map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { ' ' } else { c })
        .take(MAX_STEM_CHARS)
        .collect();
    // A name of dots ("..") would climb out of the folder.
    cleaned.trim().trim_matches('.').trim().to_string()
}

/// The system's temporary folder (it exists; files are written only through `crate::io`).
#[cfg(not(target_arch = "wasm32"))]
fn temp_folder() -> Result<PathBuf, CmdError> {
    Ok(std::env::temp_dir())
}

/// The browser has no file system to put the PDF in (`std::env::temp_dir` panics there).
#[cfg(target_arch = "wasm32")]
fn temp_folder() -> Result<PathBuf, CmdError> {
    Err(CmdError::Failed("save the document first: the browser has no folder to put the PDF in".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_name_skips_files_that_exist() {
        let dir = Path::new("/docs");
        assert_eq!(free_name(dir, "Report", |_| false), Some(dir.join("Report.pdf")));
        let taken = [dir.join("Report.pdf"), dir.join("Report (2).pdf")];
        assert_eq!(free_name(dir, "Report", |p| taken.iter().any(|t| t == p)), Some(dir.join("Report (3).pdf")));
        assert_eq!(free_name(dir, "Report", |_| true), None);
    }

    #[test]
    fn titles_become_safe_file_names() {
        assert_eq!(safe_stem("Q3: plan/draft?"), "Q3  plan draft");
        assert_eq!(safe_stem(".."), "");
        assert_eq!(safe_stem("a\u{0}b\nc"), "abc");
        assert_eq!(safe_stem(&"x".repeat(500)).chars().count(), MAX_STEM_CHARS);
    }

    #[test]
    fn open_in_pdfcraft_exports_links_and_asks_to_launch() {
        let dir = std::env::temp_dir().join(format!("wc-suite-open-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let doc = dir.join("Notes.docx");
        let mut s = Session::new(crate::sample::sample_document());
        s.run("file.save", &json!({"path": doc.to_string_lossy()})).unwrap();
        // A PDF already there is someone's: the link gets a name of its own.
        std::fs::write(dir.join("Notes.pdf"), b"theirs").unwrap();
        let r = s.run("file.openInPdfCraft", &json!({})).unwrap();
        let pdf = dir.join("Notes (2).pdf");
        assert_eq!(r["path"], pdf.to_string_lossy().as_ref());
        assert_eq!(std::fs::read(dir.join("Notes.pdf")).unwrap(), b"theirs");
        assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF"));
        assert_eq!(s.pdf_link.as_deref(), Some(pdf.as_path()));
        assert!(s.ui_requests.iter().any(|r| r["launch"] == PDF_APP && r["path"] == pdf.to_string_lossy().as_ref()));
        // Opening it again reuses the link instead of making another copy.
        let again = s.run("file.openInPdfCraft", &json!({})).unwrap();
        assert_eq!(again["path"], pdf.to_string_lossy().as_ref());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saving_rewrites_the_linked_pdf() {
        let dir = std::env::temp_dir().join(format!("wc-suite-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let doc = dir.join("Letter.docx");
        let pdf = dir.join("Letter.pdf");
        let mut s = Session::new(crate::sample::sample_document());
        s.run("file.save", &json!({"path": doc.to_string_lossy()})).unwrap();
        let r = s.run("file.pdfSync", &json!({"value": true})).unwrap();
        assert_eq!(r["path"], pdf.to_string_lossy().as_ref());
        std::fs::write(&pdf, b"stale").unwrap();
        let saved = s.run("file.save", &json!({})).unwrap();
        assert_eq!(saved["pdf"]["path"], pdf.to_string_lossy().as_ref());
        assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF"));
        // Off: saves leave the PDF alone.
        s.run("file.pdfSync", &json!({"value": false})).unwrap();
        std::fs::write(&pdf, b"stale").unwrap();
        let saved = s.run("file.save", &json!({})).unwrap();
        assert!(saved.get("pdf").is_none());
        assert_eq!(std::fs::read(&pdf).unwrap(), b"stale");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn update_without_a_link_is_an_error() {
        let mut s = Session::new(crate::sample::sample_document());
        assert!(s.run("file.updatePdf", &json!({})).is_err());
        assert!(s.pdf_link.is_none());
    }

    #[test]
    fn unsaved_documents_go_to_the_temporary_folder() {
        let mut s = Session::new(crate::sample::sample_document());
        let r = s.run("file.openInPdfCraft", &json!({"sync": false})).unwrap();
        let path = PathBuf::from(r["path"].as_str().unwrap());
        assert_eq!(path.parent(), Some(std::env::temp_dir().as_path()));
        assert!(s.pdf_link.is_none());
        let _ = std::fs::remove_file(path);
    }
}
