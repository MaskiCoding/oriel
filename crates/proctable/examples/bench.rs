//! What a Lantern poll costs, broken down. `cargo run --release -p proctable --example bench`
use std::time::Instant;

fn main() {
    let mut table_ms = Vec::new();
    let mut desc_ms = Vec::new();
    let mut detail_ms = Vec::new();
    let mut under_n = 0;
    let mut reader = proctable::Reader::new();
    let mut total_n = 0;

    for _ in 0..10 {
        let t = Instant::now();
        let mut table = proctable::table();
        table_ms.push(t.elapsed().as_secs_f64() * 1000.0);
        total_n = table.len();

        // stand in for every app that can own a window
        let roots: Vec<i32> = table
            .iter()
            .filter(|p| p.ppid == 1)
            .map(|p| p.pid)
            .collect();

        let t = Instant::now();
        let under = model::descendants(&table, &roots);
        desc_ms.push(t.elapsed().as_secs_f64() * 1000.0);
        under_n = under.len();

        let t = Instant::now();
        reader.detail(&mut table, &under);
        detail_ms.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let med = |mut v: Vec<f64>| {
        v.sort_by(f64::total_cmp);
        v[v.len() / 2]
    };
    println!("processes {total_n}, under roots {under_n}");
    println!("  table()      {:>7.2} ms", med(table_ms));
    println!("  descendants  {:>7.2} ms", med(desc_ms));
    let first = detail_ms[0];
    println!(
        "  detail()     {:>7.2} ms  (first sweep {first:.2} ms)",
        med(detail_ms)
    );
}
