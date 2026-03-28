use criterion::{criterion_group, criterion_main, Criterion};
use domain::model::*;
use domain::ports::GraphStore;
use std::path::Path;

fn bench_incremental_latency(c: &mut Criterion) {
    let store = storage::SqliteStore::open_in_memory().unwrap();
    // Setup: 100 files, 10 symbols each
    for i in 0..100usize {
        let file_path = format!("src/file_{i}.rs");
        store
            .upsert_file(&FileNode {
                path: file_path.clone().into(),
                language: Language::Rust,
                hash: format!("hash_{i}"),
            })
            .unwrap();
        for j in 0..10usize {
            store
                .upsert_symbol(&SymbolNode {
                    name: format!("sym_{i}_{j}"),
                    qualified_name: format!("{file_path}::sym_{i}_{j}"),
                    kind: SymbolKind::Function,
                    location: Location {
                        file: file_path.clone().into(),
                        line_start: j * 10,
                        line_end: j * 10 + 9,
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
    }

    let mut group = c.benchmark_group("incremental_latency");
    group.bench_function("symbols_for_files_1", |b| {
        b.iter(|| {
            store
                .symbols_for_files(&[Path::new("src/file_50.rs")])
                .unwrap()
        });
    });
    group.bench_function("symbols_for_files_10", |b| {
        let paths: Vec<String> = (0..10).map(|i| format!("src/file_{i}.rs")).collect();
        let path_refs: Vec<&Path> = paths.iter().map(|p| Path::new(p.as_str())).collect();
        b.iter(|| store.symbols_for_files(&path_refs).unwrap());
    });
    group.finish();
}

criterion_group!(benches, bench_incremental_latency);
criterion_main!(benches);
