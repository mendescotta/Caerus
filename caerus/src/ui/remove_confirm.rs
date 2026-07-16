//! If removing a package would leave any other *currently installed*
//! package's dependency unsatisfied, shows a confirmation dialog
//! transient for `parent` listing them. The install-side equivalent of
//! `deps_confirm.rs`, checking reverse rather than forward dependencies.
//!
//! Asynchronous: `cb` may fire after this function returns (a real
//! dialog was shown) or before it returns (nothing installed actually
//! depends on this package) — same shape as `deps_confirm`.

use crate::backend::package::{PkgMark, PkgState};
use crate::backend::package_store::PackageStore;
use crate::ui::dialog_util::{cancel_button_row, modal_window, present_focused, text_list_row};
use gtk::prelude::*;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// Rows shown before the list is truncated with a "…and K more" line —
/// a glibc-scale reverse-dependency closure can otherwise dump hundreds
/// of rows into an unscrollable-feeling dialog.
const MAX_IMPACT_ROWS: usize = 200;

/// Whether (`state`, `mark`) means this package is still going to be
/// installed after the batch runs: already-installed and not itself
/// marked for Remove/Purge.
fn is_still_installed_afterward(state: PkgState, mark: PkgMark) -> bool {
    let installed = matches!(
        state,
        PkgState::Installed | PkgState::Upgradable | PkgState::OnHold | PkgState::Broken
    );
    installed && !matches!(mark, PkgMark::Remove | PkgMark::Purge)
}

/// A reverse dependency only matters here if it's still going to be
/// installed after this batch runs.
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
    // The reverse-dependency walk runs on the xbps worker thread; the
    // dialog (or the fast-path `cb(true)`) follows once it reports back,
    // keeping the main loop responsive even if the worker is mid-reload.
    let parent = parent.cloned();
    let store2 = store.clone();
    let pkgname = pkgname.to_string();
    store.get_rdeps_transitive_async(&pkgname.clone(), move |rdeps| {
        // Transitive: `(affected_pkgname, direct_parent_that_pulled_it_in)`.
        // A name reached only through an intermediate package (parent !=
        // `pkgname` itself) gets annotated "(via parent)" below so the
        // dialog shows *why* it would break, not just that it would.
        let affected: Vec<(String, String)> = rdeps
            .unwrap_or_default()
            .into_iter()
            .filter(|(name, _)| name != &pkgname && still_installed_afterward(&store2, name))
            .collect();

        if affected.is_empty() {
            // The common case — don't interrupt removing a leaf package.
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

/// Multi-root counterpart to `confirm_remove_impact`, for a bulk Remove/
/// Purge mark applied to several packages at once (Delete over a
/// multi-selection, or the right-click context menu's bulk action): one
/// aggregate confirmation for the whole batch instead of either no
/// confirmation (every other bulk mark) or a chain of N per-package
/// dialogs. `names` is the batch itself — empty resolves `cb(true)`
/// immediately, same as the no-impact fast path below.
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

/// Pure filter over a raw multi-root transitive-rdeps walk: the subset
/// that would actually break a still-installed package, excluding the
/// removal selection itself (`roots`) — belt-and-suspenders on top of
/// the worker's multi-root BFS already seeding `visited` with every
/// root, so this stays correct even fed a hand-built walk. `snapshot`
/// mirrors `get_missing_deps_async`'s name -> (state, mark) map so this
/// needs no live `PackageStore`/GTK access, which is what makes it
/// directly testable.
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

/// Splits `sorted` into the rows to actually display (at most `cap`) and
/// how many were left out — pure so the truncation math is testable
/// without a GTK list to inspect.
fn capped_rows(sorted: &[(String, String)], cap: usize) -> (&[(String, String)], usize) {
    let visible = sorted.len().min(cap);
    (&sorted[..visible], sorted.len() - visible)
}

/// `roots` is the package (single-item slice) or batch (multi-item)
/// being removed — used both for the heading's subject and to decide
/// whether a row's "(via parent)" annotation is worth showing (omitted
/// when `parent` is itself one of the roots, since that's just "it").
/// Caps the list at `MAX_IMPACT_ROWS` with a summary row for the rest —
/// a glibc-scale reverse-dependency closure can otherwise dump hundreds
/// of rows into the dialog.
fn show_impact_dialog(
    parent: Option<&gtk::Window>,
    roots: &[String],
    affected: Vec<(String, String)>,
    cb: impl Fn(bool) + 'static,
) {
    let n = affected.len();
    let cb: Rc<dyn Fn(bool)> = Rc::new(cb);

    let (dlg, outer) = modal_window("Other Packages Depend On This", parent, true, (420, -1), 10);

    let subject = match roots {
        [one] => one.clone(),
        many => format!("{} selected packages", many.len()),
    };
    let heading = gtk::Label::new(Some(&format!(
        "Removing {} may break {} other installed package{} that depend{} on it:",
        subject,
        n,
        if n == 1 { "" } else { "s" },
        if n == 1 { "s" } else { "" },
    )));
    heading.set_xalign(0.0);
    heading.set_wrap(true);
    outer.append(&heading);

    // Same list-box style as `deps_confirm`'s own list and the detail
    // pane's Dependencies list, not a wrapped comma-separated line.
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
    outer.append(&scroll);

    let (btn_box, cancel_btn) = cancel_button_row(4);
    let remove_btn = gtk::Button::with_label("Remove Anyway");
    remove_btn.add_css_class("destructive-action");
    btn_box.append(&remove_btn);
    outer.append(&btn_box);

    // Cancel is the safer default — both as the Enter target and the
    // initial focus (also sidesteps the selectable-list-row-grabs-
    // focus-on-open issue the other confirm dialogs had).
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
        remove_btn.connect_clicked(move |_| {
            cb(true);
            dlg.destroy();
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
            ("c", PkgState::Installed, PkgMark::Remove), // itself marked for removal
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
        // B depends on A; both A and B are in the removal selection, so
        // B removing "because of" A isn't an impact — it's intentional.
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
