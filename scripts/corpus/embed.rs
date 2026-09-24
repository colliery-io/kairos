//! KAIROS-I-0017 spike: is fastembed a viable local default (T-0189), and what
//! does the real cosine distribution look like across 19 Metis corpora (T-0190)?
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use std::collections::HashMap;
use std::io::{BufRead, BufWriter, Write};

const OPENING_PROSE: usize = 600;

struct Doc {
    project: String,
    level: String,
    short_code: String,
    title: String,
    parent: String,
    primary: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let corpus = &args[1];
    let outdir = &args[2];

    // ---- load -------------------------------------------------------------
    let f = std::fs::File::open(corpus)?;
    let mut raw: Vec<serde_json::Value> = Vec::new();
    for line in std::io::BufReader::new(f).lines() {
        raw.push(serde_json::from_str(&line?)?);
    }
    let title_of: HashMap<String, String> = raw
        .iter()
        .filter_map(|v| {
            let sc = v["short_code"].as_str()?;
            if sc.is_empty() { return None; }
            Some((sc.to_string(), v["title"].as_str().unwrap_or("").to_string()))
        })
        .collect();

    // ---- compose the primary vector text (A-0021 rule 4) ------------------
    // Structural inputs only: title, entity type, project (the repository
    // analogue), parent's title, opening prose. No section names.
    let docs: Vec<Doc> = raw
        .iter()
        .map(|v| {
            let level = v["level"].as_str().unwrap_or("").to_string();
            let project = v["project"].as_str().unwrap_or("").to_string();
            let title = v["title"].as_str().unwrap_or("").to_string();
            let parent = v["parent"].as_str().unwrap_or("").to_string();
            let parent_title = title_of.get(&parent).cloned().unwrap_or_default();
            let mut prose = String::new();
            if let Some(chunks) = v["chunks"].as_array() {
                for c in chunks {
                    let t = c["text"].as_str().unwrap_or("").trim();
                    if t.len() < 40 { continue; }
                    prose = t.chars().take(OPENING_PROSE).collect();
                    break;
                }
            }
            let primary = format!(
                "{level} in {project}: {title}\nparent: {parent_title}\n{prose}"
            );
            Doc { project, level, short_code: v["short_code"].as_str().unwrap_or("").to_string(),
                  title, parent, primary }
        })
        .collect();

    eprintln!("composed {} primary texts", docs.len());

    // ---- embed ------------------------------------------------------------
    let t0 = std::time::Instant::now();
    let mut model = TextEmbedding::try_new(
        TextInitOptions::new(EmbeddingModel::BGESmallENV15).with_show_download_progress(true),
    )?;
    eprintln!("model ready in {:.1}s", t0.elapsed().as_secs_f64());

    let t1 = std::time::Instant::now();
    let texts: Vec<&str> = docs.iter().map(|d| d.primary.as_str()).collect();
    let vecs = model.embed(texts, Some(256))?;
    let secs = t1.elapsed().as_secs_f64();
    eprintln!(
        "embedded {} texts in {:.1}s ({:.0}/s), dim={}",
        vecs.len(), secs, vecs.len() as f64 / secs, vecs[0].len()
    );

    // ---- persist so analysis can be re-run without re-embedding -----------
    let mut bw = BufWriter::new(std::fs::File::create(format!("{outdir}/primary.f32"))?);
    for v in &vecs {
        for x in v { bw.write_all(&x.to_le_bytes())?; }
    }
    bw.flush()?;
    let mut iw = BufWriter::new(std::fs::File::create(format!("{outdir}/primary.idx"))?);
    for d in &docs {
        writeln!(iw, "{}\t{}\t{}\t{}\t{}", d.project, d.level, d.short_code, d.parent, d.title)?;
    }
    iw.flush()?;
    eprintln!("wrote primary.f32 ({} x {}) and primary.idx", vecs.len(), vecs[0].len());
    Ok(())
}
