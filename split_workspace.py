import os
import shutil

kan_dir = "duo-kan"
src_kan = f"{kan_dir}/src"

os.makedirs(src_kan, exist_ok=True)

# 1. Create duo-kan/Cargo.toml
cargo_toml_kan = """[package]
name = "duo-kan"
version = "0.1.0"
edition = "2021"

[dependencies]
anyhow = "1.0.102"
arc-swap = "1.8.2"
async-trait = "0.1.89"
chrono = { version = "0.4", features = ["serde"] }
petgraph = "0.8.3"
rand = "0.9"
rayon = "1.10"
serde = { version = "1.0.228", features = ["derive"] }
serde_json = "1.0.149"
serde_yaml = "0.9.34"
tokio = { version = "1.50.0", features = ["full", "sync", "time", "macros", "rt-multi-thread"] }
tracing = "0.1.44"
"""
with open(f"{kan_dir}/Cargo.toml", "w") as f:
    f.write(cargo_toml_kan)

# 2. Move files
items_to_move = [
    "kan",
    "kan_intelligence",
    "scan",
    "models.rs",
    "protocol.rs",
    "policy_engine.rs",
    "strategy.rs",
    "crossover.rs",
    "babylonian.rs",
    "blast_radius.rs",
    "semantic_engine.rs",
    "fetcher.rs" # context fetcher for codebase (GraphKAN)
]

for item in items_to_move:
    src_path = f"src/{item}"
    dst_path = f"{src_kan}/{item}"
    if os.path.exists(src_path):
        shutil.move(src_path, dst_path)

# 3. Create lib.rs for duo-kan
lib_rs = """pub mod kan;
pub mod kan_intelligence;
pub mod scan;
pub mod models;
pub mod protocol;
pub mod policy_engine;
pub mod strategy;
pub mod crossover;
pub mod babylonian;
pub mod blast_radius;
pub mod semantic_engine;
pub mod fetcher;

pub use models::*;
pub use protocol::*;
"""
with open(f"{src_kan}/lib.rs", "w") as f:
    f.write(lib_rs)

# 4. Update parent Cargo.toml
with open("Cargo.toml", "r") as f:
    parent_cargo = f.read()

if "duo-kan = " not in parent_cargo:
    parent_cargo = parent_cargo.replace("[dependencies]", "[dependencies]\nduo-kan = { path = \"duo-kan\" }")
if "[workspace]" not in parent_cargo:
    parent_cargo += "\n[workspace]\nmembers = [\"duo-kan\"]\n"

with open("Cargo.toml", "w") as f:
    f.write(parent_cargo)

print("Workspace split setup complete.")
