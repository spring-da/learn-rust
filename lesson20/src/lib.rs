//! 第二十课：闭包如何使用捕获值，以及迭代器何时执行。
//!
//! 把捕获的 String 移出闭包体，闭包只能调用一次：
//!
//! ```compile_fail,E0382
//! let label = String::from("orders");
//! let consume = move || label;
//! let first = consume();
//! let second = consume();
//! ```
//!
//! 修改捕获环境的闭包不能满足只接受 Fn 的约束：
//!
//! ```compile_fail,E0525
//! fn require_fn<F: Fn() -> usize>(callback: F) { callback(); }
//! let mut count = 0;
//! let callback = || { count += 1; count };
//! require_fn(callback);
//! ```
//!
//! 对拥有的 Vec 调用 into_iter，原 Vec 随之移动：
//!
//! ```compile_fail,E0382
//! let names = vec![String::from("Alice")];
//! let owned_names: Vec<String> = names.into_iter().collect();
//! println!("{names:?}");
//! ```
//!
//! collect 按值接收这个迭代器，之后不能直接再次使用该变量：
//!
//! ```compile_fail,E0382
//! let mut iterator = [1, 2, 3].into_iter().map(|value| value * 2);
//! let values: Vec<_> = iterator.collect();
//! iterator.next();
//! ```
//!
//! move 决定如何捕获，Fn/FnMut/FnOnce 取决于闭包体如何使用捕获的数据：
//!
//! ```
//! fn twice<F: Fn() -> usize>(callback: F) -> [usize; 2] {
//!     [callback(), callback()]
//! }
//! let label = String::from("orders");
//! let read_length = move || label.len();
//! assert_eq!(twice(read_length), [6, 6]);
//! ```
//!
//! by_ref 暂时借用迭代器，使 take 之后仍能从原来的位置继续：
//!
//! ```
//! let mut iterator = [1, 2, 3].into_iter();
//! let first: Vec<_> = iterator.by_ref().take(1).collect();
//! assert_eq!(first, [1]);
//! assert_eq!(iterator.collect::<Vec<_>>(), [2, 3]);
//! ```

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Order {
    pub id: usize,
    pub customer: String,
    pub amount_cents: i64,
    pub paid: bool,
}

pub fn sample_orders() -> Vec<Order> {
    [
        (1, "Alice", 1200, true),
        (2, "Bob", 800, false),
        (3, "Alice", 2500, true),
        (4, "Carol", 600, true),
        (5, "Bob", 1800, true),
        (6, "Alice", 900, true),
    ].into_iter().map(|(id, customer, amount_cents, paid)| Order {
        id, customer: customer.into(), amount_cents, paid,
    }).collect()
}

pub fn paid_at_least(minimum: i64) -> impl Fn(&Order) -> bool {
    move |order| order.paid && order.amount_cents >= minimum
}

pub fn format_order(order: &Order) -> String {
    format!("#{} {} {}", order.id, order.customer, order.amount_cents)
}

pub fn eager_labels(orders: &[Order], mut keep: impl FnMut(&Order) -> bool) -> Vec<String> {
    let mut labels = Vec::new();
    for order in orders {
        if keep(order) {
            labels.push(format_order(order));
        }
    }
    labels
}

// filter 的谓词收到 &&Order；自动解引用使字段访问依然简洁。
// 返回的流水线借用 orders，minimum 则由 move 闭包按值捕获。
pub fn qualifying<'a>(orders: &'a [Order], minimum: i64) -> impl Iterator<Item = &'a Order> + 'a {
    orders.iter().filter(move |order| order.paid && order.amount_cents >= minimum)
}

pub fn group_counts(orders: &[Order], minimum: i64) -> BTreeMap<String, usize> {
    qualifying(orders, minimum).fold(BTreeMap::new(), |mut counts, order| {
        *counts.entry(order.customer.clone()).or_insert(0) += 1;
        counts
    })
}

#[derive(Debug, PartialEq, Eq)]
pub struct AmountError {
    pub index: usize,
    pub raw: String,
}

impl fmt::Display for AmountError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid non-negative i64 amount at index {}: {:?}", self.index, self.raw)
    }
}

impl Error for AmountError {}

pub fn parse_amounts(
    raw: &[&str],
    mut on_attempt: impl FnMut(&str),
) -> Result<Vec<i64>, AmountError> {
    raw.iter().enumerate().map(|(index, text)| {
        on_attempt(text);
        match text.trim().parse::<i64>() {
            Ok(amount) if amount >= 0 => Ok(amount),
            _ => Err(AmountError { index, raw: (*text).into() }),
        }
    }).collect()
}

pub fn call_twice(mut callback: impl FnMut() -> usize) -> [usize; 2] {
    [callback(), callback()]
}

#[cfg(test)]
mod tests;
