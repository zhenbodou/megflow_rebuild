// 本文件是宏入门章的完整可运行源码；不依赖过程宏。
macro_rules! twice {
    ($value:expr) => {{
        let value = $value;
        value + value
    }};
}

macro_rules! port_names {
    ($($name:ident),* $(,)?) => {
        &[$(stringify!($name)),*]
    };
}

macro_rules! node_struct {
    ($visibility:vis $name:ident { $($field:ident : $ty:ty),* $(,)? }) => {
        $visibility struct $name {
            $(pub $field: $ty),*
        }
    };
}

node_struct!(pub Settings { capacity: usize, label: String });

fn main() {
    let mut calls = 0;
    let result = twice!({
        calls += 1;
        3
    });
    assert_eq!((result, calls), (6, 1));
    let names: &[&str] = port_names!(inp, out,);
    assert_eq!(names, &["inp", "out"]);
    let empty: &[&str] = port_names!();
    assert!(empty.is_empty());
    let settings = Settings {
        capacity: 16,
        label: "video".into(),
    };
    assert_eq!(settings.capacity, 16);
    assert_eq!(settings.label, "video");
    println!("宏入门：三组断言通过");
}
