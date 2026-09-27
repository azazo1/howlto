fn main() {
    println!("cargo:rerun-if-env-changed=PROJECT_BUILD_VERSION");

    let version =
        std::env::var("PROJECT_BUILD_VERSION").unwrap_or_else(|_| "dev-build".to_string());
    println!("cargo:rustc-env=PROJECT_BUILD_VERSION={version}");
}
