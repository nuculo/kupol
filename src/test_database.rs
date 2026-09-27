// test_database.rs

pub fn raw_query(query: &str) {
    println!("Executing unsafe: {}", query);
}

pub fn prepare_query(query: &str, params: &[&str]) {
    println!("Executing safe: {} with {:?}", query, params);
}
