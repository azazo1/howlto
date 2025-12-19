use crate::error::Result;

const INIT: &str = include_str!("init.fish");

pub fn script_init() -> Result<String> {
    // shell 函数 __howlto_invoke
    Ok(INIT.replace(
        "__howlto_path__",
        std::env::current_exe()
            .or_else(|e| {
                if let Some(p) = std::env::args().next() {
                    Ok(p.into())
                } else {
                    Err(e)
                }
            })?
            .to_string_lossy()
            .as_ref(),
    ))
}
