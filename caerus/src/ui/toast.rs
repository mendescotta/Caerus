//! In-window toasts for results that do not need a dialog (refresh finished,
//! cache cleaned, an action that failed). The libadwaita build uses
//! `AdwToastOverlay` instead; this is the plain-GTK equivalent.
//!
//! Card styling follows Shelly's `.toast-card` (Seafoam-Labs/Shelly-ALPM,
//! GPL-3.0); the card is styled in `style.css` and by the built-in theme.

use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

const SHOW_SECONDS: u64 = 6;
const SHOW_SECONDS_ERROR: u64 = 10;

/// True if `needle` occurs in `haystack` ignoring ASCII case, without
/// allocating a lowercased copy of `haystack`.
fn contains_ascii_case_insensitive(haystack: &str, needle: &str) -> bool {
    let haystack = haystack.as_bytes();
    let needle = needle.as_bytes();
    needle.len() <= haystack.len()
        && haystack
            .windows(needle.len())
            .any(|window| window.eq_ignore_ascii_case(needle))
}

/// A message that reports a failure stays longer and gets the error accent.
pub fn is_error(message: &str) -> bool {
    ["failed", "could not", "error", "refused", "cannot"]
        .iter()
        .any(|word| contains_ascii_case_insensitive(message, word))
}

struct Current {
    card: gtk::Revealer,
    timeout: Option<glib::SourceId>,
}

#[derive(Clone)]
pub struct ToastHost {
    overlay: gtk::Overlay,
    area: gtk::Box,
    current: Rc<RefCell<Option<Current>>>,
}

impl ToastHost {
    pub fn new(child: &impl IsA<gtk::Widget>) -> Self {
        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(child));
        let area = gtk::Box::new(gtk::Orientation::Vertical, 0);
        area.set_halign(gtk::Align::Center);
        area.set_valign(gtk::Align::End);
        area.add_css_class("toast-area");
        overlay.add_overlay(&area);
        Self {
            overlay,
            area,
            current: Rc::new(RefCell::new(None)),
        }
    }

    pub fn widget(&self) -> &gtk::Overlay {
        &self.overlay
    }

    /// Show a toast, replacing the one on screen.
    pub fn show(&self, message: &str) {
        self.dismiss();
        let error = is_error(message);
        let card = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .accessible_role(gtk::AccessibleRole::Alert)
            .build();
        card.add_css_class("toast-card");
        card.add_css_class(if error { "toast-err" } else { "toast-ok" });
        let label = gtk::Label::new(Some(message));
        label.set_wrap(true);
        label.set_max_width_chars(70);
        label.set_xalign(0.0);
        label.set_hexpand(true);
        label.update_property(&[gtk::accessible::Property::Label(message)]);
        card.append(&label);
        let close = gtk::Button::from_icon_name("edit-clear-symbolic");
        close.add_css_class("flat");
        close.set_valign(gtk::Align::Center);
        close.set_tooltip_text(Some("Dismiss"));
        close.update_property(&[gtk::accessible::Property::Label("Dismiss message")]);
        card.append(&close);

        let revealer = gtk::Revealer::new();
        revealer.set_transition_type(gtk::RevealerTransitionType::SlideUp);
        revealer.set_child(Some(&card));
        revealer.add_css_class("toast-revealer");
        self.area.append(&revealer);
        revealer.set_reveal_child(true);

        let seconds = if error {
            SHOW_SECONDS_ERROR
        } else {
            SHOW_SECONDS
        };
        let host = self.clone();
        let timeout = glib::timeout_add_local_once(Duration::from_secs(seconds), move || {
            // The source is finished once it fires; forget it before dismissing.
            if let Some(current) = host.current.borrow_mut().as_mut() {
                current.timeout = None;
            }
            host.dismiss();
        });
        *self.current.borrow_mut() = Some(Current {
            card: revealer,
            timeout: Some(timeout),
        });
        let host = self.clone();
        close.connect_clicked(move |_| host.dismiss());
    }

    pub fn dismiss(&self) {
        let current = self.current.borrow_mut().take();
        if let Some(current) = current {
            if let Some(timeout) = current.timeout {
                timeout.remove();
            }
            self.area.remove(&current.card);
        }
    }

    #[cfg(test)]
    pub fn shown(&self) -> Option<(String, bool)> {
        let current = self.current.borrow();
        let card = current.as_ref()?.card.child()?;
        let label = card.first_child()?.downcast::<gtk::Label>().ok()?;
        Some((label.text().to_string(), card.has_css_class("toast-err")))
    }
}

/// Runs inside the single display test (GTK stays on one thread).
#[cfg(test)]
pub fn smoke_test_toasts() {
    let host = ToastHost::new(&gtk::Label::new(Some("content")));
    assert!(host.shown().is_none());
    host.show("Changes applied.");
    assert_eq!(host.shown(), Some(("Changes applied.".into(), false)));
    host.show("Upgrade failed or was incomplete; see log.");
    assert_eq!(
        host.shown(),
        Some(("Upgrade failed or was incomplete; see log.".into(), true)),
        "a new toast replaces the old one"
    );
    assert_eq!(host.area.observe_children().n_items(), 1);
    host.dismiss();
    assert!(host.shown().is_none());
    assert_eq!(host.area.observe_children().n_items(), 0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failures_are_recognised_by_their_wording() {
        assert!(is_error("Repository sync failed — loading local data."));
        assert!(is_error("Could not reload package state: x"));
        assert!(is_error("Error loading packages: x"));
        assert!(!is_error("Changes applied."));
        assert!(!is_error("Upgrade complete."));
    }
}
