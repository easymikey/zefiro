#[test]
fn rejected_programs_do_not_compile() {
    trybuild::TestCases::new().compile_fail("tests/compile_fail/*.rs");
}
