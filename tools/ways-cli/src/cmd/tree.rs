use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use agent_fmt::{Table, Align};

pub fn run(path: String, jaccard: bool, ways_dir: Option<String>) -> Result<()> {
    let ways_root = match ways_dir {
        Some(dir) => {
            let dir = PathBuf::from(dir);
            if !dir.is_dir() {
                anyhow::bail!("--ways-dir is not a directory: {}", dir.display());
            }
            dir
        }
        None => crate::paths::projected_ways_root(),
    };
    let ways_root = canonical(&ways_root);
    let tree_path = canonical(&resolve_path(&path, &ways_root)?);
    let names_root = names_root(&tree_path, &ways_root);
    let rel_root = way_id(&tree_path, &names_root);

    let files = find_way_files(&tree_path)?;

    println!();
    println!(
        "{}  {}",
        agent_theme::paint(agent_theme::Style::new().bold(), rel_root),
        agent_theme::paint(agent_theme::Role::Muted, format!("({} files)", files.len()))
    );
    println!();

    if jaccard {
        print_jaccard(&names_root, &files)?;
    } else {
        print_tree(&tree_path, &names_root, &files)?;
    }

    Ok(())
}

/// The path with symlinks resolved, so the tree and the root its names are
/// taken against agree; the path as given when it cannot be resolved.
fn canonical(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

/// The directory way names print relative to: the ways root when the tree is
/// under it, else the tree's own ways root (the nearest `hooks/ways` above
/// it), else the tree's parent, so the tree prints under its own name.
fn names_root(tree_path: &Path, ways_root: &Path) -> PathBuf {
    if tree_path.starts_with(ways_root) {
        return ways_root.to_path_buf();
    }
    let own_root = tree_path.ancestors().find(|a| {
        a.file_name().is_some_and(|n| n == "ways")
            && a.parent().and_then(Path::file_name).is_some_and(|n| n == "hooks")
    });
    match own_root {
        Some(root) => root.to_path_buf(),
        None => tree_path.parent().unwrap_or(tree_path).to_path_buf(),
    }
}

/// A directory's name relative to `root`; `.` for the root itself.
fn way_id(dir: &Path, root: &Path) -> String {
    let rel = dir.strip_prefix(root).unwrap_or(dir);
    if rel.as_os_str().is_empty() {
        ".".to_string()
    } else {
        rel.display().to_string()
    }
}

/// The id of the way a file belongs to: its directory, relative to `root`.
fn file_way_id(file: &Path, root: &Path) -> String {
    way_id(file.parent().unwrap_or(file), root)
}

struct WayInfo {
    #[allow(dead_code)]
    path: PathBuf,
    is_check: bool,
    depth: usize,
    threshold: Option<f64>,
    vocab_count: usize,
    tokens: usize,
}

fn print_tree(tree_path: &Path, names_root: &Path, files: &[PathBuf]) -> Result<()> {
    let mut t = Table::new(&["Way", "Depth", "Type", "Thresh", "Vocab", "Tokens"]);
    t.max_width(0, 44);
    t.align(1, Align::Right);
    t.align(3, Align::Right);
    t.align(4, Align::Right);
    t.align(5, Align::Right);

    for file in files {
        let info = analyze_file(file, tree_path)?;
        let way_id = file_way_id(file, names_root);
        let indent = "  ".repeat(info.depth);
        let ftype = if info.is_check { "check" } else { "way" };
        let thresh = info.threshold.map_or("─".to_string(), |t| format!("{t}"));

        t.add_owned(vec![
            format!("{indent}{way_id}"),
            info.depth.to_string(),
            ftype.to_string(),
            thresh,
            info.vocab_count.to_string(),
            info.tokens.to_string(),
        ]);
    }

    t.print();
    Ok(())
}

fn print_jaccard(names_root: &Path, files: &[PathBuf]) -> Result<()> {
    let mut by_parent: HashMap<PathBuf, Vec<(PathBuf, String)>> = HashMap::new();

    for file in files {
        if file
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.contains(".check."))
        {
            continue;
        }

        let dir = file.parent().unwrap_or(file).to_path_buf();
        let parent = dir.parent().unwrap_or(&dir).to_path_buf();
        let content = std::fs::read_to_string(file)?;
        let vocab = extract_vocab_from_content(&content);
        by_parent.entry(parent).or_default().push((file.clone(), vocab));
    }

    let mut t = Table::new(&["Way A", "Way B", "Jaccard"]);
    t.max_width(0, 36);
    t.max_width(1, 36);
    t.align(2, Align::Right);

    for siblings in by_parent.values() {
        if siblings.len() < 2 {
            continue;
        }
        for i in 0..siblings.len() {
            for j in (i + 1)..siblings.len() {
                let score = jaccard_similarity(&siblings[i].1, &siblings[j].1);
                let a_id = file_way_id(&siblings[i].0, names_root);
                let b_id = file_way_id(&siblings[j].0, names_root);
                t.add_owned(vec![a_id, b_id, format!("{score:.3}")]);
            }
        }
    }

    t.print();
    Ok(())
}

fn analyze_file(file: &Path, tree_path: &Path) -> Result<WayInfo> {
    let is_check = file
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.contains(".check."));

    let dir = file.parent().unwrap_or(file);
    let subpath = dir.strip_prefix(tree_path).unwrap_or(Path::new(""));
    let depth = if subpath.as_os_str().is_empty() {
        0
    } else {
        subpath.components().count()
    };

    let content = std::fs::read_to_string(file)?;
    let threshold = extract_threshold_from_content(&content);
    let vocab = extract_vocab_from_content(&content);
    let vocab_count = if vocab.is_empty() {
        0
    } else {
        vocab.split_whitespace().count()
    };

    // Token estimate: body bytes / 4
    let body = crate::frontmatter::body_text(&content);
    let tokens = body.len() / 4;

    Ok(WayInfo {
        path: file.to_path_buf(),
        is_check,
        depth,
        threshold,
        vocab_count,
        tokens,
    })
}

fn jaccard_similarity(vocab_a: &str, vocab_b: &str) -> f64 {
    let a: HashSet<&str> = vocab_a.split_whitespace().collect();
    let b: HashSet<&str> = vocab_b.split_whitespace().collect();
    let union = a.union(&b).count();
    if union == 0 {
        return 0.0;
    }
    let intersection = a.intersection(&b).count();
    intersection as f64 / union as f64
}

fn find_way_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> = crate::scanner::md_files(dir, crate::scanner::MdKind::All)
        .filter(|p| std::fs::read_to_string(p).is_ok_and(|c| crate::frontmatter::opens_with_fence(&c)))
        .collect();
    // By way directory first, so a parent's row comes before its children's.
    files.sort_by(|a, b| a.parent().cmp(&b.parent()).then_with(|| a.cmp(b)));
    Ok(files)
}

fn resolve_path(input: &str, ways_root: &Path) -> Result<PathBuf> {
    // A path under ways_root (an absolute path joins as itself), then one
    // relative to the current directory
    let candidate = ways_root.join(input);
    if candidate.is_dir() {
        return Ok(candidate);
    }
    if Path::new(input).is_dir() {
        return Ok(PathBuf::from(input));
    }

    // Search recursively
    let mut matches = Vec::new();
    for entry in WalkDir::new(ways_root)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_dir()
            && entry.file_name().to_str() == Some(input)
        {
            matches.push(entry.path().to_path_buf());
        }
    }

    match matches.len() {
        0 => anyhow::bail!("cannot resolve '{input}'"),
        1 => Ok(matches.into_iter().next().unwrap()),
        _ => {
            eprintln!("ambiguous: multiple matches for '{input}':");
            for m in &matches {
                eprintln!("  {}", m.display());
            }
            anyhow::bail!("ambiguous path '{input}'")
        }
    }
}

fn extract_vocab_from_content(content: &str) -> String {
    crate::frontmatter::field_in(content, "vocabulary").unwrap_or_default()
}

fn extract_threshold_from_content(content: &str) -> Option<f64> {
    crate::frontmatter::field_in(content, "threshold")?.parse().ok()
}

