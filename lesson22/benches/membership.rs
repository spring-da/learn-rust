use lesson22::{build_index, count_indexed, count_linear, sample_data};
use std::hint::black_box;
use std::time::Instant;

fn measure(mut work: impl FnMut() -> usize) -> f64 {
    let start = Instant::now();
    for _ in 0..8 { black_box(work()); }
    start.elapsed().as_secs_f64() * 1_000_000.0 / 8.0
}

fn main() {
    let (allowed, queries) = sample_data();
    let index = build_index(&allowed);
    assert_eq!(count_linear(&allowed, &queries), count_indexed(&index, &queries));
    black_box(count_linear(black_box(&allowed), black_box(&queries)));
    black_box(count_indexed(black_box(&index), black_box(&queries)));
    let mut samples = [Vec::new(), Vec::new(), Vec::new()];
    for round in 0..5 {
        // 轮换测量顺序，减少固定次序带来的偏差。
        for offset in 0..3 {
            let variant = (round + offset) % 3;
            let time = match variant {
                0 => measure(|| count_linear(black_box(&allowed), black_box(&queries))),
                1 => measure(|| count_indexed(black_box(&index), black_box(&queries))),
                _ => measure(|| count_indexed(&build_index(black_box(&allowed)), black_box(&queries))),
            };
            samples[variant].push(time);
        }
    }
    for (name, mut values) in ["linear", "indexed_reuse", "build_and_count"].into_iter().zip(samples) {
        values.sort_by(f64::total_cmp);
        println!("{name:18} median_us/op={:.3} (5 samples, 8 iterations/sample)", values[2]);
    }
}
