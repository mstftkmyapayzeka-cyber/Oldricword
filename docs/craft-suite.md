# WordCraft ⇄ PdfCraft

Status: first version (2026-10-13). WordCraft and PdfCraft stay two apps with their own
engines, and hand documents to each other so they work as one suite.

## WordCraft → PdfCraft: a PDF that follows the document

**File › Export › PdfCraft › Open in PdfCraft** (`file.openInPdfCraft`) writes the document as a
PDF and opens it in PdfCraft. The PDF stays *linked* (**Keep PDF in Sync**, `file.pdfSync`):

- every save (Ctrl+S, AutoSave) writes the linked PDF again;
- the desktop app also writes it two seconds after editing pauses, without a save;
- PdfCraft reloads a file that changes on disk (Preferences ▸ *Reload documents when their files
  change on disk*, on by default), so its window shows what is being typed.

`file.updatePdf` writes the linked PDF now. The first export never overwrites a file: it uses a
free name beside the document (`Report.pdf`, `Report (2).pdf`, …), or the temporary folder for an
unsaved document. Opening or creating another document drops the link.

Headless sessions (`wordcraft-cli`, MCP) run the same commands: the PDF is written and linked,
and nothing is started.

## PdfCraft → WordCraft: edit a PDF as a Word document

PdfCraft's **File ▸ Edit in WordCraft** (`export.wordcraft`) exports the PDF to Word (the same
export as File ▸ Export to Word…) beside the PDF under a free name and opens it in WordCraft. From
there, Open in PdfCraft brings the edited document back as a new PDF.

## Finding the other app

Each app looks for the other in this order:

1. `PDFCRAFT_BIN` / `WORDCRAFT_BIN` (the path to the executable);
2. the folder of the running executable (install both side by side, or build both into one
   target folder);
3. `PATH` (`pdfcraft`, `wordcraft`);
4. on macOS, `open -a PdfCraft` / `open -a WordCraft`.

When it isn't found, the status bar says where the file was written.

## Not yet

- One window and one binary for both apps. The workspaces pin the shared `craft-ui` and
  `craft-print` crates at different revisions, and PdfCraft patches `egui-wgpu` and `hayro` with
  vendored copies that would apply to WordCraft too; those need aligning first.
- Edits made in PdfCraft (comments, signatures) don't flow back into the Word document: the
  linked PDF is rewritten from the document, so keep PDF-side work in a copy (Save As in PdfCraft)
  or turn Keep PDF in Sync off.
- The web builds can't start another app or keep a file in sync: PdfCraft's Edit in WordCraft
  downloads the Word document instead, and in WordCraft's web build File › Export › PDF is the
  way to get the PDF.
- The PDF is written on the UI thread; on a very long document the update after a pause in
  typing can be felt; turn Keep PDF in Sync off there and export when needed.
