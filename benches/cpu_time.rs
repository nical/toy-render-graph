#[macro_use]
extern crate criterion;
use criterion::{Criterion, Bencher};

use rendergraph::*;

fn graph_2() -> Graph {
    let mut graph = Graph::new();

    let n0 = graph.add_node("n0", size2(100, 100), &[]);
    let _ = graph.add_node("n00", size2(100, 100), &[n0]);
    let n1 = graph.add_node("n1", size2(100, 100), &[]);
    let n2 = graph.add_node("n2", size2(100, 100), &[]);
    let n3 = graph.add_node("n3", size2(100, 100), &[n1, n2]);
    let n4 = graph.add_node("n4", size2(100, 100), &[]);
    let n5 = graph.add_node("n5", size2(100, 100), &[n2, n4]);
    let n6 = graph.add_node("n6", size2(100, 100), &[n1, n2, n3, n4, n5]);
    let n7 = graph.add_node("root", size2(800, 600), &[n6]);

    graph.add_root(n7);
    graph.add_root(n4);

    graph
}

fn graph_1() -> Graph {
    let mut graph = Graph::new();

    let n0 = graph.add_node("n0", size2(100, 100), &[]);
    let _ = graph.add_node("n00", size2(100, 100), &[n0]);
    let n1 = graph.add_node("n1", size2(100, 100), &[]);
    let n2 = graph.add_node("n2", size2(100, 100), &[]);
    let n3 = graph.add_node("n3", size2(100, 100), &[n1, n2]);
    let n4 = graph.add_node("n4", size2(100, 100), &[]);
    let n5 = graph.add_node("n5", size2(100, 100), &[n2, n4]);
    let n6 = graph.add_node("n6", size2(100, 100), &[n1, n2, n3, n4, n5]);
    let n7 = graph.add_node("root", size2(800, 600), &[n6]);

    graph.add_root(n7);
    graph.add_root(n4);

    let n0 = graph.add_node("n0", size2(100, 100), &[]);
    let _ = graph.add_node("n00", size2(100, 100), &[n0]);
    let n1 = graph.add_node("n1", size2(100, 100), &[]);
    let n2 = graph.add_node("n2", size2(100, 100), &[]);
    let n3 = graph.add_node("n3", size2(100, 100), &[n1, n2]);
    let n4 = graph.add_node("n4", size2(100, 100), &[]);
    let n5 = graph.add_node("n5", size2(100, 100), &[n2, n4]);
    let n6 = graph.add_node("n6", size2(100, 100), &[n1, n2, n3, n4, n5]);
    let n8 = graph.add_node("n8", size2(800, 600), &[n6]);

    graph.add_root(n8);

    let n0 = graph.add_node("n0", size2(100, 100), &[]);
    let _ = graph.add_node("n00", size2(100, 100), &[n0]);
    let n1 = graph.add_node("n1", size2(100, 100), &[]);
    let n2 = graph.add_node("n2", size2(100, 100), &[]);
    let n3 = graph.add_node("n3", size2(100, 100), &[n1, n2]);
    let n4 = graph.add_node("n4", size2(100, 100), &[]);
    let n5 = graph.add_node("n5", size2(100, 100), &[n2, n4]);
    let n6 = graph.add_node("n6", size2(100, 100), &[n1, n2, n3, n4, n5]);
    let n9 = graph.add_node("n9", size2(800, 600), &[n6]);


    let n0 = graph.add_node("n0", size2(100, 100), &[]);
    let _ = graph.add_node("n00", size2(100, 100), &[n0]);
    let n1 = graph.add_node("n1", size2(100, 100), &[]);
    let n2 = graph.add_node("n2", size2(100, 100), &[]);
    let n3 = graph.add_node("n3", size2(100, 100), &[n1, n2]);
    let n4 = graph.add_node("n4", size2(100, 100), &[]);
    let n5 = graph.add_node("n5", size2(100, 100), &[n2, n4]);
    let n10 = graph.add_node("n6", size2(100, 100), &[n2, n4, n5]);

    let n11 = graph.add_node("n11", size2(100, 100), &[n8, n9]);
    let n12 = graph.add_node("n12", size2(100, 100), &[n11, n10]);

    graph.add_root(n12);

    graph
}


fn do_bench(
    c: &mut Criterion,
    name: &'static str,
    graph_fn: &'static Fn() -> Graph,
    options: BuilderOptions,
) {
    c.bench_function(
        name,
        move |b: &mut Bencher| {
            let graph = graph_fn();

            let mut builder = GraphBuilder::new(options);

            let mut allocator = GuillotineAllocator::new();

            b.iter(|| {
                allocator.textures.clear();
                let _ = builder.build(graph.clone(), &mut allocator);
            })
        }
    );
}

fn do_bench_no_allocator(
    c: &mut Criterion,
    name: &'static str,
    graph_fn: &'static Fn() -> Graph,
    options: BuilderOptions,
) {
    c.bench_function(
        name,
        move |b: &mut Bencher| {
            let graph = graph_fn();

            let mut builder = GraphBuilder::new(options);

            let mut allocator = DummyAtlasAllocator::new();

            b.iter(|| {
                let _ = builder.build(graph.clone(), &mut allocator);
            })
        }
    );
}

fn culled_eager_pingpong_guillotine(c: &mut Criterion) {
    do_bench(c, "culled_eager_pingpong_guillotine",
        &graph_1,
        BuilderOptions {
            culling: true,
            passes: PassOptions::Eager,
            targets: TargetOptions::PingPong,
        },
    )
}

fn culled_eager_direct_guillotine(c: &mut Criterion) {
    do_bench(c, "culled_eager_direct_guillotine",
        &graph_1,
        BuilderOptions {
            culling: true,
            passes: PassOptions::Eager,
            targets: TargetOptions::Direct,
        },
    )
}

fn culled_eager_pingpong_no_allocator(c: &mut Criterion) {
    do_bench_no_allocator(c, "culled_eager_pingpong_no_allocator",
        &graph_1,
        BuilderOptions {
            culling: true,
            passes: PassOptions::Eager,
            targets: TargetOptions::PingPong,
        },
    )
}

criterion_group!(benches,
    culled_eager_pingpong_guillotine,
    culled_eager_direct_guillotine,
    culled_eager_pingpong_no_allocator,
);

criterion_main!(benches);
