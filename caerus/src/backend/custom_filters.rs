//! User-defined "custom filters" — named sets of patterns matched
//! against package *names*, each either hiding matches (Synaptic-style
//! exclude) or showing *only* matches (include-only, the inverse).
//! Patterns containing `*` are anchored globs (`lib*`, `*-devel`),
//! patterns without one match as substrings (`devel`), both
//! case-insensitive — deliberately the same matching feel as the
//! search box.
//!
//! Persistence follows `repo_names.rs`: a tiny hand-rolled
//! tab-separated file under `$XDG_CONFIG_HOME/caerus/`, saved on every
//! mutation. Format safety comes from *input rejection* rather than
//! escaping: `sanitize` refuses tabs and control characters, so names
//! and patterns containing `=`, spaces, `*`, or non-ASCII all
//! round-trip verbatim.

use crate::backend::package::FilterMode;
use std::path::PathBuf;

/// Whether a custom filter hides packages matching its patterns (the
/// original Synaptic-style behavior) or does the opposite: shows
/// *only* packages matching a pattern, hiding everything else
/// (including everything when there are no patterns yet).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterKind {
    Exclude,
    IncludeOnly,
}

/// What the package list is currently narrowed by: one of the seven
/// preset sidebar modes, or a user-defined custom filter. The custom
/// variant carries its patterns *by value*, resolved by the sidebar at
/// selection time — the list widget never needs access to the
/// persistence store, and a name going stale after a delete/rename is
/// structurally impossible (the sidebar simply re-emits).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActiveFilter {
    Preset(FilterMode),
    Custom {
        name: String,
        patterns: Vec<String>,
        kind: FilterKind,
    },
}

/// One named filter: the unit the editor dialog manipulates and the
/// sidebar shows one row for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomFilterDef {
    pub name: String,
    pub kind: FilterKind,
    pub patterns: Vec<String>,
}

fn state_file_path() -> Option<PathBuf> {
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(config_home.join("caerus").join("custom-filters.conf"))
}

/// The ordered set of saved filters (order = sidebar display order).
pub struct CustomFilters {
    filters: Vec<CustomFilterDef>,
}

impl CustomFilters {
    pub fn load() -> Self {
        let contents = state_file_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .unwrap_or_default();
        Self {
            filters: parse(&contents),
        }
    }

    fn save(&self) {
        let Some(path) = state_file_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&path, serialize(&self.filters));
    }

    pub fn list(&self) -> &[CustomFilterDef] {
        &self.filters
    }

    pub fn get(&self, name: &str) -> Option<&CustomFilterDef> {
        self.filters.iter().find(|f| f.name == name)
    }

    /// Adds an empty filter with this (sanitized) name. Returns false —
    /// and changes nothing — for invalid or duplicate names.
    pub fn add(&mut self, name: &str) -> bool {
        let Some(name) = sanitize(name) else {
            return false;
        };
        if self.get(&name).is_some() {
            return false;
        }
        self.filters.push(CustomFilterDef {
            name,
            kind: FilterKind::Exclude,
            patterns: Vec::new(),
        });
        self.save();
        true
    }

    /// Sets `name`'s filter kind (Exclude/IncludeOnly). False — and no
    /// change — for an unknown filter name.
    pub fn set_kind(&mut self, name: &str, kind: FilterKind) -> bool {
        let Some(f) = self.filters.iter_mut().find(|f| f.name == name) else {
            return false;
        };
        f.kind = kind;
        self.save();
        true
    }

    /// Renames `old` to the sanitized `new`. False if `old` doesn't
    /// exist, `new` is invalid, or `new` already names another filter.
    pub fn rename(&mut self, old: &str, new: &str) -> bool {
        let Some(new) = sanitize(new) else {
            return false;
        };
        if new != old && self.get(&new).is_some() {
            return false;
        }
        let Some(f) = self.filters.iter_mut().find(|f| f.name == old) else {
            return false;
        };
        f.name = new;
        self.save();
        true
    }

    pub fn remove(&mut self, name: &str) {
        self.filters.retain(|f| f.name != name);
        self.save();
    }

    /// Appends a (sanitized) pattern to `name`'s list. False — and no
    /// change — for invalid patterns, duplicates within the same
    /// filter, or an unknown filter name.
    pub fn add_pattern(&mut self, name: &str, pattern: &str) -> bool {
        let Some(pattern) = sanitize(pattern) else {
            return false;
        };
        let Some(f) = self.filters.iter_mut().find(|f| f.name == name) else {
            return false;
        };
        if f.patterns.contains(&pattern) {
            return false;
        }
        f.patterns.push(pattern);
        self.save();
        true
    }

    pub fn remove_pattern(&mut self, name: &str, pattern: &str) {
        if let Some(f) = self.filters.iter_mut().find(|f| f.name == name) {
            f.patterns.retain(|p| p != pattern);
            self.save();
        }
    }
}

/// Trims, then rejects empty strings and anything containing a control
/// character (which covers `\t` and `\n` — the two characters the file
/// format relies on never appearing in a field).
pub fn sanitize(s: &str) -> Option<String> {
    let trimmed = s.trim();
    if trimmed.is_empty() || trimmed.chars().any(char::is_control) {
        return None;
    }
    Some(trimmed.to_string())
}

/// One filter per line: `name \t E|I \t pattern \t pattern ...`, where
/// `E`/`I` is the Exclude/IncludeOnly marker. Malformed lines (empty
/// name) are skipped; on a duplicate name the first line wins. A
/// filter with no patterns is legal and round-trips as a line holding
/// just the name and marker.
///
/// Files written before the include-only mode existed have no marker
/// field — their second field (if any) is just the first pattern. That
/// shape is auto-detected (second field isn't literally `E`/`I`) and
/// loaded as `FilterKind::Exclude`, the only kind that used to exist.
pub fn parse(contents: &str) -> Vec<CustomFilterDef> {
    let mut filters: Vec<CustomFilterDef> = Vec::new();
    for line in contents.lines() {
        let mut fields = line.split('\t').peekable();
        let Some(name) = fields.next() else { continue };
        if name.is_empty() || filters.iter().any(|f| f.name == name) {
            continue;
        }
        let kind = match fields.peek() {
            Some(&"E") => {
                fields.next();
                FilterKind::Exclude
            }
            Some(&"I") => {
                fields.next();
                FilterKind::IncludeOnly
            }
            _ => FilterKind::Exclude,
        };
        filters.push(CustomFilterDef {
            name: name.to_string(),
            kind,
            patterns: fields
                .filter(|p| !p.is_empty())
                .map(str::to_string)
                .collect(),
        });
    }
    filters
}

pub fn serialize(filters: &[CustomFilterDef]) -> String {
    let mut out = String::new();
    for f in filters {
        out.push_str(&f.name);
        out.push('\t');
        out.push_str(match f.kind {
            FilterKind::Exclude => "E",
            FilterKind::IncludeOnly => "I",
        });
        for p in &f.patterns {
            out.push('\t');
            out.push_str(p);
        }
        out.push('\n');
    }
    out
}

/// True if this package should be hidden by the filter: for `Exclude`,
/// any pattern matching hides it (Synaptic-style); for `IncludeOnly`,
/// the *absence* of any matching pattern hides it (including when
/// there are no patterns at all — nothing is included yet). `patterns`
/// must already be lowercased (the package list lowercases once at
/// `set_filter` time rather than per row).
pub fn filter_hides(kind: FilterKind, lowercased_patterns: &[String], pkg_name: &str) -> bool {
    let any_match = !lowercased_patterns.is_empty() && {
        let name = pkg_name.to_lowercase();
        lowercased_patterns
            .iter()
            .any(|p| !p.is_empty() && matches_lowercased(p, &name))
    };
    match kind {
        FilterKind::Exclude => any_match,
        FilterKind::IncludeOnly => !any_match,
    }
}

fn matches_lowercased(pattern: &str, name: &str) -> bool {
    if pattern.contains('*') {
        glob_match(pattern, name)
    } else {
        name.contains(pattern)
    }
}

/// Iterative star-backtracking glob matcher: `*` matches any run of
/// characters (including none), everything else matches literally.
/// O(len(p)·len(t)) worst case, no recursion, no dependencies.
fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    // Position of the last `*` seen, and where in `text` its match
    // currently ends — bumped forward one char per backtrack.
    let (mut star, mut star_ti) = (None::<usize>, 0usize);

    while ti < t.len() {
        if pi < p.len() && (p[pi] == '*') {
            star = Some(pi);
            star_ti = ti;
            pi += 1;
        } else if pi < p.len() && p[pi] == t[ti] {
            pi += 1;
            ti += 1;
        } else if let Some(s) = star {
            // Mismatch after a star: let the star swallow one more
            // character of `text` and retry from just past it.
            pi = s + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }
    // Text consumed — the rest of the pattern must be all stars.
    p[pi..].iter().all(|&c| c == '*')
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lowercases both sides and applies the same empty-pattern-never-
    /// matches rule `filter_excludes` enforces via its own guard, so
    /// tests exercise `matches_lowercased`/`glob_match` (the logic
    /// `filter_excludes` actually runs in production) the same way a
    /// real caller would use it.
    fn matches(pattern: &str, pkg_name: &str) -> bool {
        if pattern.is_empty() {
            return false;
        }
        matches_lowercased(&pattern.to_lowercase(), &pkg_name.to_lowercase())
    }

    #[test]
    fn starless_patterns_match_as_substrings() {
        assert!(matches("devel", "gtk4-devel"));
        assert!(matches("devel", "develtool"));
        assert!(matches("devel", "devel"));
        assert!(!matches("devel", "gtk4"));
    }

    #[test]
    fn globs_are_anchored_over_the_whole_name() {
        assert!(matches("lib*", "libfoo"));
        assert!(matches("lib*", "lib"));
        assert!(!matches("lib*", "zlib")); // anchored: no leading run
        assert!(matches("*-devel", "gtk4-devel"));
        assert!(!matches("*-devel", "develtool"));
        assert!(!matches("*-devel", "gtk4-devel-doc"));
        assert!(matches("*ssl*", "openssl-devel"));
        assert!(matches("lib*ssl*", "libressl-devel"));
        assert!(!matches("lib*ssl*", "openssl"));
        assert!(matches("*", "anything"));
        assert!(matches("**", "anything"));
        assert!(matches("foo*", "foo")); // trailing star matches empty
    }

    #[test]
    fn matching_is_case_insensitive_both_directions() {
        assert!(matches("LIB*", "libfoo"));
        assert!(matches("lib*", "LIBFOO"));
        assert!(matches("DeVeL", "gtk4-Devel"));
    }

    #[test]
    fn glob_metacharacters_other_than_star_are_literal() {
        assert!(matches("foo?", "foo?"));
        assert!(!matches("foo?", "food"));
        assert!(matches("a.b*", "a.bc"));
        assert!(!matches("a.b*", "aXbc"));
        assert!(matches("[x]*", "[x]y"));
    }

    #[test]
    fn degenerate_patterns_never_panic() {
        assert!(!matches("", "anything"));
        assert!(!matches("longer-than-name*", "short"));
        assert!(matches("p\u{e4}ck*", "P\u{c4}CKAGE")); // unicode, case-folded
        assert!(!glob_match("a*b", ""));
        assert!(glob_match("*", ""));
        assert!(glob_match("", ""));
    }

    #[test]
    fn filter_hides_exclude_is_any_of_and_expects_lowercased_patterns() {
        let pats = vec!["lib*".to_string(), "devel".to_string()];
        assert!(filter_hides(FilterKind::Exclude, &pats, "libfoo"));
        assert!(filter_hides(FilterKind::Exclude, &pats, "gtk4-devel"));
        assert!(filter_hides(FilterKind::Exclude, &pats, "LIBFOO")); // normalized per call
        assert!(!filter_hides(FilterKind::Exclude, &pats, "vim"));
        assert!(!filter_hides(FilterKind::Exclude, &[], "anything"));
        assert!(!filter_hides(
            FilterKind::Exclude,
            &[String::new()],
            "anything"
        ));
    }

    #[test]
    fn filter_hides_include_only_is_the_inverse_of_exclude() {
        let pats = vec!["lib*".to_string(), "devel".to_string()];
        assert!(!filter_hides(FilterKind::IncludeOnly, &pats, "libfoo"));
        assert!(!filter_hides(FilterKind::IncludeOnly, &pats, "gtk4-devel"));
        assert!(!filter_hides(FilterKind::IncludeOnly, &pats, "LIBFOO"));
        assert!(filter_hides(FilterKind::IncludeOnly, &pats, "vim"));
        // No patterns yet: nothing qualifies as "included", so
        // everything is hidden — the opposite of Exclude's empty case.
        assert!(filter_hides(FilterKind::IncludeOnly, &[], "anything"));
        assert!(filter_hides(
            FilterKind::IncludeOnly,
            &[String::new()],
            "anything"
        ));
    }

    #[test]
    fn parse_serialize_roundtrip_preserves_awkward_field_content() {
        let filters = vec![
            CustomFilterDef {
                name: "No libs/devel".to_string(),
                kind: FilterKind::Exclude,
                patterns: vec!["lib*".to_string(), "*-devel".to_string()],
            },
            CustomFilterDef {
                name: "srv=prod (päck)".to_string(), // '=', spaces, unicode
                kind: FilterKind::IncludeOnly,
                patterns: vec!["nginx*".to_string()],
            },
            CustomFilterDef {
                name: "empty-for-now".to_string(),
                kind: FilterKind::Exclude,
                patterns: vec![],
            },
        ];
        assert_eq!(parse(&serialize(&filters)), filters);
    }

    #[test]
    fn parse_defaults_to_exclude_for_pre_include_only_files() {
        // Files written before the mode marker existed have no "E"/"I"
        // field — their second field is just the first pattern.
        let parsed = parse("legacy\tlib*\t*-devel\n");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].kind, FilterKind::Exclude);
        assert_eq!(
            parsed[0].patterns,
            vec!["lib*".to_string(), "*-devel".to_string()]
        );
    }

    #[test]
    fn parse_skips_malformed_lines_and_dedups_names() {
        let parsed = parse("good\tlib*\n\n\tpattern-with-no-name\ngood\tshadowed\nother\n");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].name, "good");
        assert_eq!(parsed[0].patterns, vec!["lib*".to_string()]); // first wins
        assert_eq!(parsed[1].name, "other");
        assert!(parsed[1].patterns.is_empty());
    }

    #[test]
    fn sanitize_rejects_separators_and_trims() {
        assert_eq!(sanitize("  lib*  "), Some("lib*".to_string()));
        assert_eq!(sanitize("No libs/devel"), Some("No libs/devel".to_string()));
        assert_eq!(sanitize(""), None);
        assert_eq!(sanitize("   "), None);
        assert_eq!(sanitize("a\tb"), None);
        assert_eq!(sanitize("a\nb"), None);
        assert_eq!(sanitize("a\u{1b}b"), None);
    }
}
