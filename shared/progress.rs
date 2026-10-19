//! Versioned, tab-delimited helper events. Raw tool output always stays LOG data.
//! Package observations describe a stage; only explicit success text or a
//! successful command establishes completion.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Command {
        verb: String,
        names: Vec<String>,
    },
    Phase(String),
    Package {
        name: String,
        version: String,
        stage: String,
    },
    Finished(bool),
}

impl Event {
    pub fn encode(&self) -> String {
        let fields = match self {
            Self::Command { verb, names } => vec!["command".into(), verb.clone(), names.join(" ")],
            Self::Phase(stage) => vec!["phase".into(), stage.clone()],
            Self::Package {
                name,
                version,
                stage,
            } => vec![
                "package".into(),
                name.clone(),
                version.clone(),
                stage.clone(),
            ],
            Self::Finished(ok) => vec!["finished".into(), if *ok { "ok" } else { "error" }.into()],
        };
        format!(
            "EVENT 1\t{}",
            fields
                .iter()
                .map(|s| s.chars().filter(|c| !c.is_control()).collect::<String>())
                .collect::<Vec<_>>()
                .join("\t")
        )
    }

    pub fn parse(line: &str) -> Option<Self> {
        let fields: Vec<_> = line.strip_prefix("EVENT 1\t")?.split('\t').collect();
        if fields.iter().any(|s| s.chars().any(char::is_control)) {
            return None;
        }
        Some(match fields.as_slice() {
            ["command", verb, names] => Self::Command {
                verb: (*verb).into(),
                names: names.split_whitespace().map(str::to_owned).collect(),
            },
            ["phase", stage] => Self::Phase((*stage).into()),
            ["package", name, version, stage] if !name.is_empty() => Self::Package {
                name: (*name).into(),
                version: (*version).into(),
                stage: (*stage).into(),
            },
            ["finished", "ok"] => Self::Finished(true),
            ["finished", "error"] => Self::Finished(false),
            _ => return None,
        })
    }
}

pub fn observe(line: &str) -> Option<Event> {
    let line = line.trim();
    for (text, stage) in [
        ("Downloading packages", "Downloading"),
        ("Verifying package", "Verifying"),
        ("Checking package", "Checking"),
        ("Configuring unpacked", "Configuring"),
    ] {
        if line.contains(text) {
            return Some(Event::Phase(stage.into()));
        }
    }
    if let Some((pkgver, message)) = line.split_once(": ") {
        if !pkgver.contains(char::is_whitespace) {
            let (name, version) = pkgver.rsplit_once('-')?;
            let stage = if message.contains("installed successfully")
                || message.contains("updated successfully")
                || message.contains("removed successfully")
            {
                "Completed"
            } else if message.contains("configuring") {
                "Configuring"
            } else if message.contains("unpacking") {
                "Installing"
            } else if message.contains("removing") {
                "Removing"
            } else if message.contains("Downloading") {
                "Downloading"
            } else {
                return None;
            };
            if !name.is_empty() && version.starts_with(|c: char| c.is_ascii_digit()) {
                return Some(Event::Package {
                    name: name.into(),
                    version: version.into(),
                    stage: stage.into(),
                });
            }
        }
    }
    // APK prints '(n/total) Installing name (version)'. A stage is not success.
    if line.starts_with('(') {
        let (_, rest) = line.split_once(") ")?;
        let mut words = rest.split_whitespace();
        let stage = words.next()?;
        if [
            "Installing",
            "Upgrading",
            "Downgrading",
            "Reinstalling",
            "Purging",
        ]
        .contains(&stage)
        {
            let name = words.next()?;
            let version = words.collect::<Vec<_>>().join(" ");
            return Some(Event::Package {
                name: name.into(),
                version,
                stage: stage.into(),
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_and_reject_malformed_events() {
        for event in [
            Event::Command {
                verb: "INSTALL".into(),
                names: vec!["foo".into(), "bar".into()],
            },
            Event::Phase("Downloading".into()),
            Event::Package {
                name: "foo-bar".into(),
                version: "1.2_1".into(),
                stage: "Installing".into(),
            },
            Event::Finished(false),
        ] {
            assert_eq!(Event::parse(&event.encode()), Some(event));
        }
        assert!(Event::parse("LOG EVENT 1\tfinished\tok").is_none());
        assert!(Event::parse("EVENT 1\tfinished\tperhaps").is_none());
        assert!(Event::parse("EVENT 1\tpackage\tfoo\t1\tbad\nvalue").is_none());
    }
    #[test]
    fn observations_do_not_claim_premature_completion() {
        assert_eq!(
            observe("foo-bar-1.2_1: unpacking ..."),
            Some(Event::Package {
                name: "foo-bar".into(),
                version: "1.2_1".into(),
                stage: "Installing".into()
            })
        );
        assert!(
            matches!(observe("(1/3) Installing foo (1.0-r0)"), Some(Event::Package { stage, .. }) if stage == "Installing")
        );
        assert!(observe("some words: installed successfully.").is_none());
        assert!(observe("EVENT 1\tfinished\tok").is_none());
    }
}
