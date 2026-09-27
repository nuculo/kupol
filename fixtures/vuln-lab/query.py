# f-string SQL (common pattern). The current engine SQL rules are Rust format!-based;
# leaked.py + query.rs are the findings the v1.0 plugins will flag.
def fetch(user_id):
    return f"SELECT * FROM users WHERE id = {user_id}"
