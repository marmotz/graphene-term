//! Copy and paste of a session: the clipboard and the primary selection.

use super::*;

impl Session {
    pub(super) fn paste(&mut self) {
        self.reset_blink();
        let Some(text) = self.clipboard.as_mut().and_then(|c| c.get_text().ok()) else {
            return;
        };
        self.paste_text(&text);
    }

    /// Pastes `text` into every pane typed input goes to, each bracketing it with its
    /// own mode.
    pub(super) fn paste_text(&mut self, text: &str) {
        for id in self.workspace.input_targets() {
            if let Some(pane) = self.workspace.pane_mut(id) {
                pane.send_paste(text);
            }
        }
    }

    /// Writes the cleaned selection to the clipboard. Without selection, the shortcut is
    /// still swallowed so it never sends `^C`.
    pub(super) fn copy(&mut self) {
        match self.workspace.active_pane().and_then(Pane::copy_content) {
            Some(Content::Text(text)) => self.set_clipboard(Some(text)),
            Some(Content::Image { text, png }) => {
                #[cfg(target_os = "linux")]
                let result = clipboard::set_text_and_image(text.clone(), png);
                #[cfg(target_os = "macos")]
                let result = self
                    .clipboard
                    .as_mut()
                    .ok_or_else(|| anyhow::anyhow!("clipboard unavailable"))
                    .and_then(|clipboard| clipboard::set_image(clipboard, &png));

                if let Err(error) = result {
                    log::warn!("image copy failed, copying the text only: {error:#}");
                    self.set_clipboard(text);
                }
            }
            None => {}
        }
    }

    /// Copies one part of the target block to the clipboard. An empty part leaves the
    /// clipboard untouched.
    pub(super) fn copy_block_part(&mut self, part: Part) {
        let text = self
            .workspace
            .active_pane()
            .and_then(|pane| pane.block_part_text(part));
        self.set_clipboard(text);
    }

    pub(super) fn set_clipboard(&mut self, text: Option<String>) {
        let Some(text) = text else {
            return;
        };
        if let Some(Err(error)) = self.clipboard.as_mut().map(|c| c.set_text(text)) {
            log::warn!("copy failed: {error}");
        }
    }

    pub(super) fn set_primary(&mut self, text: String) {
        #[cfg(target_os = "linux")]
        {
            let Some(clipboard) = self.clipboard.as_mut() else {
                return;
            };
            // Some Wayland compositors do not support the primary selection.
            if let Err(error) = clipboard
                .set()
                .clipboard(LinuxClipboardKind::Primary)
                .text(text)
            {
                log::debug!("primary selection unavailable: {error}");
            }
        }
        #[cfg(not(target_os = "linux"))]
        let _ = text;
    }

    pub(super) fn paste_primary(&mut self) {
        self.reset_blink();
        #[cfg(target_os = "linux")]
        {
            let Some(clipboard) = self.clipboard.as_mut() else {
                return;
            };
            let text = match clipboard
                .get()
                .clipboard(LinuxClipboardKind::Primary)
                .text()
            {
                Ok(text) => text,
                Err(error) => {
                    log::debug!("primary selection unavailable: {error}");
                    return;
                }
            };
            self.paste_text(&text);
        }
    }
}
