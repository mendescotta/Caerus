//! Small shared building blocks for the task pages: the page header, the
//! icon + title empty state and the status pill. Spacing lives in CSS
//! (`.page-head`, `.empty-state`, `.pkg-status`) so a theme can change it.

use gtk::prelude::*;

/// Page heading with a subtitle line underneath.
#[derive(Clone)]
pub struct PageHeader {
    root: gtk::Box,
    subtitle: gtk::Label,
}

impl PageHeader {
    pub fn new(title: &str, subtitle: &str) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 2);
        root.add_css_class("page-head");
        let heading = gtk::Label::new(Some(title));
        heading.set_xalign(0.0);
        heading.add_css_class("title-2");
        heading.add_css_class("page-heading");
        heading.update_property(&[gtk::accessible::Property::Label(title)]);
        root.append(&heading);
        let subtitle_label = gtk::Label::new(Some(subtitle));
        subtitle_label.set_xalign(0.0);
        subtitle_label.set_wrap(true);
        subtitle_label.add_css_class("dim-label");
        subtitle_label.add_css_class("page-subtitle");
        subtitle_label.set_visible(!subtitle.is_empty());
        root.append(&subtitle_label);
        Self {
            root,
            subtitle: subtitle_label,
        }
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.root
    }

    pub fn set_subtitle(&self, text: &str) {
        self.subtitle.set_text(text);
        self.subtitle.set_visible(!text.is_empty());
        self.subtitle
            .update_property(&[gtk::accessible::Property::Description(text)]);
    }

    #[cfg(test)]
    pub fn subtitle_text(&self) -> String {
        self.subtitle.text().to_string()
    }
}

/// A large icon, a title and an optional subtitle, centered in the page.
#[derive(Clone)]
pub struct EmptyState {
    root: gtk::Box,
    icon: gtk::Image,
    title: gtk::Label,
    subtitle: gtk::Label,
}

impl EmptyState {
    pub fn new(icon_name: &str, title: &str, subtitle: &str) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
        root.add_css_class("empty-state");
        root.set_valign(gtk::Align::Center);
        root.set_halign(gtk::Align::Center);
        let icon = gtk::Image::from_icon_name(icon_name);
        icon.set_pixel_size(64);
        icon.add_css_class("empty-state-icon");
        root.append(&icon);
        let title_label = gtk::Label::new(Some(title));
        title_label.add_css_class("title-3");
        title_label.set_wrap(true);
        title_label.set_justify(gtk::Justification::Center);
        root.append(&title_label);
        let subtitle_label = gtk::Label::new(Some(subtitle));
        subtitle_label.add_css_class("dim-label");
        subtitle_label.set_wrap(true);
        subtitle_label.set_justify(gtk::Justification::Center);
        subtitle_label.set_max_width_chars(60);
        subtitle_label.set_visible(!subtitle.is_empty());
        root.append(&subtitle_label);
        Self {
            root,
            icon,
            title: title_label,
            subtitle: subtitle_label,
        }
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.root
    }

    pub fn set(&self, icon_name: &str, title: &str, subtitle: &str) {
        self.icon.set_icon_name(Some(icon_name));
        self.title.set_text(title);
        self.subtitle.set_text(subtitle);
        self.subtitle.set_visible(!subtitle.is_empty());
        let description = if subtitle.is_empty() {
            title.to_string()
        } else {
            format!("{title}. {subtitle}")
        };
        self.root
            .update_property(&[gtk::accessible::Property::Description(&description)]);
    }

    /// Append an action (for example a retry button) under the text.
    pub fn add_action(&self, button: &impl IsA<gtk::Widget>) {
        self.root.append(button);
    }

    #[cfg(test)]
    pub fn title_text(&self) -> String {
        self.title.text().to_string()
    }

    #[cfg(test)]
    pub fn subtitle_text(&self) -> String {
        self.subtitle.text().to_string()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PillKind {
    Done,
    Failed,
    Held,
    Neutral,
    Warn,
    Accent,
}

impl PillKind {
    const ALL: [Self; 6] = [
        Self::Done,
        Self::Failed,
        Self::Held,
        Self::Neutral,
        Self::Warn,
        Self::Accent,
    ];

    const fn class(self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Held => "held",
            Self::Neutral => "neutral",
            Self::Warn => "warn",
            Self::Accent => "accent",
        }
    }
}

/// One status pill style (`.pkg-status` plus a variant) for rows, dialogs and
/// history, instead of per-place coloured text.
pub fn status_pill(text: &str, kind: PillKind) -> gtk::Label {
    let pill = gtk::Label::new(None);
    pill.add_css_class("chip");
    pill.add_css_class("pkg-status");
    pill.set_valign(gtk::Align::Center);
    set_pill(&pill, text, kind);
    pill
}

pub fn set_pill(pill: &gtk::Label, text: &str, kind: PillKind) {
    // Rows update their pill text on every progress event but usually keep
    // the same kind (e.g. repeated "Accent" stages); skip the CSS churn
    // unless the kind actually changed.
    if !pill.has_css_class(kind.class()) {
        for other in PillKind::ALL {
            pill.remove_css_class(other.class());
        }
        pill.add_css_class(kind.class());
    }
    pill.set_text(text);
}
