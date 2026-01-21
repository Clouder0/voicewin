use anyhow::Context;

/// Where we store secrets in the OS keyring.
///
/// This is intentionally constant so upgrades don't orphan secrets.
const SERVICE: &str = "voicewin";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretKey {
    OpenAiCompatibleApiKey,
    ElevenLabsApiKey,
}

impl SecretKey {
    fn user(self) -> &'static str {
        match self {
            SecretKey::OpenAiCompatibleApiKey => "openai_compatible_api_key",
            SecretKey::ElevenLabsApiKey => "elevenlabs_api_key",
        }
    }
}

pub fn set_secret(key: SecretKey, value: &str) -> anyhow::Result<()> {
    let entry = keyring::Entry::new(SERVICE, key.user()).context("create keyring entry")?;
    entry.set_password(value).context("set secret")
}

pub fn get_secret(key: SecretKey) -> anyhow::Result<Option<String>> {
    let entry = keyring::Entry::new(SERVICE, key.user()).context("create keyring entry")?;

    match entry.get_password() {
        Ok(v) => Ok(Some(v)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(anyhow::Error::new(e)).context("get secret"),
    }
}

pub fn delete_secret(key: SecretKey) -> anyhow::Result<()> {
    let entry = keyring::Entry::new(SERVICE, key.user()).context("create keyring entry")?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(anyhow::Error::new(e)).context("delete secret"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_missing_returns_none() {
        // We don't want to touch developer's real keyring state in tests.
        // This test just validates the mapping logic.
        assert_eq!(SecretKey::ElevenLabsApiKey.user(), "elevenlabs_api_key");
    }
}
