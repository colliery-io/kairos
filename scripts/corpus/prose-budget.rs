//! Does less opening prose make the primary vector BETTER? (KAIROS-T-0190.)
//!
//! The earlier measurement found high-cosine false positives clustering in
//! initiative- and specification-level documents — long, heavily templated,
//! generically titled — and the hypothesis was that their opening prose is mostly
//! boilerplate and so the least discriminating input they have. That suggested a
//! per-entity-type prose budget.
//!
//! Before implementing four numbers chosen by intuition, test the hypothesis: vary
//! ONE budget and watch both sides of the trade at once.
//!
//!   recall@1   — can it still find a known duplicate? (higher is better)
//!   ≥0.93 pairs — how many unlinked same-project pairs cross the threshold where
//!                 precision was measured at roughly half? (lower is better)
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use std::collections::{HashMap, HashSet};
use std::io::BufRead;

fn norm_title(t: &str) -> String {
    t.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let corpus = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&corpus)?;
    let mut raw: Vec<serde_json::Value> = Vec::new();
    for line in std::io::BufReader::new(f).lines() {
        raw.push(serde_json::from_str(&line?)?);
    }
    let title_of: HashMap<String, String> = raw
        .iter()
        .filter_map(|v| {
            let sc = v["short_code"].as_str()?;
            (!sc.is_empty()).then(|| (sc.to_string(), v["title"].as_str().unwrap_or("").into()))
        })
        .collect();

    struct Doc {
        project: String,
        level: String,
        short_code: String,
        title: String,
        parent_title: String,
        prose: String,
    }
    let docs: Vec<Doc> = raw
        .iter()
        .map(|v| Doc {
            project: v["project"].as_str().unwrap_or("").into(),
            level: v["level"].as_str().unwrap_or("").into(),
            short_code: v["short_code"].as_str().unwrap_or("").into(),
            title: v["title"].as_str().unwrap_or("").into(),
            parent_title: v["parent"]
                .as_str()
                .and_then(|p| title_of.get(p))
                .cloned()
                .unwrap_or_default(),
            prose: v["chunks"]
                .as_array()
                .and_then(|cs| {
                    cs.iter()
                        .map(|c| c["text"].as_str().unwrap_or("").trim())
                        .find(|t| t.len() >= 40)
                })
                .unwrap_or("")
                .to_string(),
        })
        .collect();

    // ground truth: duplicate tickets (identical title, same project, different code)
    let mut groups: HashMap<(String, String), Vec<usize>> = HashMap::new();
    for (i, d) in docs.iter().enumerate() {
        if !d.title.trim().is_empty() {
            groups
                .entry((d.project.clone(), norm_title(&d.title)))
                .or_default()
                .push(i);
        }
    }
    let mut truth: Vec<(usize, usize)> = Vec::new();
    for g in groups.values().filter(|g| g.len() >= 2) {
        let codes: HashSet<&str> = g.iter().map(|&i| docs[i].short_code.as_str()).collect();
        if codes.len() < 2 {
            continue;
        }
        for a in 0..g.len() {
            for b in (a + 1)..g.len() {
                truth.push((g[a], g[b]));
            }
        }
    }

    let mut model = TextEmbedding::try_new(
        TextInitOptions::new(EmbeddingModel::BGESmallENV15Q).with_show_download_progress(false),
    )?;

    println!(
        "{} documents, {} duplicate pairs\n\n{:<9}{:>10}{:>14}{:>16}{:>18}",
        docs.len(),
        truth.len(),
        "prose",
        "recall@1",
        ">=0.93 pairs",
        "of those, init",
        "mean cos (true)"
    );

    for budget in [0usize, 150, 300, 600, 1200] {
        // Compose exactly as kairos_core::primary does: type, title, parent, prose.
        let texts: Vec<String> = docs
            .iter()
            .map(|d| {
                let prose: String = d.prose.chars().take(budget).collect();
                let mut s = format!("{}: {}\n", d.level, d.title);
                if !d.parent_title.is_empty() {
                    s.push_str(&format!("part of: {}\n", d.parent_title));
                }
                if !prose.is_empty() {
                    s.push_str(&prose);
                }
                s
            })
            .collect();
        let refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
        let mut vecs = model.embed(refs, Some(256))?;
        for v in vecs.iter_mut() {
            let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            if n > 0.0 {
                for x in v.iter_mut() {
                    *x /= n;
                }
            }
        }
        let dot = |a: usize, b: usize| -> f32 { vecs[a].iter().zip(&vecs[b]).map(|(p, q)| p * q).sum() };

        let mut hits = 0usize;
        let mut sims = Vec::new();
        let truth_set: HashSet<(usize, usize)> = truth
            .iter()
            .flat_map(|&(a, b)| [(a, b), (b, a)])
            .collect();
        for &(a, b) in &truth {
            sims.push(dot(a, b));
            for &(from, to) in &[(a, b), (b, a)] {
                let mut best = (f32::MIN, usize::MAX);
                for (j, d) in docs.iter().enumerate() {
                    if j != from && d.project == docs[from].project {
                        let c = dot(from, j);
                        if c > best.0 {
                            best = (c, j);
                        }
                    }
                }
                if best.1 == to {
                    hits += 1;
                }
            }
        }

        // unlinked same-project pairs over the threshold, and how many involve an
        // initiative — the level the false positives clustered in.
        let mut over = 0usize;
        let mut over_init = 0usize;
        for i in 0..docs.len() {
            for j in (i + 1)..docs.len() {
                if docs[i].project != docs[j].project || truth_set.contains(&(i, j)) {
                    continue;
                }
                if dot(i, j) > 0.93 {
                    over += 1;
                    if docs[i].level == "initiative" || docs[j].level == "initiative" {
                        over_init += 1;
                    }
                }
            }
        }
        let mean = sims.iter().sum::<f32>() / sims.len() as f32;
        println!(
            "{budget:<9}{:>9.0}%{over:>14}{over_init:>16}{mean:>18.3}",
            100.0 * hits as f64 / (2 * truth.len()) as f64
        );
    }
    Ok(())
}
