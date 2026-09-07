// 内部规则放在入口规则之前，避免入口重新吞掉内部调用。
macro_rules! settings {
    (@parse $out:ident;) => {};
    (@parse $out:ident; $name:ident = $value:expr; $($rest:tt)*) => {
        $out.push((stringify!($name), $value));
        settings!(@parse $out; $($rest)*);
    };
    () => { Vec::new() };
    ($($tokens:tt)*) => {{
        let mut output = Vec::new();
        settings!(@parse output; $($tokens)*);
        output
    }};
}
macro_rules! classify {
    (3) => {
        "literal token"
    };
    ($value:expr) => {
        "opaque expression"
    };
}
macro_rules! forward_expr {
    ($e:expr) => {
        classify!($e)
    };
}
macro_rules! forward_tt {
    ($t:tt) => {
        classify!($t)
    };
}
fn main() {
    let mut calls = 0;
    let result = settings! { capacity = { calls += 1; 8 }; workers = 2; };
    assert_eq!(result, vec![("capacity", 8), ("workers", 2)]);
    assert_eq!(calls, 1);
    let empty: Vec<(&str, i32)> = settings!();
    assert!(empty.is_empty());
    assert_eq!(forward_expr!(3), "opaque expression");
    assert_eq!(forward_tt!(3), "literal token");
    println!("声明宏进阶：递归、单次求值、片段转发全部通过");
}
