//! 注册原理第一步：只用标准库，先不涉及 inventory、TOML 或异步调度。
// ANCHOR: operation
trait Operation {
    fn apply(&self, input: i32) -> i32;
}
struct Add { amount: i32 }
struct Multiply { factor: i32 }
impl Operation for Add { fn apply(&self, input: i32) -> i32 { input + self.amount } }
impl Operation for Multiply { fn apply(&self, input: i32) -> i32 { input * self.factor } }
// ANCHOR_END: operation

// ANCHOR: constructors
fn build_add(argument: i32) -> Box<dyn Operation> { Box::new(Add { amount: argument }) }
fn build_multiply(argument: i32) -> Box<dyn Operation> { Box::new(Multiply { factor: argument }) }
type Constructor = fn(i32) -> Box<dyn Operation>;
struct Registration {
    name: &'static str,
    constructor: Constructor,
}
static REGISTRY: &[Registration] = &[
    Registration { name: "Add", constructor: build_add },
    Registration { name: "Multiply", constructor: build_multiply },
];
// ANCHOR_END: constructors

// ANCHOR: lookup
fn build(name: &str, argument: i32) -> Result<Box<dyn Operation>, String> {
    let entry = REGISTRY.iter().find(|entry| entry.name == name)
        .ok_or_else(|| format!("unknown operation: {name}"))?;
    Ok((entry.constructor)(argument))
}
// ANCHOR_END: lookup

fn main() {
    // ANCHOR: independent_instances
    let first = build("Add", 2).unwrap();
    let second = build("Add", 9).unwrap();
    let multiply = build("Multiply", 3).unwrap();
    assert_eq!(first.apply(10), 12);
    assert_eq!(second.apply(10), 19);
    assert_eq!(multiply.apply(10), 30);
    assert!(build("Missing", 0).is_err());
    // 同一个 Vec 保存不同具体类型的操作，随后统一调用 trait 方法。
    let operations: Vec<Box<dyn Operation>> = vec![first, second, multiply];
    let result = operations.iter().fold(1, |value, operation| operation.apply(value));
    assert_eq!(result, 36); // ((1 + 2) + 9) * 3
    // ANCHOR_END: independent_instances
    println!("注册原理通过：按名查构造器、独立实例、不同类型统一调用、未知名称报错");
}
