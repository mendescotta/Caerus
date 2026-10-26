use crate::backend::package::{PkgMark, PkgState};
use crate::backend::package_store::PackageStore;
use crate::ui::dialog_util::{cancel_button_row, modal_window, present_focused, text_list_row};
use gtk::prelude::*;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

const MAX_IMPACT_ROWS: usize = 200;

fn is_still_installed_afterward(state: PkgState, mark: PkgMark) -> bool {
    let installed = matches!(
        state,
        PkgState::Installed | PkgState::Upgradable | PkgState::OnHold | PkgState::Broken
    );
    installed && !matches!(mark, PkgMark::Remove | PkgMark::Purge)
}

fn still_installed_afterward(store: &PackageStore, name: &str) -> bool {
    match store.state_and_mark(name) {
        Some((state, mark)) => is_still_installed_afterward(state, mark),
        None => false,
    }
}

pub fn confirm_remove_impact(
    parent: Option<&gtk::Window>,
    store: &PackageStore,
    pkgname: &str,
    cb: impl Fn(bool) + 'static,
) {
    let parent = parent.cloned();
    let store2 = store.clone();
    let pkgname = pkgname.to_string();
    store.get_rdeps_transitive_async(&pkgname.clone(), move |rdeps| {
        let affected: Vec<(String, String)> = rdeps
            .unwrap_or_default()
            .into_iter()
            .filter(|(name, _)| name != &pkgname && still_installed_afterward(&store2, name))
            .collect();

        if affected.is_empty() {
            cb(true);
            return;
        }
        show_impact_dialog(
            parent.as_ref(),
            std::slice::from_ref(&pkgname),
            affected,
            cb,
        );
    });
}

pub fn confirm_bulk_remove_impact(
    parent: Option<&gtk::Window>,
    store: &PackageStore,
    names: Vec<String>,
    cb: impl Fn(bool) + 'static,
) {
    if names.is_empty() {
        cb(true);
        return;
    }
    let roots: HashSet<String> = names.iter().cloned().collect();
    let snapshot = store.state_and_mark_snapshot();
    let parent = parent.cloned();
    store.get_rdeps_transitive_many_async(names, move |rdeps| {
        let affected = bulk_affected(&roots, &snapshot, rdeps.unwrap_or_default());
        if affected.is_empty() {
            cb(true);
            return;
        }
        let mut roots: Vec<String> = roots.into_iter().collect();
        roots.sort();
        show_impact_dialog(parent.as_ref(), &roots, affected, cb);
    });
}

fn bulk_affected(
    roots: &HashSet<String>,
    snapshot: &HashMap<String, (PkgState, PkgMark)>,
    rdeps: Vec<(String, String)>,
) -> Vec<(String, String)> {
    rdeps
        .into_iter()
        .filter(|(name, _)| {
            !roots.contains(name)
                && snapshot
                    .get(name)
                    .is_some_and(|&(state, mark)| is_still_installed_afterward(state, mark))
        })
        .collect()
}

fn capped_rows(sorted: &[(String, String)], cap: usize) -> (&[(String, String)], usize) {
    let visible = sorted.len().min(cap);
    (&sorted[..visible], sorted.len() - visible)
}

/// Wording for the reverse-dependency warning.
struct ImpactText {
    title: &'static str,
    verb: &'static str,
    note: Option<&'static str>,
    button: &'static str,
}

const REMOVE_IMPACT: ImpactText = ImpactText {
    title: "Other Packages Depend On This",
    verb: "Removing",
    note: None,
    button: "Remove Anyway",
};

const FORCE_IMPACT: ImpactText = ImpactText {
    title: "Force Remove?",
    verb: "Force-removing",
    note: Some(
        "Force removal (xbps-remove -F) skips the dependency check and runs \
         immediately; it is not queued for review. Dependent packages stay \
         installed and may stop working until their dependency is reinstalled.",
    ),
    button: "Force Remove",
};

fn show_impact_dialog(
    parent: Option<&gtk::Window>,
    roots: &[String],
    affected: Vec<(String, String)>,
    cb: impl Fn(bool) + 'static,
) {
    show_impact_dialog_with(parent, roots, Some(affected), &REMOVE_IMPACT, cb);
}

/// Dialog heading; `affected` is `None` when the dependency check failed,
/// which must never read as "nothing depends on it".
fn impact_heading(verb: &str, subject: &str, affected: Option<usize>) -> String {
    match affected {
        None => format!(
            "{verb} {subject}: could not check which installed packages depend on it, \
             so packages that need it may break."
        ),
        Some(0) => format!("{verb} {subject}: no other installed package depends on it."),
        Some(n) => format!(
            "{verb} {subject} may break {n} other installed package{} that depend{} on it:",
            if n == 1 { "" } else { "s" },
            if n == 1 { "s" } else { "" },
        ),
    }
}

fn subject_of(roots: &[String]) -> String {
    match roots {
        [one] => one.clone(),
        many => format!("{} selected packages", many.len()),
    }
}

fn show_impact_dialog_with(
    parent: Option<&gtk::Window>,
    roots: &[String],
    affected: Option<Vec<(String, String)>>,
    text: &ImpactText,
    cb: impl Fn(bool) + 'static,
) {
    let heading_text = impact_heading(
        text.verb,
        &subject_of(roots),
        affected.as_ref().map(Vec::len),
    );
    let affected = affected.unwrap_or_default();
    let n = affected.len();
    let cb: Rc<dyn Fn(bool)> = Rc::new(cb);

    let (dlg, outer) = modal_window(text.title, parent, true, (420, -1), 10);

    let heading = gtk::Label::new(Some(&heading_text));
    heading.set_xalign(0.0);
    heading.set_wrap(true);
    outer.append(&heading);
    if let Some(note) = text.note {
        let note = gtk::Label::new(Some(note));
        note.set_xalign(0.0);
        note.set_wrap(true);
        note.add_css_class("dim-label");
        outer.append(&note);
    }

    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_propagate_natural_height(true);
    scroll.set_max_content_height(360);
    scroll.set_vexpand(true);

    let mut sorted = affected;
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let (visible, hidden) = capped_rows(&sorted, MAX_IMPACT_ROWS);
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    for (name, via) in visible {
        let label = if roots.contains(via) {
            name.clone()
        } else {
            format!("{name} (via {via})")
        };
        list.append(&text_list_row(&label, false));
    }
    if hidden > 0 {
        list.append(&text_list_row(&format!("\u{2026}and {hidden} more"), false));
    }
    scroll.set_child(Some(&list));
    scroll.set_visible(n > 0);
    outer.append(&scroll);

    let (btn_box, cancel_btn) = cancel_button_row(4);
    let remove_btn = gtk::Button::with_label(text.button);
    remove_btn.add_css_class("destructive-action");
    btn_box.append(&remove_btn);
    outer.append(&btn_box);

    dlg.set_default_widget(Some(&cancel_btn));

    {
        let cb = cb.clone();
        let dlg = dlg.clone();
        cancel_btn.connect_clicked(move |_| {
            cb(false);
            dlg.destroy();
        });
    }
    {
        let cb = cb.clone();
        let dlg = dlg.clone();
        let parent = parent.cloned();
        remove_btn.connect_clicked(move |_| {
            // Clear focus before destroying: the focused widget is often a
            // row's checkbox, and set_mark()'s splice() (run from the idle
            // callback below) recycles that exact row. GtkColumnView
            // unparents the row widget but the window's remembered
            // focus_widget isn't always cleared synchronously, and if the
            // window regains X11 focus (right as this dialog closes) before
            // that happens, GTK walks the now-parentless row widget's
            // ancestor chain in gtk_column_view_row_widget_get_column_view()
            // and segfaults on a NULL parent (upstream gtk4 4.24.1,
            // gtkcolumnviewrowwidget.c:45 — no NULL guard between the
            // GTK_IS_COLUMN_VIEW check and the forced second
            // _gtk_widget_get_parent() call). Clearing focus here makes sure
            // the window has nothing stale to restore.
            if let Some(p) = &parent {
                gtk::prelude::GtkWindowExt::set_focus(p, None::<&gtk::Widget>);
            }
            dlg.destroy();
            let cb = cb.clone();
            glib::source::idle_add_local_once(move || cb(true));
        });
    }
    {
        let cb = cb.clone();
        dlg.connect_close_request(move |_| {
            cb(false);
            glib::Propagation::Proceed
        });
    }

    present_focused(&dlg, &cancel_btn);
}

/// Confirm a forced removal, always: it bypasses review and the solver.
pub fn confirm_force_remove(
    parent: Option<&gtk::Window>,
    store: &PackageStore,
    names: Vec<String>,
    cb: impl Fn(bool) + 'static,
) {
    if names.is_empty() {
        return;
    }
    let roots: HashSet<String> = names.iter().cloned().collect();
    let snapshot = store.state_and_mark_snapshot();
    let parent = parent.cloned();
    store.get_rdeps_transitive_many_async(names, move |rdeps| {
        // A failed lookup is reported as unknown, never as "no dependents".
        let affected = rdeps.map(|rdeps| bulk_affected(&roots, &snapshot, rdeps));
        let mut roots: Vec<String> = roots.into_iter().collect();
        roots.sort();
        show_impact_dialog_with(parent.as_ref(), &roots, affected, &FORCE_IMPACT, cb);
    });
}

/// How to remove an installed package.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemovalChoice {
    /// `xbps-remove`: queued for review.
    Remove,
    /// `xbps-remove -R`: queued for review, also drops unused dependencies.
    Purge,
    /// `xbps-remove -F`: runs now, ignoring reverse dependencies.
    Force,
}

/// Ask whether to remove, purge or force-remove. `None` means cancelled.
pub fn choose_removal(
    parent: Option<&gtk::Window>,
    pkgname: &str,
    cb: impl Fn(Option<RemovalChoice>) + 'static,
) {
    let cb: Rc<dyn Fn(Option<RemovalChoice>)> = Rc::new(cb);
    let (dlg, outer) = modal_window(&format!("Remove {pkgname}?"), parent, true, (440, -1), 10);
    let heading = gtk::Label::new(Some(&format!("How should {pkgname} be removed?")));
    heading.set_xalign(0.0);
    heading.set_wrap(true);
    heading.add_css_class("heading");
    outer.append(&heading);

    let decided = Rc::new(std::cell::Cell::new(false));
    let choices = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let mut first = None;
    for (choice, title, detail, class) in [
        (
            RemovalChoice::Remove,
            "Remove",
            "Uninstall the package. Dependencies it pulled in stay installed.",
            None,
        ),
        (
            RemovalChoice::Purge,
            "Purge",
            "Uninstall it and any dependencies nothing else needs (xbps-remove -R).",
            None,
        ),
        (
            RemovalChoice::Force,
            "Force remove",
            "Uninstall now even if other packages depend on it (xbps-remove -F). \
             Not queued for review.",
            Some("destructive-action"),
        ),
    ] {
        let content = gtk::Box::new(gtk::Orientation::Vertical, 2);
        let title_label = gtk::Label::new(Some(title));
        title_label.set_xalign(0.0);
        title_label.add_css_class("heading");
        content.append(&title_label);
        let detail_label = gtk::Label::new(Some(detail));
        detail_label.set_xalign(0.0);
        detail_label.set_wrap(true);
        detail_label.add_css_class("dim-label");
        content.append(&detail_label);
        let button = gtk::Button::new();
        button.set_child(Some(&content));
        button.add_css_class("removal-choice");
        if let Some(class) = class {
            button.add_css_class(class);
        }
        button.update_property(&[
            gtk::accessible::Property::Label(title),
            gtk::accessible::Property::Description(detail),
        ]);
        let cb = cb.clone();
        let dlg_c = dlg.clone();
        let parent = parent.cloned();
        let decided = decided.clone();
        button.connect_clicked(move |_| {
            if decided.replace(true) {
                return;
            }
            // Same focus hazard as the impact dialog: the focused widget may be
            // a checkbox in a row that the resulting mark change recycles.
            if let Some(p) = &parent {
                gtk::prelude::GtkWindowExt::set_focus(p, None::<&gtk::Widget>);
            }
            dlg_c.destroy();
            let cb = cb.clone();
            glib::source::idle_add_local_once(move || cb(Some(choice)));
        });
        if first.is_none() {
            first = Some(button.clone());
        }
        choices.append(&button);
    }
    outer.append(&choices);

    let (btn_box, cancel_btn) = cancel_button_row(4);
    outer.append(&btn_box);
    {
        let cb = cb.clone();
        let dlg = dlg.clone();
        let decided = decided.clone();
        cancel_btn.connect_clicked(move |_| {
            if !decided.replace(true) {
                cb(None);
            }
            dlg.destroy();
        });
    }
    {
        let cb = cb.clone();
        dlg.connect_close_request(move |_| {
            if !decided.replace(true) {
                cb(None);
            }
            glib::Propagation::Proceed
        });
    }
    if let Some(first) = first {
        present_focused(&dlg, &first);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(entries: &[(&str, PkgState, PkgMark)]) -> HashMap<String, (PkgState, PkgMark)> {
        entries
            .iter()
            .map(|&(name, state, mark)| (name.to_string(), (state, mark)))
            .collect()
    }

    fn roots(names: &[&str]) -> HashSet<String> {
        names.iter().map(ToString::to_string).collect()
    }

    fn rdeps(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|&(name, via)| (name.to_string(), via.to_string()))
            .collect()
    }

    #[test]
    fn bulk_affected_keeps_only_still_installed_non_root_names() {
        let snapshot = snap(&[
            ("a", PkgState::Installed, PkgMark::None),
            ("b", PkgState::NotInstalled, PkgMark::None),
            ("c", PkgState::Installed, PkgMark::Remove),
            ("d", PkgState::Upgradable, PkgMark::None),
        ]);
        let walk = rdeps(&[
            ("a", "root1"),
            ("b", "root1"),
            ("c", "root1"),
            ("d", "root1"),
        ]);
        let affected = bulk_affected(&roots(&["root1"]), &snapshot, walk);
        assert_eq!(
            affected,
            vec![
                ("a".to_string(), "root1".to_string()),
                ("d".to_string(), "root1".to_string())
            ]
        );
    }

    #[test]
    fn bulk_affected_excludes_names_that_are_themselves_roots() {
        let snapshot = snap(&[("b", PkgState::Installed, PkgMark::None)]);
        let walk = rdeps(&[("b", "a")]);
        let affected = bulk_affected(&roots(&["a", "b"]), &snapshot, walk);
        assert!(affected.is_empty());
    }

    #[test]
    fn bulk_affected_drops_names_absent_from_the_snapshot() {
        let affected = bulk_affected(&roots(&["a"]), &HashMap::new(), rdeps(&[("x", "a")]));
        assert!(affected.is_empty());
    }

    #[test]
    fn failed_dependency_check_is_not_reported_as_no_dependents() {
        let unknown = impact_heading("Force-removing", "foo", None);
        assert!(unknown.contains("could not check"), "{unknown}");
        assert!(!unknown.contains("no other installed package"), "{unknown}");
        assert!(impact_heading("Removing", "foo", Some(0)).contains("no other installed package"));
        assert_eq!(
            impact_heading("Removing", "foo", Some(1)),
            "Removing foo may break 1 other installed package that depends on it:"
        );
    }

    #[test]
    fn capped_rows_splits_at_the_limit() {
        let sorted = rdeps(&[("a", "r"), ("b", "r"), ("c", "r")]);
        let (visible, hidden) = capped_rows(&sorted, 2);
        assert_eq!(visible.len(), 2);
        assert_eq!(hidden, 1);
    }

    #[test]
    fn capped_rows_reports_no_overflow_under_the_limit() {
        let sorted = rdeps(&[("a", "r")]);
        let (visible, hidden) = capped_rows(&sorted, 200);
        assert_eq!(visible.len(), 1);
        assert_eq!(hidden, 0);
    }
}
