#[test]
fn must_produce_semantic_contract_fixtures() {
    let tests = trybuild::TestCases::new();
    tests.pass("tests/ui/must_produce_trait_method_pass.rs");
    tests.compile_fail("tests/ui/must_produce_string_only_fail.rs");
}
