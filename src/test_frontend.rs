// test_frontend.rs
use crate::test_database::raw_query;

pub fn login_handler(user_id: String) {
    auth_service(user_id);
}

pub fn auth_service(user_id: String) {
    // 🚨 BAD QUERY INJECTED
    let query = format!("SELECT * FROM users WHERE id = {}", user_id);
    raw_query(&query);
}
