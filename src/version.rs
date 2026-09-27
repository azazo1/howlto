// 与 build.rs 配套使用: 常量由 build.rs 注入, 缺少 build.rs 会编译失败.

/// 当前构建应当显示的版本号, 由 build.rs 注入, 日常开发构建为 dev-build.
pub const BUILD_VERSION: &str = env!("PROJECT_BUILD_VERSION");

/// 返回当前构建应当显示的版本号.
pub fn version() -> &'static str {
    BUILD_VERSION
}
