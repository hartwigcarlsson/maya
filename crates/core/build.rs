// `sqlx::migrate!` bäddar in migreringarna; en ny fil ska ge en ny kompilering.
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}
