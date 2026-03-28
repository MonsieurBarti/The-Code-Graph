use criterion::{criterion_group, criterion_main, Criterion};
use domain::model::*;
use domain::ports::GraphStore;

fn setup_store() -> storage::SqliteStore {
    let store = storage::SqliteStore::open_in_memory().unwrap();
    // Insert synthesized data: 1000 symbols across 100 files
    for i in 0..1000usize {
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
                name: format!("func_{i}"),
                qualified_name: format!("{file_path}::func_{i}"),
                kind: SymbolKind::Function,
                location: Location {
                    file: file_path.into(),
                    line_start: i,
                    line_end: i + 10,
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
    // Insert edges: 500 call edges
    for i in 0..500usize {
        store
            .upsert_edge(&Edge {
                kind: EdgeKind::Calls,
                source: format!("src/mod_{}.rs::func_{}", i / 10, i),
                target: format!("src/mod_{}.rs::func_{}", (i + 1) / 10, i + 1),
                metadata: None,
            })
            .unwrap();
    }
    store
}

fn bench_query_latency(c: &mut Criterion) {
    let store = setup_store();
    let mut group = c.benchmark_group("query_latency");

    group.bench_function("find_by_name", |b| {
        b.iter(|| store.find_by_name("func_500").unwrap());
    });
    group.bench_function("all_symbols", |b| {
        b.iter(|| store.all_symbols().unwrap());
    });
    group.bench_function("all_edges", |b| {
        b.iter(|| store.all_edges().unwrap());
    });
    group.finish();
}

criterion_group!(benches, bench_query_latency);
criterion_main!(benches);
