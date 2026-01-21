use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProfileId(pub Uuid);

impl ProfileId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PromptId(pub Uuid);

impl PromptId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExePath(pub String);

impl ExePath {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProcessName(pub String);

impl ProcessName {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WindowTitle(pub String);

impl WindowTitle {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppIdentity {
    pub exe_path: Option<ExePath>,
    pub process_name: Option<ProcessName>,
    pub window_title: Option<WindowTitle>,
}

impl AppIdentity {
    pub fn new() -> Self {
        Self {
            exe_path: None,
            process_name: None,
            window_title: None,
        }
    }

    pub fn with_exe_path(mut self, exe_path: impl Into<String>) -> Self {
        self.exe_path = Some(ExePath::new(exe_path));
        self
    }

    pub fn with_process_name(mut self, process_name: impl Into<String>) -> Self {
        self.process_name = Some(ProcessName::new(process_name));
        self
    }

    pub fn with_window_title(mut self, window_title: impl Into<String>) -> Self {
        self.window_title = Some(WindowTitle::new(window_title));
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InsertMode {
    Paste,
    PasteAndEnter,
}
