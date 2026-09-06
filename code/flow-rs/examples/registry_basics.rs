// 本示例只练习 inventory。它与引擎 NodeRegistration 是不同类型的注册表。
use flow_rs::inventory;

struct Plugin {
    name: &'static str,
    run: fn(i32) -> i32,
}

inventory::collect!(Plugin);

mod double {
    use super::{inventory, Plugin};
    fn run(value: i32) -> i32 {
        value * 2
    }
    inventory::submit! { Plugin { name: "double", run } }
}

mod increment {
    use super::{inventory, Plugin};
    fn run(value: i32) -> i32 {
        value + 1
    }
    inventory::submit! { Plugin { name: "increment", run } }
}

fn main() {
    // 注册顺序没有保证，断言之前先排序。
    let mut plugins: Vec<_> = inventory::iter::<Plugin>.into_iter().collect();
    plugins.sort_by_key(|plugin| plugin.name);
    assert_eq!(plugins.len(), 2);
    let results: Vec<_> = plugins.iter().map(|p| (p.name, (p.run)(3))).collect();
    assert_eq!(results, [("double", 6), ("increment", 4)]);
    assert!(plugins.iter().all(|p| p.name != "missing"));
    println!("注册与调用通过：{results:?}");
}
