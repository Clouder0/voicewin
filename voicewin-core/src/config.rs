use crate::enhancement::PromptTemplate;
use crate::power_mode::{GlobalDefaults, PowerModeProfile};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppConfig {
    pub defaults: GlobalDefaults,
    pub profiles: Vec<PowerModeProfile>,
    pub prompts: Vec<PromptTemplate>,

    // Secrets are stored outside this struct at rest.
    #[serde(default)]
    pub llm_api_key_present: bool,
}
