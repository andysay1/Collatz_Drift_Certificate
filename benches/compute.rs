use criterion::{criterion_group, criterion_main, Criterion, Throughput, BenchmarkId};

fn collatz_s_sum(k: u32, l: u32, idx: usize) -> u32 {
    let mask: u64 = (1u64 << k) - 1;
    let mut m = ((idx as u64) << 1) | 1;
    let mut s: u64 = 0;
    for _ in 0..l {
        let t = 3u64.wrapping_mul(m & mask).wrapping_add(1);
        let e = t.trailing_zeros() as u64;
        s += e;
        m = (t >> e) & mask;
    }
    s.min(u32::MAX as u64) as u32
}

fn bench_collatz(c: &mut Criterion) {
    let mut group = c.benchmark_group("collatz_s_sum");
    for &(k,l) in &[(12u32,64u32),(16,64),(16,128)] {
        let n = 1usize << (k as usize - 1);
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(format!("k{}_l{}", k, l)), &n, |b, &n| {
            b.iter(|| {
                let mut min_s = u32::MAX;
                for idx in 0..n { let s = collatz_s_sum(k,l,idx); if s < min_s { min_s = s; } }
                criterion::black_box(min_s);
            })
        });
    }
    group.finish();
}

criterion_group!(benches, bench_collatz);
criterion_main!(benches);

