//! Starting a sibling Craft app with a file (`Services::launch_app`): File › Export › Open in
//! PdfCraft.
//!
//! The app is looked for, in order: the `PDFCRAFT_BIN` environment variable (any sibling:
//! `<NAME>_BIN`), next to this executable (an install or `cargo build` that puts both in one
//! folder), then on `PATH`. On macOS an installed `PdfCraft.app` is also tried through `open -a`.
//! The child runs on its own; WordCraft doesn't wait for it.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The sibling apps WordCraft may start, by the name commands use.
const APPS: &[(&str, &str, &str)] = &[
    // (name, executable stem, macOS bundle name)
    ("pdfcraft", "pdfcraft", "PdfCraft"),
];

/// The hook for `Services::launch_app`.
pub fn hook() -> Box<dyn Fn(&str, &str) -> Result<(), String>> {
    Box::new(|app, path| launch(app, Path::new(path)))
}

fn launch(app: &str, file: &Path) -> Result<(), String> {
    let &(_, stem, bundle) = APPS.iter().find(|(name, _, _)| *name == app).ok_or_else(|| format!("unknown app {app:?}"))?;
    if let Some(exe) = find_executable(stem) {
        return spawn(Command::new(exe).arg(file));
    }
    if cfg!(target_os = "macos") {
        return spawn(Command::new("open").arg("-a").arg(bundle).arg(file));
    }
    Err(format!("{stem} wasn't found: install it, put it next to WordCraft or set {}_BIN", stem.to_ascii_uppercase()))
}

fn spawn(cmd: &mut Command) -> Result<(), String> {
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    // Reaped on a thread so a long-lived child never stays a zombie on Unix.
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

fn exe_name(stem: &str) -> String {
    if cfg!(windows) { format!("{stem}.exe") } else { stem.to_string() }
}

fn find_executable(stem: &str) -> Option<PathBuf> {
    let env = std::env::var_os(format!("{}_BIN", stem.to_ascii_uppercase())).map(PathBuf::from);
    let beside = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join(exe_name(stem))));
    let on_path = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).map(|d| d.join(exe_name(stem))).collect::<Vec<_>>()).unwrap_or_default();
    pick(env, beside, on_path, |p| p.is_file())
}

/// The first candidate that is a file: the configured one, the one beside us, then `PATH`'s.
fn pick(env: Option<PathBuf>, beside: Option<PathBuf>, on_path: Vec<PathBuf>, is_file: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    env.into_iter().chain(beside).chain(on_path).find(|p| is_file(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_configured_binary_wins_then_the_neighbour_then_path() {
        let all = |_: &Path| true;
        let env = Some(PathBuf::from("/opt/pdfcraft"));
        let beside = Some(PathBuf::from("/apps/pdfcraft"));
        let path = vec![PathBuf::from("/usr/bin/pdfcraft")];
        assert_eq!(pick(env.clone(), beside.clone(), path.clone(), all), env);
        assert_eq!(pick(None, beside.clone(), path.clone(), all), beside);
        assert_eq!(pick(None, None, path.clone(), all), path.first().cloned());
        // Missing files are skipped.
        assert_eq!(pick(env, beside, path.clone(), |p| p == Path::new("/usr/bin/pdfcraft")), path.first().cloned());
    }

    #[test]
    fn unknown_apps_are_refused() {
        assert!(launch("photoshop", Path::new("x.pdf")).is_err());
    }
}
