use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Success,
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UndoAction {
    RestoreDates {
        id: i64,
        start_date: Option<String>,
        target_date: Option<String>,
        rev: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    pub id: u64,
    pub kind: ToastKind,
    pub message: String,
    pub copyable: bool,
    pub undo: Option<UndoAction>,
}

#[derive(Debug, Clone, Default)]
pub struct Toasts {
    next_id: u64,
    pub items: Vec<Toast>,
}

impl Toasts {
    pub fn push(&mut self, kind: ToastKind, message: impl Into<String>) -> u64 {
        self.next_id += 1;
        let copyable = kind == ToastKind::Error;
        self.items.push(Toast {
            id: self.next_id,
            kind,
            message: message.into(),
            copyable,
            undo: None,
        });
        self.next_id
    }

    pub fn push_undo(&mut self, message: impl Into<String>, undo: UndoAction) -> u64 {
        self.next_id += 1;
        self.items.push(Toast {
            id: self.next_id,
            kind: ToastKind::Success,
            message: message.into(),
            copyable: false,
            undo: Some(undo),
        });
        self.next_id
    }

    pub fn take(&mut self, id: u64) -> Option<Toast> {
        let index = self.items.iter().position(|toast| toast.id == id)?;
        Some(self.items.remove(index))
    }

    pub fn last_error_message(&self) -> Option<&str> {
        self.items
            .iter()
            .rev()
            .find(|toast| toast.kind == ToastKind::Error)
            .map(|toast| toast.message.as_str())
    }

    pub fn last_message(&self) -> Option<&str> {
        self.items.last().map(|toast| toast.message.as_str())
    }
}

pub fn error_text(error: &dyn std::fmt::Display, fallback: &str) -> String {
    let text = error.to_string();
    if text.is_empty() {
        fallback.to_string()
    } else {
        text
    }
}

pub fn json_error_message(value: &Value) -> Option<String> {
    value
        .get("message")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}
