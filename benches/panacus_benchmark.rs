use criterion::{black_box, criterion_group, criterion_main, Criterion};
use panacus::{analyses::InputRequirement, graph_broker::GraphBroker};
use std::collections::HashSet;

fn benchmark_graph_broker_hist(c: &mut Criterion) {
    let gfa_file = "./benches/chrM.pan.fa.6626ff2.4030258.6a1ecc2.smooth.gfa";
    let input_requirements = HashSet::from([
        InputRequirement::Hist,
        InputRequirement::Graph(gfa_file.to_string()),
        InputRequirement::Node,
        InputRequirement::Bp,
        InputRequirement::Edge,
        InputRequirement::PathLens,
    ]);
    c.bench_function("graph_broker_hist", |b| {
        b.iter(|| GraphBroker::from_gfa(black_box(&input_requirements)))
    });
}

fn benchmark_graph_broker_hist_finish(c: &mut Criterion) {
    let gfa_file = "./benches/chrM.pan.fa.6626ff2.4030258.6a1ecc2.smooth.gfa";
    let input_requirements = HashSet::from([
        InputRequirement::Hist,
        InputRequirement::Graph(gfa_file.to_string()),
        InputRequirement::Node,
        InputRequirement::Bp,
        InputRequirement::Edge,
        InputRequirement::PathLens,
    ]);
    let gb = GraphBroker::from_gfa(black_box(&input_requirements));
    c.bench_function("graph_broker_hist_finish", |b| {
        b.iter(|| black_box((&gb).clone()).finish())
    });
}

fn benchmark_graph_broker_hist_node(c: &mut Criterion) {
    let gfa_file = "./benches/chrM.pan.fa.6626ff2.4030258.6a1ecc2.smooth.gfa";
    let input_requirements = HashSet::from([
        InputRequirement::Hist,
        InputRequirement::Graph(gfa_file.to_string()),
        InputRequirement::Node,
    ]);
    c.bench_function("graph_broker_hist_node", |b| {
        b.iter(|| GraphBroker::from_gfa(black_box(&input_requirements)))
    });
}

criterion_group!(
    benches,
    benchmark_graph_broker_hist,
    benchmark_graph_broker_hist_finish,
    benchmark_graph_broker_hist_node
);
criterion_main!(benches);
