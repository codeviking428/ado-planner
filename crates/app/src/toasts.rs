use std::time::{Duration, Instant};

use serde_json::Value;

const TOAST_CAP: usize = 2;
const SUCCESS_TTL: Duration = Duration::from_secs(10);
const UNDO_TTL: Duration = Duration::from_secs(12);
const WARNING_TTL: Duration = Duration::from_secs(10);
const ERROR_TTL: Duration = Duration::from_secs(12);

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
    RemoveLink {
        successor_id: i64,
        predecessor_id: i64,
    },
    RestoreLink {
        successor_id: i64,
        predecessor_id: i64,
    },
    DeleteCreated {
        id: i64,
        title: String,
        work_item_type: String,
        placement: String,
    },
    RestoreFields {
        id: i64,
        fields: Vec<(String, Value)>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryEntry {
    Dates {
        id: i64,
        before_start: Option<String>,
        before_target: Option<String>,
        after_start: Option<String>,
        after_target: Option<String>,
    },
    Link {
        successor_id: i64,
        predecessor_id: i64,
        linked: bool,
    },
    Created {
        id: i64,
        title: String,
        work_item_type: String,
        placement: String,
    },
    Fields {
        id: i64,
        changes: Vec<(String, Value, Value)>,
    },
}

#[derive(Debug, Clone, Default)]
pub struct EditHistory {
    pub undo: Vec<HistoryEntry>,
    pub redo: Vec<HistoryEntry>,
}

impl EditHistory {
    const CAP: usize = 50;

    pub fn push(&mut self, entry: HistoryEntry) {
        self.undo.push(entry);
        self.redo.clear();
        if self.undo.len() > Self::CAP {
            let extra = self.undo.len() - Self::CAP;
            self.undo.drain(0..extra);
        }
    }

    pub fn pop_undo(&mut self) -> Option<HistoryEntry> {
        self.undo.pop()
    }

    pub fn pop_redo(&mut self) -> Option<HistoryEntry> {
        self.redo.pop()
    }
}

/// Extra toast action offered after a parent bar move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToastExtra {
    MoveChildren { parent_id: i64, delta_days: i32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    pub id: u64,
    pub kind: ToastKind,
    pub message: String,
    pub copyable: bool,
    pub undo: Option<UndoAction>,
    pub extra: Option<ToastExtra>,
    pub created_at: Instant,
}

#[derive(Debug, Clone, Default)]
pub struct Toasts {
    next_id: u64,
    pub items: Vec<Toast>,
    /// Hovered toasts do not expire until the pointer leaves.
    pub paused: bool,
}

impl Toasts {
    fn dismiss_success(&mut self) {
        self.items.retain(|toast| toast.kind != ToastKind::Success);
    }

    pub fn push(&mut self, kind: ToastKind, message: impl Into<String>) -> u64 {
        if kind == ToastKind::Success {
            self.dismiss_success();
        }
        self.next_id += 1;
        let copyable = kind == ToastKind::Error;
        self.items.push(Toast {
            id: self.next_id,
            kind,
            message: message.into(),
            copyable,
            undo: None,
            extra: None,
            created_at: Instant::now(),
        });
        self.cap();
        self.next_id
    }

    pub fn dismiss_undo(&mut self) {
        self.items.retain(|toast| toast.undo.is_none());
    }

    pub fn dismiss_undo_for_item(&mut self, id: i64) {
        self.items.retain(|toast| match &toast.undo {
            Some(UndoAction::DeleteCreated { id: item, .. })
            | Some(UndoAction::RestoreDates { id: item, .. })
            | Some(UndoAction::RestoreFields { id: item, .. }) => *item != id,
            Some(UndoAction::RemoveLink {
                successor_id,
                predecessor_id,
            })
            | Some(UndoAction::RestoreLink {
                successor_id,
                predecessor_id,
            }) => *successor_id != id && *predecessor_id != id,
            None => true,
        });
    }

    pub fn push_undo(&mut self, message: impl Into<String>, undo: UndoAction) -> u64 {
        self.push_undo_extra(message, undo, None)
    }

    pub fn push_undo_extra(
        &mut self,
        message: impl Into<String>,
        undo: UndoAction,
        extra: Option<ToastExtra>,
    ) -> u64 {
        self.dismiss_success();
        self.next_id += 1;
        self.items.push(Toast {
            id: self.next_id,
            kind: ToastKind::Success,
            message: message.into(),
            copyable: false,
            undo: Some(undo),
            extra,
            created_at: Instant::now(),
        });
        self.cap();
        self.next_id
    }

    pub fn take(&mut self, id: u64) -> Option<Toast> {
        let index = self.items.iter().position(|toast| toast.id == id)?;
        Some(self.items.remove(index))
    }

    pub fn dismiss(&mut self, id: u64) {
        let _ = self.take(id);
    }

    pub fn expire(&mut self) {
        if self.items.is_empty() {
            self.paused = false;
            return;
        }
        if self.paused {
            return;
        }
        let now = Instant::now();
        self.items.retain(|toast| match toast.kind {
            ToastKind::Error => now.saturating_duration_since(toast.created_at) < ERROR_TTL,
            ToastKind::Success => {
                let ttl = if toast.undo.is_some() {
                    UNDO_TTL
                } else {
                    SUCCESS_TTL
                };
                now.saturating_duration_since(toast.created_at) < ttl
            }
            ToastKind::Warning => now.saturating_duration_since(toast.created_at) < WARNING_TTL,
        });
        self.cap();
    }

    fn cap(&mut self) {
        if self.items.len() > TOAST_CAP {
            let extra = self.items.len() - TOAST_CAP;
            self.items.drain(0..extra);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_undo_toast_replaces_the_previous_one() {
        let mut toasts = Toasts::default();
        toasts.push_undo(
            "Moved A",
            UndoAction::RestoreDates {
                id: 1,
                start_date: None,
                target_date: None,
                rev: 1,
            },
        );
        toasts.push(ToastKind::Success, "Undone: A dates");
        toasts.push_undo(
            "Moved B",
            UndoAction::RestoreDates {
                id: 2,
                start_date: None,
                target_date: None,
                rev: 1,
            },
        );
        assert_eq!(
            toasts.items.len(),
            1,
            "a new action must drop the stale Undid toast, got {:?}",
            toasts
                .items
                .iter()
                .map(|toast| toast.message.as_str())
                .collect::<Vec<_>>()
        );
        assert_eq!(toasts.items[0].message, "Moved B");
        assert!(toasts.items[0].undo.is_some());
    }

    #[test]
    fn undo_toasts_expire_after_ttl() {
        let mut toasts = Toasts::default();
        toasts.push_undo(
            "Cleared dates",
            UndoAction::RestoreDates {
                id: 1,
                start_date: None,
                target_date: None,
                rev: 1,
            },
        );
        toasts.items[0].created_at = Instant::now() - Duration::from_secs(20);
        toasts.expire();
        assert!(toasts.items.is_empty());
    }

    #[test]
    fn dismiss_undo_for_item_drops_create_toast() {
        let mut toasts = Toasts::default();
        toasts.push_undo(
            "Created Cart",
            UndoAction::DeleteCreated {
                id: 7,
                title: "Cart".into(),
                work_item_type: "Feature".into(),
                placement: "at the root".into(),
            },
        );
        toasts.push(ToastKind::Success, "Saved");
        toasts.dismiss_undo_for_item(7);
        assert!(
            toasts
                .items
                .iter()
                .all(|toast| !matches!(toast.undo, Some(UndoAction::DeleteCreated { .. }))),
            "deleting an item must drop its create-Undo toast"
        );
        assert!(
            toasts.items.iter().any(|toast| toast.message == "Saved"),
            "unrelated toasts must stay"
        );
    }

    #[test]
    fn hover_pauses_expiry() {
        let mut toasts = Toasts::default();
        toasts.push(ToastKind::Success, "Saved");
        toasts.items[0].created_at = Instant::now() - Duration::from_secs(20);
        toasts.paused = true;
        toasts.expire();
        assert_eq!(toasts.items.len(), 1, "hovered toasts must not expire");
        toasts.paused = false;
        toasts.expire();
        assert!(toasts.items.is_empty());
    }

    #[test]
    fn error_toasts_expire_after_ttl() {
        let mut toasts = Toasts::default();
        toasts.push(ToastKind::Error, "rev mismatch");
        toasts.items[0].created_at = Instant::now() - Duration::from_secs(20);
        toasts.expire();
        assert!(
            toasts.items.is_empty(),
            "error toasts must not stick forever"
        );
    }

    #[test]
    fn empty_list_clears_pause() {
        let mut toasts = Toasts {
            paused: true,
            ..Default::default()
        };
        toasts.expire();
        assert!(
            !toasts.paused,
            "an empty toast stack must not stay paused after the hovered toast is gone"
        );
    }
}
