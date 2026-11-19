fn main() {
    // sqlx::migrate! embeds the migrations directory at compile time.
    println!("cargo:rerun-if-changed=migrations");
}
