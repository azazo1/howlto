use crate::error::{Error, Result};

pub const CURRENT_CONFIG_VERSION: u32 = 1;

pub fn ensure_supported_version(version: u32) -> Result<()> {
    if version > CURRENT_CONFIG_VERSION {
        Err(Error::ConfigVersion {
            version,
            current: CURRENT_CONFIG_VERSION,
        })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_version_is_supported() {
        assert!(ensure_supported_version(CURRENT_CONFIG_VERSION).is_ok());
    }

    #[test]
    fn future_version_is_rejected() {
        let error = ensure_supported_version(CURRENT_CONFIG_VERSION + 1).unwrap_err();
        assert!(error.to_string().contains("newer than supported"));
    }
}
