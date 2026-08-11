use clap::Parser;
use regex::Regex;
use serde::Serialize;
use std::fs;
use walkdir::WalkDir;

#[derive(Parser)]
struct Opts {
    /// Root path to scan
    #[arg(short, long, default_value = ".")]
    path: String,

    /// Output JSON report path
    #[arg(short, long, default_value = "autofix-report.json")]
    out: String,

    /// Dry run only (do not apply anything)
    #[arg(long, default_value_t = true)]
    dry_run: bool,
}

#[derive(Serialize)]
struct Match {
    file: String,
    line: usize,
    column: usize,
    text: String,
    pattern: String,
    suggestion: String,
}

fn main() -> anyhow::Result<()> {
    let opts = Opts::parse();
    let patterns = vec![
        ("unwrap", Regex::new(r"\b([a-zA-Z0-9_.)>]+)\.unwrap\(\)")?),
        (
            "expect",
            Regex::new(r"\b([a-zA-Z0-9_.)>]+)\.expect\(.*?\)")?,
        ),
        (
            "downcast_unwrap",
            Regex::new(r"downcast::<([A-Za-z0-9_:]+)>\(\)\.unwrap\(\)")?,
        ),
        (
            "downcast_ref",
            Regex::new(r"downcast_ref::<([A-Za-z0-9_:]+)>\(\)")?,
        ),
    ];

    let mut matches: Vec<Match> = Vec::new();

    for entry in WalkDir::new(&opts.path).into_iter().filter_map(Result::ok) {
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        let s = p.to_string_lossy();
        if !s.ends_with(".rs") {
            continue;
        }
        if s.contains("/target/") || s.contains("/.git/") {
            continue;
        }
        let content = fs::read_to_string(p)?;
        for (pat_name, re) in &patterns {
            for (ln, line) in content.lines().enumerate() {
                if let Some(m) = re.captures(line) {
                    let m0 = m.get(0).unwrap();
                    let col = m0.start() + 1;
                    let found = m0.as_str().to_string();
                    let suggestion = suggest_fix(p.to_string_lossy().as_ref(), line, pat_name);
                    matches.push(Match {
                        file: s.to_string(),
                        line: ln + 1,
                        column: col,
                        text: found,
                        pattern: pat_name.to_string(),
                        suggestion,
                    });
                }
            }
        }
    }

    let json = serde_json::to_string_pretty(&matches)?;
    fs::write(&opts.out, &json)?;
    println!("Wrote {} matches to {}", matches.len(), &opts.out);
    Ok(())
}

fn suggest_fix(_file: &str, line: &str, pat: &str) -> String {
    match pat {
        "unwrap" => format!("Consider replacing `.unwrap()` with `match ... {{ Some(x) => x, None => {{ eprintln!(\\\"...\\\"); return; }} }}` or `if let Some(x) = ...` depending on context. Found: `{}`", line.trim()),
        "expect" => format!("Consider handling the error instead of `.expect(...)`. Found: `{}`", line.trim()),
        "downcast_unwrap" => format!("Replace `downcast::<T>().unwrap()` with a safe match or downcast_ref and log on failure. Found: `{}`", line.trim()),
        "downcast_ref" => format!("Verify type before using `downcast_ref`, avoid `unwrap()`. Found: `{}`", line.trim()),
        _ => "".to_string(),
    }
}
