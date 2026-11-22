//! 第十七课：用编译器验证 Vec 的所有权与借用规则。
//!
//! Vec 赋值后，旧变量不能继续使用：
//!
//! ```compile_fail,E0382
//! let original = vec![10, 20, 30];
//! let owner = original;
//! println!("{original:?} {owner:?}");
//! ```
//!
//! 即使预留了充足容量，push 的可变借用仍与后续使用的元素引用冲突：
//!
//! ```compile_fail,E0502
//! let mut points = Vec::with_capacity(100);
//! points.extend([10, 20, 30]);
//! let first = &points[0];
//! points.push(40);
//! println!("{first}");
//! ```
//!
//! 先完成引用的最后一次使用，再 push，可以通过编译：
//!
//! ```
//! let mut points = vec![10, 20, 30];
//! let first = &points[0];
//! assert_eq!(*first, 10); // first 的最后一次使用。
//! points.push(40);
//! assert_eq!(points, [10, 20, 30, 40]);
//! ```

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct User {
    pub name: String,
    pub tags: Vec<String>,
}

/// 创建新 Vec，并对每个元素调用 Clone；是否共享内部数据取决于 T。
pub fn snapshot<T: Clone>(values: &[T]) -> Vec<T> {
    values.to_vec()
}

/// 修改调用方持有的 Vec，包括其长度；仅需修改元素时应优先接收 &mut [T]。
pub fn append_point(points: &mut Vec<i32>, point: i32) {
    points.push(point);
}

#[cfg(test)]
mod tests {
    use super::{User, append_point, snapshot};
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn moving_vec_preserves_its_buffer() {
        let original = vec![1, 2, 3];
        let address = original.as_ptr();
        let owner = original;
        assert_eq!(address, owner.as_ptr());
        assert_eq!(owner, [1, 2, 3]);
    }

    #[test]
    fn borrowed_slice_edits_owner_then_borrow_can_end() {
        let mut owner = vec![1, 2, 3];
        let view = &mut owner[..2];
        view[0] = 9;
        assert_eq!(view, [9, 2]);
        owner.push(4);
        assert_eq!(owner, [9, 2, 3, 4]);
    }

    #[test]
    fn push_with_spare_capacity_preserves_buffer() {
        let mut points = Vec::with_capacity(8);
        points.extend([1, 2]);
        let address = points.as_ptr();
        let capacity = points.capacity();
        points.push(3);
        assert_eq!(points.as_ptr(), address);
        assert_eq!(points.capacity(), capacity);
        assert_eq!(points, [1, 2, 3]);
    }

    #[test]
    fn push_when_full_increases_capacity_and_preserves_elements() {
        let mut points = vec![1, 2, 3];
        points.resize(points.capacity(), 0);
        let before = points.clone();
        let old_capacity = points.capacity();
        points.push(9);
        assert!(points.capacity() > old_capacity);
        assert_eq!(&points[..before.len()], before.as_slice());
        assert_eq!(points.last(), Some(&9));
        // 不断言地址改变或固定增长倍数。
    }

    #[test]
    fn snapshot_of_ints_is_independent_including_empty_input() {
        for original in [vec![], vec![1], vec![1, 2, 3]] {
            let mut copied = snapshot(&original);
            assert_eq!(original, copied);
            if let Some(first) = copied.first_mut() {
                *first = 99;
                assert_eq!(original[0], 1);
            }
            copied.push(7);
            assert_eq!(copied.len(), original.len() + 1);
        }
    }

    #[test]
    fn cloning_owned_fields_and_shared_handles_have_different_results() {
        let original = vec![User {
            name: "Alice".into(),
            tags: vec!["java".into()],
        }];
        let mut copied = snapshot(&original);
        copied[0].name = "Bob".into();
        copied[0].tags[0] = "rust".into();
        assert_eq!(original[0].name, "Alice");
        assert_eq!(original[0].tags, ["java"]);

        let shared = vec![Rc::new(Cell::new(10))];
        let handles = snapshot(&shared);
        handles[0].set(99);
        assert!(Rc::ptr_eq(&shared[0], &handles[0]));
        assert_eq!(shared[0].get(), 99);
    }

    #[test]
    fn mutable_vec_parameter_updates_caller_length() {
        let mut points = vec![10, 20];
        append_point(&mut points, 30);
        assert_eq!(points, [10, 20, 30]);
    }
}
