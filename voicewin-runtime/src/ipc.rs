use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunSessionRequest {
    // Reserved for future use (e.g., manual transcript override / debug).
    pub transcript: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordingStatus {
    pub is_recording: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToggleRecordingResponse {
    pub status: RecordingStatus,
    pub stage: String,
    pub final_text: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunSessionResponse {
    pub stage: String,
    pub final_text: Option<String>,
    pub error: Option<String>,
}
