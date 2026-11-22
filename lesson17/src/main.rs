use lesson17::{User, append_point, snapshot};
use std::cell::Cell;
use std::rc::Rc;

fn main() {
    println!("Lesson 17: Vec memory experiments");
    assignment();
    borrowed_views();
    push_and_capacity();
    owned_copy();
    clone_elements();
    function_boundary();
}

fn assignment() {
    println!("\n[1] Copy an integer array; move a Vec owner");
    let array = [10, 20, 30];
    let mut array_copy = array; // [i32; 3] 实现了 Copy。
    array_copy[0] = 99;
    println!("array={array:?} array_copy={array_copy:?}");

    let original = vec![10, 20, 30];
    let buffer_before_move = original.as_ptr();
    let mut owner = original;
    owner[0] = 99;
    show("owner", &owner);
    println!(
        "same buffer after move: {}",
        buffer_before_move == owner.as_ptr()
    );
    // println!("{original:?}"); // E0382：原变量已被移动。
}

fn borrowed_views() {
    println!("\n[2] Borrow slices without copying elements");
    let mut points = vec![10, 20, 30];
    let prefix: &[i32] = &points[..2];
    println!(
        "prefix={prefix:?} len={} first={:p}",
        prefix.len(),
        prefix.as_ptr()
    );
    println!(
        "same first element: {}",
        prefix.as_ptr() == points.as_ptr()
    );
    // 上一行是 prefix 的最后一次使用，之后可以开始可变借用。

    let writable: &mut [i32] = &mut points[..2];
    writable[0] = 99;
    println!("writable={writable:?}");
    // writable 的最后一次使用已经结束。
    points.push(40);
    show("owner", &points);
}

fn push_and_capacity() {
    println!("\n[3] Push with spare capacity, then push when full");
    let mut spare = Vec::with_capacity(5);
    spare.extend([10, 20, 30]);
    let before = spare.as_ptr();
    spare.push(40);
    show("spare", &spare);
    println!(
        "same buffer with spare capacity: {}",
        before == spare.as_ptr()
    );

    let mut full = vec![10, 20, 30];
    full.resize(full.capacity(), 0); // 使用实际 capacity，保证接下来必定扩容。
    let old_capacity = full.capacity();
    // 仅保存地址数值作观察；扩容后不通过旧指针访问内存。
    let old_address = full.as_ptr() as usize;
    full.push(40);
    show("grown", &full);
    println!("capacity: {old_capacity} -> {}", full.capacity());
    println!(
        "address: 0x{old_address:x} -> {:p} (may stay the same)",
        full.as_ptr()
    );
    println!("Do not assume a fixed growth factor or a changed address");
}

fn owned_copy() {
    println!("\n[4] Turn a borrowed prefix into an independently owned Vec");
    let base = vec![10, 20, 30];
    let mut independent = snapshot(&base[..2]);
    independent[0] = 99;
    independent.push(77);
    show("base", &base);
    show("copy", &independent);
    println!("the i32 elements are independent immediately after to_vec");
}

fn clone_elements() {
    println!("\n[5] Clone behavior depends on the element type");
    let original = vec![User {
        name: "Alice".into(),
        tags: vec!["java".into()],
    }];
    let mut copied = snapshot(&original);
    copied[0].name = "Bob".into();
    copied[0].tags[0] = "rust".into();
    println!("owned fields: original={original:?} copy={copied:?}");

    // Rc 是共享所有权句柄；Cell 在这个单线程例子中允许修改共享整数。
    let shared = vec![Rc::new(Cell::new(10))];
    let handles = snapshot(&shared);
    handles[0].set(99);
    println!(
        "Rc handles: original={} copy={} same allocation={}",
        shared[0].get(),
        handles[0].get(),
        Rc::ptr_eq(&shared[0], &handles[0])
    );
}

fn function_boundary() {
    println!("\n[6] Borrow the Vec mutably when the caller's length should change");
    let mut points = vec![10, 20];
    append_point(&mut points, 77);
    show("caller", &points);

    let mut independent = snapshot(&points);
    independent[0] = -1;
    println!("int snapshot: original={points:?} independent={independent:?}");
}

// 此处为观察 capacity 接收 &Vec；一般只读业务函数优先接收 &[i32]。
fn show(label: &str, values: &Vec<i32>) {
    println!(
        "{label:10} len={} cap={} first={:p} values={values:?}",
        values.len(),
        values.capacity(),
        values.as_ptr()
    );
}
