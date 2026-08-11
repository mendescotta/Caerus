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

    /// Apply comments inline to files (inserts TODO comment above each match)
    #[arg(long, default_value_t = false)]
    apply: bool,

    /// Apply safe downcast unwrap rewrites (very conservative)
    #[arg(long, default_value_t = false)]
    apply_safe: bool,
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
        ("expect", Regex::new(r"\b([a-zA-Z0-9_.)>]+)\.expect\(.*?\)")?),
        ("downcast_unwrap", Regex::new(r"downcast::<([A-Za-z0-9_:]+)>\(\)\.unwrap\(\)")?),
        ("downcast_ref", Regex::new(r"downcast_ref::<([A-Za-z0-9_:]+)>\(\)")?),
    ];

    let mut matches: Vec<Match> = Vec::new();

    for entry in WalkDir::new(&opts.path).into_iter().filter_map(Result::ok) {
        let p = entry.path();
        if !p.is_file() { continue; }
        let s = p.to_string_lossy();
        if !s.ends_with(".rs") { continue; }
        if s.contains("/target/") || s.contains("/.git/") { continue; }
        let content = fs::read_to_string(p)?;
        for (pat_name, re) in &patterns {
            for (ln, line) in content.lines().enumerate() {
                if let Some(m) = re.captures(line) {
// AUTOFIX: Consider replacing `.unwrap()` with `match ... { Some(x) => x, None => { eprintln!(\"...\"); return; } }` or `if let Some(x) = ...` depending on context. Found: `let m0 = m.get(0).unwrap();`

                    let m0 = m.get(0).unwrap();
                    let col = m0.start()+1;
                    let found = m0.as_str().to_string();
                    let suggestion = suggest_fix(p.to_string_lossy().as_ref(), &line, pat_name);
                    matches.push(Match {
                        file: s.to_string(),
                        line: ln+1,
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

    if opts.apply {
        apply_comments(&matches)?;
        println!("Applied comments to files (in-place).");
    }

    if opts.apply_safe {
        apply_safe_edits()?;
        println!("Applied conservative safe rewrites (in-place).");
    }

    Ok(())
}

fn apply_comments(matches: &Vec<Match>) -> anyhow::Result<()> {
    use std::collections::BTreeMap;
    // Group matches by file
    let mut by_file: BTreeMap<String, Vec<&Match>> = BTreeMap::new();
    for m in matches {
        by_file.entry(m.file.clone()).or_default().push(m);
    }
    for (file, muts) in by_file {
        // Read file
        let path = std::path::Path::new(&file);
        let content = fs::read_to_string(path)?;
        let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
        // Sort matches descending by line so insertions don't shift later indices
        let mut muts_sorted = muts.clone();
        muts_sorted.sort_by(|a, b| b.line.cmp(&a.line));
        for m in muts_sorted {
            let idx = if m.line == 0 { 0 } else { m.line - 1 };
            if idx <= lines.len() {
                let comment = format!("// AUTOFIX: {}\n", m.suggestion.replace('\n'," "));
                lines.insert(idx, comment);
            }
        }
        let new = lines.join("\n");
        fs::write(path, new)?;
    }
    Ok(())
}

fn apply_safe_edits() -> anyhow::Result<()> {
    use regex::Regex;
    use std::collections::BTreeMap;
    let pattern = Regex::new(r"let\s+([A-Za-z0-9_]+)\s*=\s*item\.child\(\)\.and_downcast::<([A-Za-z0-9_:]+)>\(\)\.unwrap\(\)\s*;")?;
    let mut changed_files: BTreeMap<String, String> = BTreeMap::new();
    for entry in WalkDir::new(".").into_iter().filter_map(Result::ok) {
        let p = entry.path();
        if !p.is_file() { continue; }
        let s = p.to_string_lossy();
        if !s.ends_with(".rs") { continue; }
        if s.contains("/target/") || s.contains("/.git/") { continue; }
        let content = fs::read_to_string(p)?;
        if !pattern.is_match(&content) { continue; }
        let new = pattern.replace_all(&content, |caps: &regex::Captures| {
            let var = &caps[1];
            let ty = &caps[2];
            let msg = format!("caerus: expected {} child in {}", ty, s);
            format!("let {var} = match item.child().and_downcast::<{ty}>() {{ Some(v) => v, None => {{ eprintln!(\"{}\"); return; }} }};", msg)
        });
        changed_files.insert(s.to_string(), new.to_string());
    }
    for (file, new_content) in changed_files {
        fs::write(&file, new_content)?;
        println!("Patched {}", file);
    }
    Ok(())
}

fn suggest_fix(_file: &str, line: &str, pat: &str) -> String {
    match pat {
        "unwrap" => format!("Consider replacing `.unwrap()` with `match ... {{ Some(x) => x, None => {{ eprintln!(\\\"...\\\"); return; }} }}` or `if let Some(x) = ...` depending on context. Found: `{}`", line.trim()),
        "expect" => format!("Consider handling the error instead of `.expect(...)`. Found: `{}`", line.trim()),
// AUTOFIX: Replace `downcast::<T>().unwrap()` with a safe match or downcast_ref and log on failure. Found: `"downcast_unwrap" => format!("Replace `downcast::<T>().unwrap()` with a safe match or downcast_ref and log on failure. Found: `{}`", line.trim()),`

        "downcast_unwrap" => format!("Replace `downcast::<T>().unwrap()` with a safe match or downcast_ref and log on failure. Found: `{}`", line.trim()),
        "downcast_ref" => format!("Verify type before using `downcast_ref`, avoid `unwrap()`. Found: `{}`", line.trim()),
        _ => "".to_string(),
    }
}