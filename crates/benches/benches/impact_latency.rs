use criterion::{criterion_group, criterion_main, Criterion};
use domain::model::*;
use domain::ports::GraphStore;
use domain::use_cases::impact::ImpactUseCase;

fn bench_impact_latency(c: &mut Criterion) {
    let store = storage::SqliteStore::open_in_memory().unwrap();
    // Build a chain of 500 symbols
    for i in 0..500usize {
        let file_path = format!("src/mod_{}.rs", i / 10);
        store
            .upsert_file(&FileNode {
                path: file_path.clone().into(),
                language: Language::Rust,
                hash: format!("hash_{i}"),
            })
            .unwrap();
        store
            .upsert_symbol(&SymbolNode {
                name: format!("fn_{i}"),
                qualified_name: format!("{file_path}::fn_{i}"),
                kind: SymbolKind::Function,
                location: Location {
                    file: file_path.into(),
                    line_start: i,
                    line_end: i + 5,
                    col_start: 0,
                    col_end: 1,
                },
                visibility: Visibility::Public,
                is_exported: true,
                is_async: false,
                is_test: false,
                decorators: vec![],
                signature: None,
            })
            .unwrap();
    }
    for i in 0..499usize {
        store
            .upsert_edge(&Edge {
                kind: EdgeKind::Calls,
                source: format!("src/mod_{}.rs::fn_{}", i / 10, i),
                target: format!("src/mod_{}.rs::fn_{}", (i + 1) / 10, i + 1),
                metadata: None,
            })
            .unwrap();
    }

    let uc = ImpactUseCase::new(store);
    let target = vec![ImpactTarget::Symbol("src/mod_0.rs::fn_0".into())];
    let mut group = c.benchmark_group("impact_latency");

    for depth in [1usize, 2, 3] {
        group.bench_function(format!("depth_{depth}"), |b| {
            b.iter(|| {
                uc.blast_radius(&target, depth, Confidence::Structural)
                    .unwrap()
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_impact_latency);
criterion_main!(benches);
