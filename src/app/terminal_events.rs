//! Events sent by the terminals of a session.

use super::*;

impl Session {
    /// Handles an event from the terminal of the pane `id`; events of a pane already
    /// closed are dropped.
    pub(super) fn handle_terminal_event(
        &mut self,
        id: PaneId,
        event: TermEvent,
        event_loop: &ActiveEventLoop,
    ) {
        let visible = self.workspace.is_visible(id);
        let active = self.workspace.active_pane_id() == Some(id);
        // The tab bar shows the title of every tab.
        let bar_shown = self.tab_bar_shown();
        let Some(pane) = self.workspace.pane_mut(id) else {
            return;
        };
        let mut notices = Vec::new();
        match event {
            TermEvent::Wakeup => {
                pane.terminal.wakeup_handled();
                // A notice is always followed by a `Wakeup`: none is left behind.
                notices.extend(
                    pane::lock(&pane.terminal.timeline)
                        .take_notices()
                        .into_iter()
                        .map(Notice::Pane),
                );
                if self.first_frame.output() {
                    perf::mark("first PTY output");
                }
                pane.validate_selection();
                pane.finish_extend();
                // Without OSC title, the title follows the shell's directory.
                let title = active && pane.title_refresh_due(Instant::now());
                if visible {
                    self.window.request_redraw();
                }
                let hovered = self.hovered_cell.is_some_and(|(pane, ..)| pane == id);
                if hovered {
                    self.update_hover(true);
                }
                if active || hovered {
                    self.update_pointer();
                }
                if title {
                    self.refresh_title();
                }
            }
            TermEvent::MouseCursorDirty | TermEvent::CursorBlinkingChange => {
                if visible {
                    self.window.request_redraw();
                }
            }
            TermEvent::Title(title) => {
                pane.title = Some(title);
                if active {
                    self.refresh_title();
                }
                if bar_shown {
                    self.window.request_redraw();
                }
            }
            TermEvent::ResetTitle => {
                pane.title = None;
                if active {
                    self.refresh_title();
                }
                if bar_shown {
                    self.window.request_redraw();
                }
            }
            TermEvent::PtyWrite(text) => pane.terminal.write(text.into_bytes()),
            TermEvent::ClipboardStore(_, text) => {
                if let Some(clipboard) = self.clipboard.as_mut() {
                    let _ = clipboard.set_text(text);
                }
            }
            TermEvent::ClipboardLoad(_, format) => {
                let text = self
                    .clipboard
                    .as_mut()
                    .and_then(|c| c.get_text().ok())
                    .unwrap_or_default();
                pane.terminal.write(format(&text).into_bytes());
            }
            TermEvent::ColorRequest(index, format) => {
                let color = pane.terminal.term.lock().colors()[index]
                    .unwrap_or_else(|| self.palette.by_index(index));
                pane.terminal.write(format(color).into_bytes());
            }
            TermEvent::TextAreaSizeRequest(format) => {
                pane.terminal
                    .write(format(pane.layout.window_size()).into_bytes());
            }
            TermEvent::Bell => {
                self.window
                    .request_user_attention(Some(winit::window::UserAttentionType::Informational));
                notices.push(Notice::Bell);
            }
            TermEvent::Exit | TermEvent::ChildExit(_) => self.close_pane(id, event_loop),
        }
        for notice in notices {
            self.notify(id, notice);
        }
    }
}
