use crate::backend::package::{
    pkg_format_size, pkg_state_icon, Package, PackageExtraInfo, PackageObject, PkgMark, PkgState,
};
use crate::backend::package_store::PackageStore;
use crate::ui::deps_confirm;
use crate::ui::dialog_util::{count_pill, set_count};
use crate::ui::remove_confirm;
use gio::prelude::*;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

const MAX_FILES_SHOWN: usize = 300;

enum KvValue {
    Text(String),
    Link(String),
}

type MarkChangedCbs = RefCell<Vec<Box<dyn Fn()>>>;
type HoldRequestedCbs = RefCell<Vec<Box<dyn Fn(String, bool)>>>;
type ActionRequestedCbs = RefCell<Vec<Box<dyn Fn(String)>>>;

struct Inner {
    widget: gtk::Box,
    store: PackageStore,
    current_pkgname: RefCell<Option<String>>,

    name: gtk::Label,
    version: gtk::Label,
    state_chip: gtk::Label,
    update_chip: gtk::Label,
    tags_box: gtk::Box,
    desc: gtk::Label,
    btn_install: gtk::Button,
    btn_upgrade: gtk::Button,
    btn_remove: gtk::Button,
    btn_purge: gtk::Button,
    btn_unmark: gtk::Button,

    hold_overlay: gtk::Overlay,
    btn_hold: gtk::Button,
    hold_dot: gtk::Box,
    repolock_overlay: gtk::Overlay,
    btn_repolock: gtk::Button,
    repolock_dot: gtk::Box,
    automark_overlay: gtk::Overlay,
    btn_automark: gtk::Button,
    automark_dot: gtk::Box,
    btn_reinstall: gtk::Button,
    btn_reconfigure: gtk::Button,
    btn_download: gtk::Button,

    size_install_card: gtk::Box,
    size_install_list: gtk::Box,
    source_card: gtk::Box,
    source_list: gtk::Box,
    deps_card: gtk::Box,
    deps_pill: gtk::Label,
    deps_list: gtk::ListBox,
    deps_placeholder: gtk::Label,
    rdeps_card: gtk::Box,
    rdeps_pill: gtk::Label,
    rdeps_list: gtk::ListBox,
    rdeps_placeholder: gtk::Label,
    provides_card: gtk::Box,
    provides_body: gtk::Box,
    relation_rows: RefCell<Vec<gtk::Box>>,
    files_card: gtk::Box,
    files_pill: gtk::Label,
    files_expander: gtk::Expander,
    files_list: gtk::ListBox,

    content_stack: gtk::Stack,

    install_size: Cell<u64>,
    download_size: Cell<u64>,
    current_maintainer: RefCell<String>,
    current_automatic: Cell<Option<bool>>,

    on_mark_changed: MarkChangedCbs,
    on_hold_requested: HoldRequestedCbs,
    on_reinstall_requested: ActionRequestedCbs,
    on_reconfigure_requested: ActionRequestedCbs,
    on_download_requested: ActionRequestedCbs,
    on_repolock_requested: HoldRequestedCbs,
    on_automatic_requested: HoldRequestedCbs,
    on_jump_to_package: ActionRequestedCbs,
}

#[derive(Clone)]
pub struct DetailPane {
    inner: Rc<Inner>,
}

fn pkg_state_text_class(state: PkgState) -> (&'static str, Option<&'static str>) {
    match state {
        PkgState::NotInstalled => ("Not installed", None),
        PkgState::Installed => ("Installed", Some("chip-ok")),
        PkgState::Upgradable => ("Upgrade available", Some("chip-warn")),
        PkgState::OnHold => ("On hold", Some("chip-warn")),
        PkgState::Broken => ("Broken", Some("chip-err")),
    }
}

fn chip(text: &str, extra_class: Option<&str>) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.add_css_class("chip");
    if let Some(class) = extra_class {
        l.add_css_class(class);
    }
    l.set_valign(gtk::Align::Center);
    l
}

fn card_simple(title: &str) -> gtk::Box {
    let card = card_simple_no_header();
    let header = gtk::Label::new(Some(title));
    header.set_xalign(0.0);
    header.add_css_class("card-header");
    header.set_margin_bottom(8);
    card.append(&header);
    card
}

fn card_with_pill(title: &str) -> (gtk::Box, gtk::Label) {
    let card = card_simple_no_header();
    let header_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    header_row.set_margin_bottom(8);
    let header = gtk::Label::new(Some(title));
    header.set_xalign(0.0);
    header.add_css_class("card-header");
    header.set_hexpand(true);
    let pill = count_pill();
    header_row.append(&header);
    header_row.append(&pill);
    card.append(&header_row);
    (card, pill)
}

fn card_simple_no_header() -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    card.add_css_class("card");
    card.set_hexpand(true);
    card.set_halign(gtk::Align::Fill);
    card.set_valign(gtk::Align::Start);
    card.set_size_request(260, -1);
    card
}

fn set_card_visible(card: &gtk::Box, visible: bool) {
    card.set_visible(visible);
}

fn icon_button(icon_name: &str) -> gtk::Button {
    let img = gtk::Image::from_icon_name(icon_name);
    img.set_pixel_size(16);
    let btn = gtk::Button::new();
    btn.set_child(Some(&img));
    btn.add_css_class("icon-btn");
    btn
}

fn icon_toggle(icon_name: &str) -> (gtk::Overlay, gtk::Button, gtk::Box) {
    let btn = icon_button(icon_name);
    let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    dot.add_css_class("state-dot");
    dot.add_css_class("off");
    dot.set_halign(gtk::Align::End);
    dot.set_valign(gtk::Align::Start);
    dot.set_can_target(false);
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&btn));
    overlay.add_overlay(&dot);
    (overlay, btn, dot)
}

fn set_dot_state(dot: &gtk::Box, on: bool) {
    dot.remove_css_class(if on { "off" } else { "on" });
    dot.add_css_class(if on { "on" } else { "off" });
}

fn build_kv_row(key: &str, value: KvValue) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let key_label = gtk::Label::new(Some(key));
    key_label.set_width_chars(12);
    key_label.set_xalign(0.0);
    key_label.add_css_class("dim-label");
    row.append(&key_label);
    match value {
        KvValue::Text(text) => {
            let val = gtk::Label::new(Some(&text));
            val.set_xalign(0.0);
            val.set_selectable(true);
            val.set_wrap(true);
            val.set_hexpand(true);
            row.append(&val);
        }
        KvValue::Link(url) => {
            let link = gtk::LinkButton::new(&url);
            link.add_css_class("flat");
            link.add_css_class("inline-link");
            link.set_halign(gtk::Align::Start);
            link.set_hexpand(true);
            row.append(&link);
        }
    }
    row
}

fn rebuild_kv_card(card: &gtk::Box, list_box: &gtk::Box, rows: Vec<(&str, KvValue)>) {
    clear_box_children(list_box);
    let visible = !rows.is_empty();
    for (key, value) in rows {
        list_box.append(&build_kv_row(key, value));
    }
    set_card_visible(card, visible);
}

fn rebuild_size_install(inner: &Inner, extra: Option<&PackageExtraInfo>) {
    let mut rows = Vec::new();
    if inner.install_size.get() > 0 {
        rows.push((
            "Installed size",
            KvValue::Text(pkg_format_size(inner.install_size.get())),
        ));
    }
    if inner.download_size.get() > 0 {
        rows.push((
            "Download size",
            KvValue::Text(pkg_format_size(inner.download_size.get())),
        ));
    }
    if let Some(date) = extra
        .and_then(|e| e.install_date.as_deref())
        .filter(|d| !d.is_empty())
    {
        rows.push(("Installed on", KvValue::Text(date.to_string())));
    }
    if let Some(e) = extra.filter(|e| e.has_automatic_install) {
        rows.push((
            "Auto-installed",
            KvValue::Text(if e.automatic_install { "Yes" } else { "No" }.into()),
        ));
    }
    rebuild_kv_card(&inner.size_install_card, &inner.size_install_list, rows);
}

fn rebuild_source(inner: &Inner, extra: Option<&PackageExtraInfo>) {
    let mut rows = Vec::new();

    if let Some(url) = extra
        .and_then(|e| e.repository.as_deref())
        .filter(|r| !r.is_empty())
    {
        let repo_names = crate::backend::repo_names::RepoNames::load();
        let display = repo_names.get(url).map_or_else(
            || crate::backend::repo_names::display_repo(url).to_string(),
            str::to_string,
        );
        rows.push(("Repository", KvValue::Text(display)));
    }

    if let Some(license) = extra
        .and_then(|e| e.license.as_deref())
        .filter(|l| !l.is_empty())
    {
        rows.push(("License", KvValue::Text(license.to_string())));
    }

    let maintainer = inner.current_maintainer.borrow();
    if !maintainer.is_empty() {
        rows.push(("Maintainer", KvValue::Text(maintainer.clone())));
    }
    drop(maintainer);

    if let Some(url) = extra
        .and_then(|e| e.homepage.as_deref())
        .filter(|u| !u.is_empty())
    {
        rows.push(("Homepage", KvValue::Link(url.to_string())));
    }

    rebuild_kv_card(&inner.source_card, &inner.source_list, rows);
}

impl DetailPane {
    pub fn new(store: PackageStore) -> Self {
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 0);
        widget.set_height_request(220);
        widget.set_margin_start(12);
        widget.set_margin_end(12);
        widget.set_margin_top(10);
        widget.set_margin_bottom(10);

        let header_card = gtk::Box::new(gtk::Orientation::Vertical, 0);
        header_card.add_css_class("card");
        header_card.set_hexpand(true);
        header_card.set_halign(gtk::Align::Fill);
        header_card.set_valign(gtk::Align::Start);
        header_card.set_size_request(260, -1);

        let pkg_head = gtk::Box::new(gtk::Orientation::Horizontal, 14);
        let icon_frame = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        icon_frame.add_css_class("pkg-icon");
        icon_frame.set_halign(gtk::Align::Start);
        icon_frame.set_valign(gtk::Align::Start);
        let icon_image = gtk::Image::from_icon_name("package-x-generic-symbolic");
        icon_image.set_pixel_size(28);
        icon_frame.append(&icon_image);
        pkg_head.append(&icon_frame);

        let title_col = gtk::Box::new(gtk::Orientation::Vertical, 4);
        title_col.set_hexpand(true);
        title_col.set_halign(gtk::Align::Fill);

        let title_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let name = gtk::Label::new(None);
        name.set_xalign(0.0);
        name.set_selectable(true);
        name.add_css_class("detail-name");
        title_row.append(&name);

        let version = gtk::Label::new(None);
        version.set_xalign(0.0);
        version.set_selectable(true);
        version.add_css_class("dim-label");
        version.set_valign(gtk::Align::Baseline);
        title_row.append(&version);

        let state_chip = chip("", None);
        title_row.append(&state_chip);
        let update_chip = chip("", None);
        update_chip.set_visible(false);
        title_row.append(&update_chip);
        title_col.append(&title_row);

        let tags_box = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        tags_box.set_margin_top(2);
        title_col.append(&tags_box);

        let desc = gtk::Label::new(None);
        desc.set_xalign(0.0);
        desc.set_wrap(true);
        desc.set_wrap_mode(gtk::pango::WrapMode::Word);
        desc.set_selectable(true);
        desc.add_css_class("dim-label");
        desc.set_margin_top(4);
        desc.set_hexpand(true);
        desc.set_halign(gtk::Align::Fill);
        title_col.append(&desc);

        pkg_head.append(&title_col);
        header_card.append(&pkg_head);

        let action_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        action_row.set_margin_top(12);
        let btn_install = gtk::Button::with_label("Install");
        btn_install.set_visible(false);
        btn_install.add_css_class("suggested-action");
        let btn_upgrade = gtk::Button::with_label("Upgrade");
        btn_upgrade.set_visible(false);
        btn_upgrade.add_css_class("suggested-action");
        let btn_remove = gtk::Button::with_label("Remove");
        btn_remove.set_visible(false);
        btn_remove.add_css_class("destructive-action");
        let btn_purge = gtk::Button::with_label("Purge");
        btn_purge.set_visible(false);
        btn_purge.add_css_class("destructive-action");
        btn_purge.set_tooltip_text(Some(
            "Remove this package and any dependencies left orphaned by doing so",
        ));
        let btn_unmark = gtk::Button::with_label("Unmark");
        btn_unmark.set_visible(false);
        action_row.append(&btn_install);
        action_row.append(&btn_upgrade);
        action_row.append(&btn_remove);
        action_row.append(&btn_purge);
        action_row.append(&btn_unmark);
        header_card.append(&action_row);

        let secondary_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        secondary_row.add_css_class("actions-secondary");
        secondary_row.set_margin_top(10);

        let (hold_overlay, btn_hold, hold_dot) = icon_toggle("hold-symbolic");
        hold_overlay.set_visible(false);
        let (repolock_overlay, btn_repolock, repolock_dot) = icon_toggle("repo-lock-symbolic");
        repolock_overlay.set_visible(false);
        let (automark_overlay, btn_automark, automark_dot) = icon_toggle("mark-manual-symbolic");
        automark_overlay.set_visible(false);
        let btn_reinstall = icon_button("reinstall-symbolic");
        btn_reinstall.set_visible(false);
        btn_reinstall.set_tooltip_text(Some(
            "Force re-installation, overwriting any locally-modified files",
        ));
        let btn_reconfigure = icon_button("applications-utilities-symbolic");
        btn_reconfigure.set_visible(false);
        btn_reconfigure.set_tooltip_text(Some("Re-run this package's post-install configuration"));
        let btn_download = icon_button("download-only-symbolic");
        btn_download.set_visible(false);
        btn_download.set_tooltip_text(Some(
            "Fetch and verify the package file without installing it",
        ));

        secondary_row.append(&hold_overlay);
        secondary_row.append(&repolock_overlay);
        secondary_row.append(&automark_overlay);
        secondary_row.append(&btn_reinstall);
        secondary_row.append(&btn_reconfigure);
        secondary_row.append(&btn_download);
        header_card.append(&secondary_row);

        let size_install_card = card_simple("Size & Installation");
        let size_install_list = gtk::Box::new(gtk::Orientation::Vertical, 8);
        size_install_card.append(&size_install_list);

        let source_card = card_simple("Source");
        let source_list = gtk::Box::new(gtk::Orientation::Vertical, 8);
        source_card.append(&source_list);

        let (deps_card, deps_pill) = card_with_pill("Dependencies");
        let deps_scroll = gtk::ScrolledWindow::new();
        deps_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        deps_scroll.set_max_content_height(180);
        deps_scroll.set_propagate_natural_height(true);
        deps_scroll.set_hexpand(true);
        deps_scroll.set_halign(gtk::Align::Fill);
        let deps_list = gtk::ListBox::new();
        deps_list.set_selection_mode(gtk::SelectionMode::Single);
        deps_list.set_hexpand(true);
        deps_list.set_halign(gtk::Align::Fill);
        let deps_ph = gtk::Label::new(Some("Select a package"));
        deps_ph.add_css_class("dim-label");
        deps_ph.set_margin_top(12);
        deps_list.set_placeholder(Some(&deps_ph));
        deps_scroll.set_child(Some(&deps_list));
        deps_card.append(&deps_scroll);

        let (rdeps_card, rdeps_pill) = card_with_pill("Reverse Dependencies");
        let rdeps_scroll = gtk::ScrolledWindow::new();
        rdeps_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        rdeps_scroll.set_max_content_height(180);
        rdeps_scroll.set_propagate_natural_height(true);
        rdeps_scroll.set_hexpand(true);
        rdeps_scroll.set_halign(gtk::Align::Fill);
        let rdeps_list = gtk::ListBox::new();
        rdeps_list.set_selection_mode(gtk::SelectionMode::Single);
        rdeps_list.set_hexpand(true);
        rdeps_list.set_halign(gtk::Align::Fill);
        let rdeps_ph = gtk::Label::new(Some("Select a package"));
        rdeps_ph.add_css_class("dim-label");
        rdeps_ph.set_margin_top(12);
        rdeps_list.set_placeholder(Some(&rdeps_ph));
        rdeps_scroll.set_child(Some(&rdeps_list));
        rdeps_card.append(&rdeps_scroll);

        let provides_card = card_simple("Provides & Requires");
        let provides_scroll = gtk::ScrolledWindow::new();
        provides_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        provides_scroll.set_max_content_height(220);
        provides_scroll.set_propagate_natural_height(true);
        provides_scroll.set_hexpand(true);
        provides_scroll.set_halign(gtk::Align::Fill);
        let provides_body = gtk::Box::new(gtk::Orientation::Vertical, 12);
        provides_scroll.set_child(Some(&provides_body));
        provides_card.append(&provides_scroll);

        let files_card = card_simple_no_header();
        let files_pill = count_pill();
        let files_label_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let files_title = gtk::Label::new(Some("Files"));
        files_title.set_xalign(0.0);
        files_title.add_css_class("card-header");
        files_title.set_hexpand(true);
        files_label_box.append(&files_title);
        files_label_box.append(&files_pill);
        let files_expander = gtk::Expander::new(None);
        files_expander.set_label_widget(Some(&files_label_box));
        let files_list = gtk::ListBox::new();
        files_list.set_selection_mode(gtk::SelectionMode::None);
        let files_scroll = gtk::ScrolledWindow::new();
        files_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        files_scroll.set_max_content_height(260);
        files_scroll.set_propagate_natural_height(true);
        files_scroll.set_hexpand(true);
        files_scroll.set_halign(gtk::Align::Fill);
        files_scroll.set_child(Some(&files_list));
        files_expander.set_child(Some(&files_scroll));
        files_expander.set_margin_top(4);
        files_card.append(&files_expander);

        let cards_col = gtk::Box::new(gtk::Orientation::Vertical, 12);
        cards_col.set_hexpand(true);
        cards_col.set_halign(gtk::Align::Fill);

        let card_order: [&gtk::Box; 7] = [
            &header_card,
            &size_install_card,
            &source_card,
            &deps_card,
            &rdeps_card,
            &provides_card,
            &files_card,
        ];
        for card in card_order {
            cards_col.append(card);
        }

        let content_scroll = gtk::ScrolledWindow::new();
        content_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        content_scroll.set_vexpand(true);
        content_scroll.set_hexpand(true);
        content_scroll.set_halign(gtk::Align::Fill);
        content_scroll.set_child(Some(&cards_col));

        let empty_page = gtk::Box::new(gtk::Orientation::Vertical, 10);
        empty_page.set_valign(gtk::Align::Center);
        empty_page.set_halign(gtk::Align::Center);
        empty_page.set_vexpand(true);
        empty_page.set_hexpand(true);
        let empty_icon = gtk::Image::from_icon_name("package-x-generic-symbolic");
        empty_icon.set_pixel_size(40);
        empty_icon.add_css_class("dim-label");
        let empty_title = gtk::Label::new(Some("Select a package to view details"));
        empty_title.add_css_class("dim-label");
        let empty_sub = gtk::Label::new(Some(
            "Choose a package from the list to see its description, size, \
             dependencies and available actions here.",
        ));
        empty_sub.add_css_class("dim-label");
        empty_sub.set_wrap(true);
        empty_sub.set_justify(gtk::Justification::Center);
        empty_sub.set_max_width_chars(40);
        empty_page.append(&empty_icon);
        empty_page.append(&empty_title);
        empty_page.append(&empty_sub);

        let content_stack = gtk::Stack::new();
        content_stack.set_vexpand(true);
        content_stack.add_named(&empty_page, Some("empty"));
        content_stack.add_named(&content_scroll, Some("content"));
        content_stack.set_visible_child_name("empty");
        widget.append(&content_stack);

        let inner = Rc::new(Inner {
            widget,
            store,
            current_pkgname: RefCell::new(None),
            name,
            version,
            state_chip,
            update_chip,
            tags_box,
            desc,
            btn_install,
            btn_upgrade,
            btn_remove,
            btn_purge,
            btn_unmark,
            hold_overlay,
            btn_hold,
            hold_dot,
            repolock_overlay,
            btn_repolock,
            repolock_dot,
            automark_overlay,
            btn_automark,
            automark_dot,
            btn_reinstall,
            btn_reconfigure,
            btn_download,
            size_install_card,
            size_install_list,
            source_card,
            source_list,
            deps_card,
            deps_pill,
            deps_list,
            deps_placeholder: deps_ph,
            rdeps_card,
            rdeps_pill,
            rdeps_list,
            rdeps_placeholder: rdeps_ph,
            provides_card,
            provides_body,
            relation_rows: RefCell::new(Vec::new()),
            files_card,
            files_pill,
            files_expander,
            files_list,
            content_stack,
            install_size: Cell::new(0),
            download_size: Cell::new(0),
            current_maintainer: RefCell::new(String::new()),
            current_automatic: Cell::new(None),
            on_mark_changed: RefCell::new(Vec::new()),
            on_hold_requested: RefCell::new(Vec::new()),
            on_reinstall_requested: RefCell::new(Vec::new()),
            on_reconfigure_requested: RefCell::new(Vec::new()),
            on_download_requested: RefCell::new(Vec::new()),
            on_repolock_requested: RefCell::new(Vec::new()),
            on_automatic_requested: RefCell::new(Vec::new()),
            on_jump_to_package: RefCell::new(Vec::new()),
        });

        wire_buttons(&inner);
        wire_files_expander(&inner);
        wire_dependency_lists(&inner);

        Self { inner }
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.inner.widget
    }

    pub fn connect_mark_changed(&self, f: impl Fn() + 'static) {
        self.inner.on_mark_changed.borrow_mut().push(Box::new(f));
    }

    pub fn connect_hold_requested(&self, f: impl Fn(String, bool) + 'static) {
        self.inner.on_hold_requested.borrow_mut().push(Box::new(f));
    }

    pub fn connect_reinstall_requested(&self, f: impl Fn(String) + 'static) {
        self.inner
            .on_reinstall_requested
            .borrow_mut()
            .push(Box::new(f));
    }

    pub fn connect_reconfigure_requested(&self, f: impl Fn(String) + 'static) {
        self.inner
            .on_reconfigure_requested
            .borrow_mut()
            .push(Box::new(f));
    }

    pub fn connect_download_requested(&self, f: impl Fn(String) + 'static) {
        self.inner
            .on_download_requested
            .borrow_mut()
            .push(Box::new(f));
    }

    pub fn connect_repolock_requested(&self, f: impl Fn(String, bool) + 'static) {
        self.inner
            .on_repolock_requested
            .borrow_mut()
            .push(Box::new(f));
    }

    pub fn connect_automatic_requested(&self, f: impl Fn(String, bool) + 'static) {
        self.inner
            .on_automatic_requested
            .borrow_mut()
            .push(Box::new(f));
    }

    pub fn connect_jump_to_package(&self, f: impl Fn(String) + 'static) {
        self.inner.on_jump_to_package.borrow_mut().push(Box::new(f));
    }

    pub fn show_package(&self, pkg: Option<&Package>) {
        show_package_impl(&self.inner, pkg);
    }
}

fn wire_buttons(inner: &Rc<Inner>) {
    {
        let btn_install = inner.btn_install.clone();
        let inner = inner.clone();
        btn_install.connect_clicked(move |_| {
            let Some(name) = inner.current_pkgname.borrow().clone() else {
                return;
            };
            let root = inner.widget.root().and_downcast::<gtk::Window>();
            let store = inner.store.clone();
            let inner2 = inner.clone();
            let name2 = name.clone();
            deps_confirm::confirm_install_deps(root.as_ref(), &store, &name, move |proceed| {
                if proceed {
                    inner2.store.set_mark(&name2, PkgMark::Install);
                    update_action_buttons(&inner2, None);
                    for f in inner2.on_mark_changed.borrow().iter() {
                        f();
                    }
                }
            });
        });
    }
    wire_simple_mark_button(inner, &inner.btn_upgrade, PkgMark::Upgrade);
    wire_remove_button(inner, &inner.btn_remove, PkgMark::Remove);
    wire_remove_button(inner, &inner.btn_purge, PkgMark::Purge);
    wire_simple_mark_button(inner, &inner.btn_unmark, PkgMark::None);

    {
        let btn = inner.btn_hold.clone();
        let inner = inner.clone();
        btn.connect_clicked(move |_| {
            let Some(pkg) = lookup_current_pkg(&inner) else {
                return;
            };
            let want_hold = pkg.state != PkgState::OnHold;
            for f in inner.on_hold_requested.borrow().iter() {
                f(pkg.name.clone(), want_hold);
            }
        });
    }
    {
        let btn = inner.btn_repolock.clone();
        let inner = inner.clone();
        btn.connect_clicked(move |_| {
            let Some(pkg) = lookup_current_pkg(&inner) else {
                return;
            };
            let want_locked = !pkg.is_repolocked;
            for f in inner.on_repolock_requested.borrow().iter() {
                f(pkg.name.clone(), want_locked);
            }
        });
    }
    {
        let btn = inner.btn_automark.clone();
        let inner = inner.clone();
        btn.connect_clicked(move |_| {
            let Some(name) = inner.current_pkgname.borrow().clone() else {
                return;
            };
            let want_automatic = !inner.current_automatic.get().unwrap_or(false);
            for f in inner.on_automatic_requested.borrow().iter() {
                f(name.clone(), want_automatic);
            }
        });
    }
    wire_action_button(inner, &inner.btn_reinstall, |i| &i.on_reinstall_requested);
    wire_action_button(inner, &inner.btn_reconfigure, |i| {
        &i.on_reconfigure_requested
    });
    wire_action_button(inner, &inner.btn_download, |i| &i.on_download_requested);
}

fn lookup_current_pkg(inner: &Inner) -> Option<Package> {
    let name = inner.current_pkgname.borrow().clone()?;
    let list = inner.store.list();
    let n = list.n_items();
    for i in 0..n {
        if let Some(obj) = crate::backend::package_store::package_obj_at(&list, i) {
            if obj.name() == name {
                return Some(obj.pkg().clone());
            }
        }
    }
    None
}

fn wire_action_button(
    inner: &Rc<Inner>,
    btn: &gtk::Button,
    get_cbs: impl Fn(&Inner) -> &ActionRequestedCbs + 'static,
) {
    let btn = btn.clone();
    let inner = inner.clone();
    btn.connect_clicked(move |_| {
        let Some(name) = inner.current_pkgname.borrow().clone() else {
            return;
        };
        for f in get_cbs(&inner).borrow().iter() {
            f(name.clone());
        }
    });
}

fn wire_simple_mark_button(inner: &Rc<Inner>, btn: &gtk::Button, mark: PkgMark) {
    let btn = btn.clone();
    let inner = inner.clone();
    btn.connect_clicked(move |_| {
        let Some(name) = inner.current_pkgname.borrow().clone() else {
            return;
        };
        inner.store.set_mark(&name, mark);
        update_action_buttons(&inner, None);
        for f in inner.on_mark_changed.borrow().iter() {
            f();
        }
    });
}

fn wire_remove_button(inner: &Rc<Inner>, btn: &gtk::Button, mark: PkgMark) {
    let btn = btn.clone();
    let inner = inner.clone();
    btn.connect_clicked(move |_| {
        let Some(name) = inner.current_pkgname.borrow().clone() else {
            return;
        };
        let root = inner.widget.root().and_downcast::<gtk::Window>();
        let store = inner.store.clone();
        let inner2 = inner.clone();
        let name2 = name.clone();
        remove_confirm::confirm_remove_impact(root.as_ref(), &store, &name, move |proceed| {
            if proceed {
                inner2.store.set_mark(&name2, mark);
                update_action_buttons(&inner2, None);
                for f in inner2.on_mark_changed.borrow().iter() {
                    f();
                }
            }
        });
    });
}

fn update_action_buttons(inner: &Rc<Inner>, pkg: Option<&Package>) {
    let owned;
    let pkg: Option<&Package> = if pkg.is_some() {
        pkg
    } else {
        owned = lookup_current_pkg(inner);
        owned.as_ref()
    };

    let Some(pkg) = pkg else {
        inner.btn_install.set_visible(false);
        inner.btn_remove.set_visible(false);
        inner.btn_purge.set_visible(false);
        inner.btn_upgrade.set_visible(false);
        inner.btn_unmark.set_visible(false);
        inner.hold_overlay.set_visible(false);
        inner.repolock_overlay.set_visible(false);
        inner.automark_overlay.set_visible(false);
        inner.btn_reinstall.set_visible(false);
        inner.btn_reconfigure.set_visible(false);
        inner.btn_download.set_visible(false);
        return;
    };

    let installed = pkg.state != PkgState::NotInstalled;

    inner.hold_overlay.set_visible(installed);
    if installed {
        let is_held = pkg.state == PkgState::OnHold;
        set_dot_state(&inner.hold_dot, is_held);
        inner.btn_hold.set_tooltip_text(Some(if is_held {
            "Release Hold — allow upgrades again"
        } else {
            "Hold — pin this package's version, excluding it from upgrades"
        }));
    }

    inner.repolock_overlay.set_visible(installed);
    if installed {
        set_dot_state(&inner.repolock_dot, pkg.is_repolocked);
        inner
            .btn_repolock
            .set_tooltip_text(Some(if pkg.is_repolocked {
                "Release Repo-Lock — allow upgrades from any enabled repository again"
            } else {
                "Repo-Lock — only ever upgrade this package from the repository it's \
                 currently installed from"
            }));
    }

    inner.btn_reinstall.set_visible(installed);
    inner.btn_reconfigure.set_visible(installed);
    inner.btn_download.set_visible(!installed);

    if pkg.mark != PkgMark::None {
        inner.btn_install.set_visible(false);
        inner.btn_remove.set_visible(false);
        inner.btn_purge.set_visible(false);
        inner.btn_upgrade.set_visible(false);
        inner.btn_unmark.set_visible(true);
        return;
    }
    inner.btn_unmark.set_visible(false);

    if pkg.state == PkgState::NotInstalled {
        inner.btn_install.set_visible(true);
        inner.btn_remove.set_visible(false);
        inner.btn_purge.set_visible(false);
        inner.btn_upgrade.set_visible(false);
    } else {
        inner.btn_install.set_visible(false);
        inner
            .btn_upgrade
            .set_visible(pkg.state == PkgState::Upgradable);
        inner.btn_remove.set_visible(true);
        inner.btn_remove.set_sensitive(!pkg.essential);
        inner.btn_remove.set_tooltip_text(if pkg.essential {
            Some("Essential package — removal disabled")
        } else {
            None
        });
        inner.btn_purge.set_visible(true);
        inner.btn_purge.set_sensitive(!pkg.essential);
        inner.btn_purge.set_tooltip_text(if pkg.essential {
            Some("Essential package — purge disabled")
        } else {
            None
        });
    }
}

fn clear_list(lb: &gtk::ListBox) {
    while let Some(c) = lb.first_child() {
        lb.remove(&c);
    }
}

fn populate_dep_list(
    lb: &gtk::ListBox,
    items: Option<Vec<String>>,
    snapshot: &HashMap<String, PackageObject>,
) {
    clear_list(lb);
    let Some(items) = items else { return };
    for name in items {
        lb.append(&dependency_row(&name, snapshot));
    }
}

fn dependency_row(name: &str, snapshot: &HashMap<String, PackageObject>) -> gtk::ListBoxRow {
    let label = gtk::Label::new(Some(name));
    label.set_xalign(0.0);
    label.set_margin_start(8);
    label.set_margin_top(4);
    label.set_margin_bottom(4);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);

    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&label));

    if let Some(pkg) = snapshot.get(name).map(|o| o.pkg().clone()) {
        if pkg.state != PkgState::NotInstalled {
            label.add_css_class("pkg-installed");
        } else {
            label.add_css_class("dim-label");
        }
        unsafe {
            row.set_data("dep-pkgname", name.to_string());
        }
        wire_hover_tooltip(&row, pkg);
    } else {
        label.add_css_class("dim-label");
    }

    row
}

fn wire_hover_tooltip(row: &gtk::ListBoxRow, pkg: Package) {
    row.set_has_tooltip(true);
    row.connect_query_tooltip(move |_, _x, _y, _keyboard_mode, tooltip| {
        tooltip.set_custom(Some(&build_hover_content(&pkg)));
        true
    });
}

fn build_hover_content(pkg: &Package) -> gtk::Box {
    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 4);
    vbox.set_margin_start(10);
    vbox.set_margin_end(10);
    vbox.set_margin_top(8);
    vbox.set_margin_bottom(8);

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    if let Some(icon) = pkg_state_icon(pkg.state, pkg.mark) {
        header.append(&gtk::Image::from_icon_name(icon));
    }
    let name = gtk::Label::new(Some(&pkg.name));
    name.add_css_class("detail-name");
    header.append(&name);
    let (state_text, state_class) = pkg_state_text_class(pkg.state);
    header.append(&chip(state_text, state_class));
    vbox.append(&header);

    if let Some(v) = pkg
        .version_installed
        .as_ref()
        .or(pkg.version_available.as_ref())
    {
        let l = gtk::Label::new(Some(v));
        l.set_xalign(0.0);
        l.add_css_class("dim-label");
        vbox.append(&l);
    }
    if pkg.install_size > 0 {
        let l = gtk::Label::new(Some(&format!(
            "Size: {}",
            pkg_format_size(pkg.install_size)
        )));
        l.set_xalign(0.0);
        vbox.append(&l);
    }
    if let Some(repo) = &pkg.repository {
        let l = gtk::Label::new(Some(&format!("Source: {repo}")));
        l.set_xalign(0.0);
        vbox.append(&l);
    }

    vbox
}

fn wire_dependency_lists(inner: &Rc<Inner>) {
    for lb in [&inner.deps_list, &inner.rdeps_list] {
        let lb = lb.clone();
        let inner = inner.clone();
        lb.connect_row_activated(move |_, row| {
            let name = unsafe { row.data::<String>("dep-pkgname") };
            let Some(name) = name else { return };
            let name = unsafe { name.as_ref() }.clone();
            for f in inner.on_jump_to_package.borrow().iter() {
                f(name.clone());
            }
        });
    }
}

fn build_subgroup(title: &str, items: &[String], conflict: bool) -> gtk::Box {
    let col = gtk::Box::new(gtk::Orientation::Vertical, 4);

    let label_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let label = gtk::Label::new(Some(title));
    label.set_xalign(0.0);
    label.add_css_class("dim-label");
    label.set_hexpand(true);
    let count = gtk::Label::new(Some(&items.len().to_string()));
    count.add_css_class("dim-label");
    label_row.append(&label);
    label_row.append(&count);
    col.append(&label_row);

    let flow = gtk::FlowBox::new();
    flow.add_css_class("chip-flow");
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_row_spacing(6);
    flow.set_column_spacing(6);
    flow.set_max_children_per_line(64);
    for item in items {
        let tag = gtk::Label::new(Some(item));
        tag.add_css_class("plain-tag");
        if conflict {
            tag.add_css_class("plain-tag-conflict");
        }
        flow.insert(&tag, -1);
    }
    col.append(&flow);

    col
}

fn populate_provides_conflicts(inner: &Rc<Inner>, extra: Option<&PackageExtraInfo>) {
    for row in inner.relation_rows.borrow_mut().drain(..) {
        inner.provides_body.remove(&row);
    }
    let Some(extra) = extra else {
        set_card_visible(&inner.provides_card, false);
        return;
    };

    let fields: Vec<(&str, &[String], bool)> = [
        ("Provides", extra.provides.as_slice(), false),
        ("Requires", extra.shlib_requires.as_slice(), false),
        ("Exports", extra.shlib_provides.as_slice(), false),
        ("Conflicts", extra.conflicts.as_slice(), true),
        ("Replaces", extra.replaces.as_slice(), false),
    ]
    .into_iter()
    .filter(|(_, items, _)| !items.is_empty())
    .collect();

    if fields.is_empty() {
        set_card_visible(&inner.provides_card, false);
        return;
    }
    set_card_visible(&inner.provides_card, true);

    for (title, items, conflict) in fields {
        let sub = build_subgroup(title, items, conflict);
        inner.provides_body.append(&sub);
        inner.relation_rows.borrow_mut().push(sub);
    }
}

fn wire_files_expander(inner: &Rc<Inner>) {
    let files_expander = inner.files_expander.clone();
    let inner = inner.clone();
    files_expander.connect_expanded_notify(move |exp| {
        if !exp.is_expanded() {
            return;
        }
        let Some(name) = inner.current_pkgname.borrow().clone() else {
            return;
        };
        let inner2 = inner.clone();
        let name_for_call = name.clone();
        inner.store.get_files_async(&name_for_call, move |files| {
            if inner2.current_pkgname.borrow().as_deref() == Some(name.as_str()) {
                populate_files(&inner2, files);
            }
        });
    });
}

fn populate_files(inner: &Inner, files: Option<Vec<String>>) {
    let lb = &inner.files_list;
    while let Some(c) = lb.first_child() {
        lb.remove(&c);
    }
    let Some(mut files) = files else {
        set_count(&inner.files_pill, None);
        return;
    };
    set_count(&inner.files_pill, Some(files.len()));
    files.sort();
    let total = files.len();
    let shown = total.min(MAX_FILES_SHOWN);
    for f in &files[..shown] {
        let l = gtk::Label::new(Some(f));
        l.set_xalign(0.0);
        l.set_selectable(true);
        l.add_css_class("monospace");
        l.set_margin_start(8);
        l.set_margin_top(2);
        l.set_margin_bottom(2);
        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&l));
        lb.append(&row);
    }
    if total > shown {
        let l = gtk::Label::new(Some(&format!("\u{2026} and {} more", total - shown)));
        l.set_xalign(0.0);
        l.add_css_class("dim-label");
        l.set_margin_start(8);
        l.set_margin_top(4);
        l.set_margin_bottom(4);
        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&l));
        row.set_activatable(false);
        row.set_selectable(false);
        lb.append(&row);
    }
}

fn clear_box_children(b: &gtk::Box) {
    while let Some(c) = b.first_child() {
        b.remove(&c);
    }
}

fn set_header_chips(inner: &Inner, pkg: &Package) {
    for class in ["chip-ok", "chip-warn", "chip-err"] {
        inner.state_chip.remove_css_class(class);
        inner.update_chip.remove_css_class(class);
    }
    if pkg.state == PkgState::Upgradable {
        inner.state_chip.set_text("Installed");
        inner.state_chip.add_css_class("chip-ok");
        inner.state_chip.set_visible(true);
        let target = pkg.version_available.as_deref().unwrap_or("");
        inner.update_chip.set_text(&format!("Update to {target}"));
        inner.update_chip.add_css_class("chip-warn");
        inner.update_chip.set_visible(true);
    } else {
        let (text, class) = pkg_state_text_class(pkg.state);
        inner.state_chip.set_text(text);
        if let Some(class) = class {
            inner.state_chip.add_css_class(class);
        }
        inner.state_chip.set_visible(true);
        inner.update_chip.set_visible(false);
    }
}

fn show_package_impl(inner: &Rc<Inner>, pkg: Option<&Package>) {
    *inner.current_pkgname.borrow_mut() = pkg.map(|p| p.name.clone());

    inner.files_expander.set_expanded(false);
    populate_files(inner, None);

    let Some(pkg) = pkg else {
        inner.content_stack.set_visible_child_name("empty");
        update_action_buttons(inner, None);
        return;
    };
    inner.content_stack.set_visible_child_name("content");

    inner.name.set_text(&pkg.name);

    let ver = match (&pkg.version_installed, &pkg.version_available) {
        (Some(inst), Some(avail)) if inst != avail => Some(format!("{inst}  \u{2192}  {avail}")),
        (Some(inst), _) => Some(inst.clone()),
        (None, Some(avail)) => Some(avail.clone()),
        (None, None) => None,
    };
    inner.version.set_visible(ver.is_some());
    inner.version.set_text(ver.as_deref().unwrap_or(""));

    set_header_chips(inner, pkg);

    clear_box_children(&inner.tags_box);
    for tag in pkg
        .tags
        .split([',', ' '])
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        inner.tags_box.append(&chip(tag, None));
    }

    inner.desc.set_text(
        pkg.long_desc
            .as_deref()
            .filter(|s| !s.is_empty())
            .or(Some(pkg.short_desc.as_str()))
            .filter(|s| !s.is_empty())
            .unwrap_or("No description available."),
    );

    inner.install_size.set(pkg.install_size);
    inner.download_size.set(pkg.download_size);
    rebuild_size_install(inner, None);

    *inner.current_maintainer.borrow_mut() = pkg.maintainer.clone();
    rebuild_source(inner, None);
    inner.current_automatic.set(None);
    inner.automark_overlay.set_visible(false);
    populate_provides_conflicts(inner, None);

    set_card_visible(&inner.files_card, pkg.state != PkgState::NotInstalled);

    {
        let inner = inner.clone();
        let name = pkg.name.clone();
        inner
            .store
            .clone()
            .get_extra_info_async(&pkg.name, move |extra| {
                if inner.current_pkgname.borrow().as_deref() != Some(name.as_str()) {
                    return;
                }

                if let Some(extra) = &extra {
                    if extra.download_size > 0 {
                        inner.download_size.set(extra.download_size);
                    }
                }
                rebuild_size_install(&inner, extra.as_ref());
                rebuild_source(&inner, extra.as_ref());

                let auto_flag = extra.as_ref().filter(|e| e.has_automatic_install);
                inner.automark_overlay.set_visible(auto_flag.is_some());
                if let Some(flag) = auto_flag {
                    inner.current_automatic.set(Some(flag.automatic_install));
                    set_dot_state(&inner.automark_dot, flag.automatic_install);
                    inner
                        .btn_automark
                        .set_tooltip_text(Some(if flag.automatic_install {
                            "Mark as Manually Installed — won't be offered for orphan cleanup"
                        } else {
                            "Mark as Automatically Installed — eligible for orphan cleanup \
                             if nothing ends up needing it"
                        }));
                } else {
                    inner.current_automatic.set(None);
                }

                populate_provides_conflicts(&inner, extra.as_ref());
            });
    }

    set_card_visible(&inner.deps_card, true);
    set_card_visible(&inner.rdeps_card, true);
    inner.deps_placeholder.set_text("Loading\u{2026}");
    inner.rdeps_placeholder.set_text("Loading\u{2026}");
    set_count(&inner.deps_pill, None);
    set_count(&inner.rdeps_pill, None);
    clear_list(&inner.deps_list);
    clear_list(&inner.rdeps_list);
    {
        let inner2 = inner.clone();
        let name = pkg.name.clone();
        inner.store.get_deps_async(&pkg.name, move |deps| {
            if inner2.current_pkgname.borrow().as_deref() == Some(name.as_str()) {
                let count = deps.as_ref().map_or(0, Vec::len);
                set_card_visible(&inner2.deps_card, count > 0);
                set_count(&inner2.deps_pill, (count > 0).then_some(count));
                let snapshot = inner2.store.snapshot_objects();
                populate_dep_list(&inner2.deps_list, deps, &snapshot);
            }
        });
    }
    {
        let inner2 = inner.clone();
        let name = pkg.name.clone();
        inner.store.get_rdeps_async(&pkg.name, move |rdeps| {
            if inner2.current_pkgname.borrow().as_deref() == Some(name.as_str()) {
                let count = rdeps.as_ref().map_or(0, Vec::len);
                set_card_visible(&inner2.rdeps_card, count > 0);
                set_count(&inner2.rdeps_pill, (count > 0).then_some(count));
                let snapshot = inner2.store.snapshot_objects();
                populate_dep_list(&inner2.rdeps_list, rdeps, &snapshot);
            }
        });
    }

    update_action_buttons(inner, Some(pkg));
}
