fn main() {
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    println!("cargo:rerun-if-changed=build.rs");
    let commit = std::env::var("GITHUB_SHA").unwrap_or_default();
    println!("cargo:rustc-env=ZLOG_COMMIT={}", commit.trim());
    println!(
        "cargo:rustc-env=ZLOG_BUILT={}",
        chrono::Utc::now().format("%Y-%m-%d %H:%M UTC")
    );
}
