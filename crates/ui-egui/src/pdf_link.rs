//! The PDF linked to the document (File › Export › Open in PdfCraft, `wordcraft_engine::cmd::suite`):
//! opening it in PdfCraft, and writing it again a moment after editing stops, so PdfCraft — which
//! reloads a file that changes on disk — shows what is being typed without a save.

use serde_json::{Value, json};

use crate::{WordApp, i18n, now_ms};

/// How long editing must pause before the linked PDF is written again, in milliseconds.
const IDLE_MS: f64 = 2000.0;

/// What the live update last saw.
#[derive(Default)]
pub struct PdfSync {
    /// The document revision the PDF was last written for (or seen at while unlinked).
    rev: u64,
    /// When the document last changed after that, while linked.
    changed_at: Option<f64>,
}

/// A command's `{"launch": app, "path": …}` request: open the file in the sibling app.
pub(crate) fn launch(app: &mut WordApp, req: &Value) {
    let (Some(target), Some(path)) = (req.get("launch").and_then(Value::as_str), req.get("path").and_then(Value::as_str)) else { return };
    let Some(hook) = app.services.launch_app.as_ref() else {
        app.status(i18n::fmt(tl!("The PDF is ready: {path}"), &[("path", path)]));
        return;
    };
    match hook(target, path) {
        Ok(()) => app.status(i18n::fmt(tl!("Opened in PdfCraft: {path}"), &[("path", path)])),
        Err(e) => {
            log::warn!("opening {path} in {target}: {e}");
            app.status(i18n::fmt(tl!("PdfCraft couldn't be started ({error}). The PDF is at {path}"), &[("error", &e), ("path", path)]));
        }
    }
}

/// Once a frame: write the linked PDF again after a pause in editing.
pub(crate) fn poll(app: &mut WordApp, ctx: &egui::Context) {
    let rev = app.session.rev();
    if app.session.pdf_link.is_none() {
        app.pdf_sync = PdfSync { rev, changed_at: None };
        return;
    }
    let now = now_ms();
    if rev != app.pdf_sync.rev {
        app.pdf_sync.rev = rev;
        app.pdf_sync.changed_at = Some(now);
    }
    let Some(at) = app.pdf_sync.changed_at else { return };
    let wait = IDLE_MS - (now - at);
    if wait > 0.0 {
        // `wait` is at most IDLE_MS, so the cast can't overflow.
        ctx.request_repaint_after(std::time::Duration::from_millis(wait.ceil() as u64));
        return;
    }
    app.pdf_sync.changed_at = None;
    if let Err(e) = app.session.run("file.updatePdf", &json!({})) {
        log::warn!("updating the linked PDF: {e}");
        app.status(i18n::fmt(tl!("The linked PDF wasn't updated: {error}"), &[("error", &e.to_string())]));
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::{Services, WordApp};

    #[test]
    fn edits_reach_the_linked_pdf_after_a_pause() {
        let dir = std::env::temp_dir().join(format!("wc-pdf-link-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pdf = dir.join("Live.pdf");
        let mut app = WordApp::new(wordcraft_engine::Session::new(wordcraft_engine::sample::sample_document()), Services::default());
        let ctx = egui::Context::default();
        app.session.run("file.pdfSync", &json!({"value": true, "path": pdf.to_string_lossy()})).unwrap();
        super::poll(&mut app, &ctx);
        std::fs::write(&pdf, b"stale").unwrap();
        app.session.run("text.insert", &json!({"text": "More "})).unwrap();
        super::poll(&mut app, &ctx);
        // Still typing: not yet.
        assert_eq!(std::fs::read(&pdf).unwrap(), b"stale");
        // The pause has passed.
        app.pdf_sync.changed_at = Some(crate::now_ms() - super::IDLE_MS - 1.0);
        super::poll(&mut app, &ctx);
        assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF"));
        assert!(app.pdf_sync.changed_at.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn launch_uses_the_host_hook() {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let log = seen.clone();
        let services = Services {
            launch_app: Some(Box::new(move |app: &str, path: &str| {
                log.borrow_mut().push((app.to_string(), path.to_string()));
                Ok(())
            })),
            ..Default::default()
        };
        let mut app = WordApp::new(wordcraft_engine::Session::new(wordcraft_engine::sample::sample_document()), services);
        super::launch(&mut app, &json!({"launch": "pdfcraft", "path": "/tmp/x.pdf"}));
        assert_eq!(seen.borrow().as_slice(), &[("pdfcraft".to_string(), "/tmp/x.pdf".to_string())]);
        // Junk requests are ignored.
        super::launch(&mut app, &json!({"launch": 5}));
        assert_eq!(seen.borrow().len(), 1);
    }
}
