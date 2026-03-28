use criterion::{criterion_group, criterion_main, Criterion};
use std::path::Path;

fn bench_parse_throughput(c: &mut Criterion) {
    let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let registry = parser::ParserRegistry::new();
    let mut group = c.benchmark_group("parse_throughput");

    for (ext, label) in &[
        ("ts", "typescript"),
        ("py", "python"),
        ("rs", "rust"),
        ("go", "golang"),
    ] {
        let mut files = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&fixtures_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some(ext) {
                    if let Ok(content) = std::fs::read(&path) {
                        files.push((path, content));
                    }
                }
            }
        }
        if files.is_empty() {
            continue;
        }

        group.bench_function(*label, |b| {
            b.iter(|| {
                for (path, content) in &files {
                    if let Some(p) = registry.parser_for_file(path) {
                        let _ = p.parse(content, path);
                    }
                }
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_parse_throughput);
criterion_main!(benches);
