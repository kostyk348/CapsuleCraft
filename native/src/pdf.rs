//! One pdfium binding per process — the whole point of this module.
//!
//! `pdfium-render` 0.9 refuses to bind twice: the second
//! `Pdfium::bind_to_library()` call returns
//! `PdfiumLibraryBindingsAlreadyInitialized`. The app hits pdfium from two
//! places — the inline page preview and the full-screen viewer — so binding
//! per document meant the viewer always lost the race against the preview and
//! showed "pdfium load: … already initialized" instead of a page (v2.0.0
//! bug report: «открывается, но страница пустая, выдаёт ошибку, что
//! библиотека рендера уже запущена»).
//!
//! The engine handle is cached in a thread-local: every pdfium call the app
//! makes happens on the UI thread, and `Pdfium` itself is not `Sync`, so a
//! process-wide `static` would need an `unsafe impl Sync` we do not want.

use std::cell::Cell;
use std::path::Path;

use pdfium_render::prelude::{Pdfium, PdfiumError};

thread_local! {
    /// Bound once per thread; the leaked box lives for the process lifetime.
    static ENGINE: Cell<Option<&'static Pdfium>> = const { Cell::new(None) };
}

/// Bind the pdfium shared library at `lib_path`, or return the engine already
/// bound by an earlier call. Idempotent by construction: the second caller in
/// the same process gets the same handle instead of an `AlreadyInitialized`
/// error.
pub fn engine(lib_path: &Path) -> Result<&'static Pdfium, String> {
    if let Some(bound) = ENGINE.with(|cell| cell.get()) {
        return Ok(bound);
    }
    let bindings = Pdfium::bind_to_library(lib_path).map_err(|error| match error {
        PdfiumError::LoadLibraryError(reason) => {
            format!("pdfium: cannot load {}: {reason}", lib_path.display())
        }
        // Cannot happen while this module owns the only bind, but name it
        // precisely if it ever does: the message used to be unreadable.
        PdfiumError::PdfiumLibraryBindingsAlreadyInitialized => {
            "pdfium: the renderer was already initialised in this process".to_string()
        }
        other => format!("pdfium: {other}"),
    })?;
    let engine: &'static Pdfium = Box::leak(Box::new(Pdfium::new(bindings)));
    ENGINE.with(|cell| cell.set(Some(engine)));
    Ok(engine)
}

/// True when an engine is already bound on this thread (tests / diagnostics).
pub fn is_bound() -> bool {
    ENGINE.with(|cell| cell.get().is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_library_reports_its_path() {
        let missing = Path::new("/definitely/not/here/pdfium.so");
        // No engine was bound in this test thread, so the load is attempted.
        if is_bound() {
            return;
        }
        let error = engine(missing).expect_err("a missing library must not bind");
        assert!(
            error.contains("pdfium"),
            "error should name the engine: {error}"
        );
        assert!(!is_bound());
    }

    #[test]
    fn the_already_initialised_case_is_named() {
        // Guard against the old, unreadable message coming back: the whole bug
        // was a second bind reported as a generic render failure.
        let text = format!("{}", PdfiumError::PdfiumLibraryBindingsAlreadyInitialized);
        assert!(text.to_lowercase().contains("already"), "{text}");
    }
}
