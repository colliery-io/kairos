//! KAIROS-T-0190's measurement: the real cosine distribution over 19 Metis
//! corpora, split by what the graph says about each pair. Full pairwise, so the
//! numbers are exact rather than sampled.
use std::collections::HashMap;
use std::io::Read;

const DIM: usize = 384;

#[derive(Clone)]
struct Meta { project: String, level: String, short_code: String, parent: String, title: String }

fn pct(v: &mut Vec<f32>, p: f64) -> f32 {
    if v.is_empty() { return f32::NAN; }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[((v.len() - 1) as f64 * p / 100.0).round() as usize]
}

fn describe(name: &str, v: &mut Vec<f32>) {
    if v.is_empty() { println!("{name:<34} (none)"); return; }
    let n = v.len();
    let mean = v.iter().sum::<f32>() / n as f32;
    println!("{name:<34}{n:>11}{mean:>8.3}{:>8.3}{:>8.3}{:>8.3}{:>8.3}{:>8.3}",
        pct(v, 50.0), pct(v, 90.0), pct(v, 99.0), pct(v, 99.9), pct(v, 100.0));
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::env::args().nth(1).unwrap();
    let mut buf = Vec::new();
    std::fs::File::open(format!("{dir}/primary.f32"))?.read_to_end(&mut buf)?;
    let n = buf.len() / 4 / DIM;
    let mut vecs: Vec<f32> = Vec::with_capacity(n * DIM);
    for c in buf.chunks_exact(4) { vecs.push(f32::from_le_bytes(c.try_into().unwrap())); }
    for i in 0..n {
        let s = &mut vecs[i*DIM..(i+1)*DIM];
        let norm = s.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 { for x in s.iter_mut() { *x /= norm; } }
    }

    let idx = std::fs::read_to_string(format!("{dir}/primary.idx"))?;
    let meta: Vec<Meta> = idx.lines().map(|l| {
        let f: Vec<&str> = l.split('\t').collect();
        Meta { project: f[0].into(), level: f[1].into(), short_code: f[2].into(),
               parent: f[3].into(), title: f.get(4).unwrap_or(&"").to_string() }
    }).collect();
    assert_eq!(meta.len(), n, "index/vector count mismatch");
    println!("{n} primary vectors, dim {DIM}\n");

    let key = |m: &Meta| format!("{}/{}", m.project, m.short_code);
    let by_key: HashMap<String, usize> =
        meta.iter().enumerate().map(|(i, m)| (key(m), i)).collect();
    let parent_idx: Vec<Option<usize>> = meta.iter()
        .map(|m| if m.parent.is_empty() { None }
                 else { by_key.get(&format!("{}/{}", m.project, m.parent)).copied() })
        .collect();

    let dot = |a: usize, b: usize| -> f32 {
        let (x, y) = (&vecs[a*DIM..(a+1)*DIM], &vecs[b*DIM..(b+1)*DIM]);
        x.iter().zip(y).map(|(p, q)| p * q).sum()
    };

    let mut same_parent = Vec::new();
    let mut parent_child = Vec::new();
    let mut same_proj = Vec::new();
    let mut cross_proj = Vec::new();
    let mut unlinked: Vec<(f32, usize, usize)> = Vec::new();
    let mut top1: Vec<f32> = vec![-2.0; n];

    for i in 0..n {
        for j in (i + 1)..n {
            let c = dot(i, j);
            if c > top1[i] { top1[i] = c; }
            if c > top1[j] { top1[j] = c; }
            let linked = parent_idx[i] == Some(j) || parent_idx[j] == Some(i);
            let sib = parent_idx[i].is_some() && parent_idx[i] == parent_idx[j];
            if meta[i].project != meta[j].project {
                if cross_proj.len() < 4_000_000 { cross_proj.push(c); }
            } else if linked {
                parent_child.push(c);
            } else if sib {
                same_parent.push(c);
            } else {
                same_proj.push(c);
                if c > 0.90 { unlinked.push((c, i, j)); }
            }
        }
    }

    println!("{:<34}{:>11}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}",
             "pair class", "n", "mean", "p50", "p90", "p99", "p99.9", "max");
    describe("different project", &mut cross_proj);
    describe("same project, no graph relation", &mut same_proj);
    describe("same project, shared parent", &mut same_parent);
    describe("direct parent/child edge", &mut parent_child);
    let mut t = top1.clone();
    describe("nearest neighbour, per document", &mut t);

    // dump for the lexical-overlap comparison
    {
        use std::io::Write;
        let mut w = std::io::BufWriter::new(std::fs::File::create(format!("{dir}/unlinked.tsv")).unwrap());
        for (c, i, j) in &unlinked {
            writeln!(w, "{c:.4}\t{}\t{}\t{}\t{}\t{}\t{}", meta[*i].project,
                meta[*i].level, meta[*i].title, meta[*j].level, meta[*j].title,
                meta[*i].short_code).unwrap();
        }
    }
    println!("\nunlinked same-project pairs above 0.90: {}", unlinked.len());
    unlinked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    for (c, i, j) in unlinked.iter().take(24) {
        println!("  {c:.3}  {:<9} {:<13} {:.56}", meta[*i].project, meta[*i].short_code, meta[*i].title);
        println!("         {:<9} {:<13} {:.56}", meta[*j].project, meta[*j].short_code, meta[*j].title);
    }
    Ok(())
}
