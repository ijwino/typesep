#[test]
fn nothing_prints_to_stdout_except_through_write_out() {
    let source = include_str!("../src/main.rs");
    let macro_call = regex::Regex::new(r"(^|[^a-z_])(println!|print!)").expect("regex");
    let mut found: Vec<String> = Vec::new();
    for (line, text) in source.lines().enumerate() {
        let code = text.split("//").next().unwrap_or("");
        if macro_call.is_match(code) {
            found.push(format!("main.rs:{}: {}", line + 1, text.trim()));
        }
    }
    assert!(
        found.is_empty(),
        "use outln! or out!, which survive a closed pipe:\n{}",
        found.join("\n")
    );
}
