use super::{
    AmountError, Order, call_twice, eager_labels, format_order, group_counts, paid_at_least,
    parse_amounts, qualifying, sample_orders,
};
use std::cell::Cell;
use std::collections::BTreeMap;

#[test]
fn closures_can_read_owned_data_or_mutate_borrowed_state() {
    let mut count = 0;
    assert_eq!(call_twice(|| { count += 1; count }), [1, 2]);
    assert_eq!(count, 2);
    let label = String::from("orders");
    assert_eq!(call_twice(move || label.len()), [6, 6]);
    let mut minimum = 1000_i64;
    let fixed = move |order: &Order| order.amount_cents >= minimum;
    minimum = 2000;
    assert!(fixed(&sample_orders()[0]));
    assert_eq!(minimum, 2000);
}

#[test]
fn iterator_modes_borrow_edit_or_move_elements() {
    let mut orders = sample_orders();
    let borrowed: Vec<_> = orders.iter().collect();
    assert!(std::ptr::eq(borrowed[0], &orders[0]));
    drop(borrowed);
    for order in orders.iter_mut() {
        order.amount_cents += 100;
    }
    assert_eq!(orders[0].amount_cents, 1300);
    let customers: Vec<String> = orders.into_iter().map(|order| order.customer).collect();
    assert_eq!(customers, ["Alice", "Bob", "Alice", "Carol", "Bob", "Alice"]);
}

#[test]
fn eager_and_lazy_reports_match_without_mutating_input() {
    let orders = sample_orders();
    let before = orders.clone();
    let expected = ["#1 Alice 1200", "#3 Alice 2500", "#5 Bob 1800"];
    assert_eq!(eager_labels(&orders, paid_at_least(1000)), expected);
    let mut eager_calls = 0;
    let eager = eager_labels(&orders, |order| {
        eager_calls += 1;
        order.paid && order.amount_cents >= 1000
    });
    assert_eq!(eager, expected);
    assert_eq!(eager_calls, 6);
    assert_eq!(qualifying(&orders, 1000).map(format_order).collect::<Vec<_>>(), expected);
    assert_eq!(orders, before);
}

#[test]
fn lazy_take_visits_only_enough_source_elements() {
    let orders = sample_orders();
    let filters = Cell::new(0);
    let maps = Cell::new(0);
    let pipeline = orders.iter().filter(|order| {
        filters.set(filters.get() + 1);
        order.paid && order.amount_cents >= 1000
    }).map(|order| {
        maps.set(maps.get() + 1);
        order.id
    }).take(2);
    assert_eq!((filters.get(), maps.get()), (0, 0));
    assert_eq!(pipeline.collect::<Vec<_>>(), [1, 3]);
    assert_eq!((filters.get(), maps.get()), (3, 2));
}

#[test]
fn take_zero_does_not_request_an_item() {
    let visits = Cell::new(0);
    let pipeline = (0..10).inspect(|_| visits.set(visits.get() + 1)).take(0);
    assert!(pipeline.collect::<Vec<_>>().is_empty());
    assert_eq!(visits.get(), 0);
}

#[test]
fn by_ref_keeps_iterator_position_for_remaining_items() {
    let orders = sample_orders();
    let mut iterator = orders.iter().map(|order| order.id);
    assert_eq!(iterator.by_ref().take(2).collect::<Vec<_>>(), [1, 2]);
    assert_eq!(iterator.collect::<Vec<_>>(), [3, 4, 5, 6]);
}

#[test]
fn fold_groups_only_qualifying_orders_in_sorted_order() {
    let expected = BTreeMap::from([(String::from("Alice"), 2), (String::from("Bob"), 1)]);
    assert_eq!(group_counts(&sample_orders(), 1000), expected);
    assert!(group_counts(&[], 1000).is_empty());
}

#[test]
fn paid_at_least_uses_inclusive_boundary_and_paid_flag() {
    let keep = paid_at_least(1000);
    for (amount_cents, paid, expected) in [(1000, true, true), (999, true, false), (1000, false, false)] {
        let order = Order { id: 1, customer: "dev".into(), amount_cents, paid };
        assert_eq!(keep(&order), expected);
    }
}

#[test]
fn result_collection_stops_at_first_error() {
    let mut attempts = 0;
    let result = parse_amounts(&["1200", "bad", "1800"], |_| attempts += 1);
    assert_eq!(result, Err(AmountError { index: 1, raw: "bad".into() }));
    assert_eq!(attempts, 2);
}

#[test]
fn parse_amounts_handles_ranges_and_invalid_input() {
    assert_eq!(parse_amounts(&["0", " 1200 ", "9223372036854775807"], |_| {}).unwrap(), [0, 1200, i64::MAX]);
    for raw in ["", "bad", "-1", "9223372036854775808"] {
        assert_eq!(parse_amounts(&[raw], |_| {}), Err(AmountError { index: 0, raw: raw.into() }));
    }
    assert!(parse_amounts(&[], |_| {}).unwrap().is_empty());
}
