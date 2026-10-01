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
