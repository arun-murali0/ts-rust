use criterion::{Criterion, criterion_group, criterion_main};
use std::fs;
use std::hint::black_box;
use std::path::PathBuf;
use ts_rust::{ModuleGraph, ModuleResolver, check_project};

const LAYERS: usize = 20;
const WIDTH: usize = 10;
const FAN_OUT: usize = 3;
const INDEPENDENT_FILES: usize = 200;

// A project written under the temp directory for one benchmark and removed when it is
// dropped. Files are written before measuring starts, so only the work on them is timed.
struct Project {
    root: PathBuf,
    entries: Vec<PathBuf>,
}

impl Project {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("ts-rust-bench-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("benchmark directory should be creatable");
        Self {
            root,
            entries: Vec::new(),
        }
    }

    fn write(&mut self, name: &str, source: &str, entry: bool) {
        let path = self.root.join(name);
        fs::write(&path, source).expect("benchmark file should be writable");
        if entry {
            self.entries.push(path);
        }
    }

    // `main.ts` imports the first layer, and each file of a layer imports FAN_OUT files of
    // the next one. Imports are what the graph is made of, so this is the shape that
    // exercises discovery, resolution and layering.
    fn layered() -> Self {
        let mut project = Self::new("layered");
        let mut main = String::new();
        for i in 0..WIDTH {
            main.push_str(&format!("import {{ v0_{i} }} from \"./l0_{i}\";\n"));
        }
        project.write("main.ts", &main, true);

        for layer in 0..LAYERS {
            for i in 0..WIDTH {
                let mut source = String::new();
                if layer + 1 < LAYERS {
                    for step in 0..FAN_OUT {
                        let next = (i + step) % WIDTH;
                        source.push_str(&format!(
                            "import {{ v{n}_{next} }} from \"./l{n}_{next}\";\n",
                            n = layer + 1
                        ));
                    }
                }
                source.push_str(&format!("export const v{layer}_{i} = {i};\n"));
                project.write(&format!("l{layer}_{i}.ts"), &source, false);
            }
        }
        project
    }

    // Files with no imports, so the time is spent checking functions and not on the
    // warning each import statement still gets. Every file is an entry and none depends
    // on another, so one layer holds them all and the parallel driver has the most to
    // spread.
    fn independent() -> Self {
        let mut project = Self::new("independent");
        for n in 0..INDEPENDENT_FILES {
            let mut source = String::new();
            for i in 0..20 {
                source.push_str(&format!(
                    "function add{i}(a: number, b: number): number {{\n\
                     \treturn a + b;\n\
                     }}\n\
                     add{i}(1, 2);\n"
                ));
            }
            project.write(&format!("solo_{n}.ts"), &source, true);
        }
        project
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn bench_build(c: &mut Criterion) {
    let project = Project::layered();

    // A new resolver per run, so its directory and package.json caches start cold, which
    // is what a one-shot CLI invocation pays.
    c.bench_function("module_graph/build_layered_201_files", |b| {
        b.iter(|| {
            let resolver = ModuleResolver::new();
            let graph = ModuleGraph::build(black_box(&project.entries), &resolver)
                .expect("benchmark graph should build");
            assert_eq!(graph.len(), LAYERS * WIDTH + 1);
            black_box(graph)
        })
    });
}

fn bench_graph_queries(c: &mut Criterion) {
    let project = Project::layered();
    let graph = ModuleGraph::build(&project.entries, &ModuleResolver::new())
        .expect("benchmark graph should build");

    c.bench_function("module_graph/layers_layered_201_files", |b| {
        b.iter(|| black_box(graph.layers()))
    });

    c.bench_function("module_graph/cycles_layered_201_files", |b| {
        b.iter(|| black_box(graph.cycles()))
    });

    // Nothing was touched, so this is the cost of asking "did anything change" about a
    // project where the answer is no: one stat per file.
    c.bench_function("module_graph/changed_files_untouched_201_files", |b| {
        b.iter(|| black_box(graph.changed_files()))
    });
}

fn bench_check_project(c: &mut Criterion) {
    let project = Project::independent();
    let graph = ModuleGraph::build(&project.entries, &ModuleResolver::new())
        .expect("benchmark graph should build");

    c.bench_function("check_project/independent_200_files", |b| {
        b.iter(|| {
            let report = check_project(black_box(&graph));
            assert_eq!(report.failure_count(), 0);
            black_box(report)
        })
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default();
    targets = bench_build,
        bench_graph_queries,
        bench_check_project
}

criterion_main!(benches);
