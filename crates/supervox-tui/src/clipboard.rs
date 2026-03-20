//! Clipboard helper — copy text to system clipboard via arboard.

use arboard::Clipboard;

/// Copy text to the system clipboard.
pub fn copy_to_clipboard(text: &str) -> Result<(), String> {
    let mut clipboard = Clipboard::new().map_err(|e| format!("Clipboard init: {e}"))?;
    clipboard
        .set_text(text)
        .map_err(|e| format!("Clipboard set: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore] // Requires display server / macOS pasteboard
    fn copy_and_verify() {
        copy_to_clipboard("supervox test").unwrap();
        let mut clipboard = Clipboard::new().unwrap();
        let text = clipboard.get_text().unwrap();
        assert_eq!(text, "supervox test");
    }
}
