use lesson20::{
    call_twice, eager_labels, format_order, group_counts, paid_at_least, parse_amounts,
    qualifying, sample_orders,
};
use std::cell::Cell;

fn main() {
    println!("Lesson 20: closures, iterators, and an order report");
    let orders = sample_orders();

    println!("\n[1] Capture by move; classify closures by how their bodies use data");
    let mut minimum = 1000_i64;
    let fixed = move |order: &lesson20::Order| order.paid && order.amount_cents >= minimum;
    minimum = 2000;
    println!("minimum={minimum} order#1 captured(1000)={}", fixed(&orders[0]));
    let mut count = 0;
    let calls = call_twice(|| { count += 1; count });
    println!("FnMut calls={calls:?} final count={count}");
    let label = String::from("orders");
    let read_length = move || label.len();
    println!("owned capture read repeatedly: {:?}", call_twice(read_length));
    let payload = String::from("consume once");
    let consume = move || payload;
    println!("FnOnce result={:?}", consume());

    println!("\n[2] iter borrows, iter_mut edits, into_iter moves elements");
    let mut edited = sample_orders();
    let ids: Vec<_> = edited.iter().map(|order| order.id).collect();
    println!("borrowed ids={ids:?} original count={}", edited.len());
    for order in edited.iter_mut() {
        order.amount_cents += 100;
    }
    println!("after iter_mut: first={}", edited[0].amount_cents);
    let customers: Vec<String> = edited.into_iter().map(|order| order.customer).collect();
    println!("moved customer strings={customers:?}");

    println!("\n[3] Filter paid orders >= 1000 cents and map display lines");
    let eager = eager_labels(&orders, paid_at_least(1000));
    let lazy = qualifying(&orders, 1000).map(format_order);
    println!("eager={eager:?}\nlazy collected={:?}", lazy.collect::<Vec<_>>());

    println!("\n[4] Lazy evaluation, short-circuiting, and iterator position");
    // Cell 仅记录单线程次数，允许在闭包仍存在时读取计数。
    let filter_calls = Cell::new(0);
    let map_calls = Cell::new(0);
    let pipeline = orders.iter().filter(|order| {
        filter_calls.set(filter_calls.get() + 1);
        order.paid && order.amount_cents >= 1000
    }).map(|order| {
        map_calls.set(map_calls.get() + 1);
        format_order(order)
    }).take(2);
    println!("before consumption: filter={} map={}", filter_calls.get(), map_calls.get());
    let first: Vec<_> = pipeline.collect();
    println!("first={first:?} filter={} map={}", filter_calls.get(), map_calls.get());
    let mut cursor = orders.iter().map(|order| order.id);
    let head: Vec<_> = cursor.by_ref().take(2).collect();
    println!("cursor head={head:?} remaining={:?}", cursor.collect::<Vec<_>>());

    println!("\n[5] Fold into per-customer counts in sorted order");
    println!("counts={:?}", group_counts(&orders, 1000));

    println!("\n[6] collect<Result<Vec<_>, _>> stops at the first error");
    let mut attempts = 0;
    let result = parse_amounts(&["1200", "bad", "1800"], |_| attempts += 1);
    println!("result={result:?} attempts={attempts}");
}
