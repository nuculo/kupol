# Engine SQL plugin matches format!("SELECT …"), not a Python f-string.
fn lookup(name: &str) -> String {
    format!("SELECT * FROM accounts WHERE name = {}", name)
}
