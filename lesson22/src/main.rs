use lesson22::{build_index, count_indexed, count_linear, sample_data};

fn main() {
    let (allowed, queries) = sample_data();
    let linear = count_linear(&allowed, &queries);
    let indexed = count_indexed(&build_index(&allowed), &queries);
    assert_eq!(linear, indexed);
    println!("allowed={} queries={} linear_hits={linear} indexed_hits={indexed}", allowed.len(), queries.len());
    println!("Run cargo test, then cargo bench --bench membership");
}
