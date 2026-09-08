use flow_derive::add_cvt_func;
#[add_cvt_func]
async fn convert(value: u32) -> u32 { value }
fn main() {}
