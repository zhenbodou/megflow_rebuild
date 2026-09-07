//! 不依赖任何第三方 crate：把“值 → 信封 → 业务变换 → 类型擦除”逐步连接起来。
use flow_message::{Envelope, EnvelopeInfo};

// ANCHOR: business_only
fn label(value: u32) -> String {
    format!("frame-{value}")
}
// ANCHOR_END: business_only

// ANCHOR: preserve_context
fn label_message(mut input: Envelope<u32>) -> Envelope<String> {
    let value = input.unpack();
    let output = label(value);
    input.repack(output)
}
// ANCHOR_END: preserve_context

fn main() {
    assert_eq!(label(7), "frame-7");

    // ANCHOR: example_input
    let input = Envelope::with_info(
        7u32,
        EnvelopeInfo {
            partial_id: Some(42),
            ..Default::default()
        },
    );
    let mut output = label_message(input);
    assert_eq!(output.info().partial_id, Some(42));
    assert_eq!(output.unpack(), "frame-7");
    assert!(output.is_none());
    // ANCHOR_END: example_input

    // ANCHOR: erased_payload
    let message = Envelope::new(String::from("frame-8"));
    let mut erased = message.seal();
    assert!(erased.downcast_ref::<Envelope<u32>>().is_none());
    let concrete = erased
        .downcast_mut::<Envelope<String>>()
        .expect("真实载荷是 String");
    assert_eq!(concrete.unpack(), "frame-8");
    // ANCHOR_END: erased_payload
    println!("完成：业务转换、元信息保留、取出载荷、类型擦除与安全恢复");
}
