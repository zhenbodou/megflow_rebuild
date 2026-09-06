//! 这些小程序应该编译失败；对应 .stderr 固定用户可见的错误及定位。
#[test]
fn invalid_macro_inputs_report_source_diagnostics() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/*.rs");
}
