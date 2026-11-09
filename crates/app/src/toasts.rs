use std::time::{Duration, Instant};

use serde_json::Value;

const TOAST_CAP: usize = 2;
const TOAST_TTL: Duration = Duration::from_secs(6);
/// Undo/Redo toasts share the same timeout as Saved / error toasts.
const UNDO_TOAST_TTL: Duration = TOAST_TTL;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Success,
    Error,
    Warning,
}

/// One item in a toast Undo that restores dates (parent move + children).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DateRestore {
    pub id: i64,
    pub start_date: Option<String>,
    pub target_date: Option<String>,
    pub rev: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UndoAction {
    RestoreDates {
        id: i64,
        start_date: Option<String>,
        target_date: Option<String>,
        rev: i64,
    },
    /// Parent move plus "Move children too" as one Undo.
    RestoreDatesBatch { items: Vec<DateRestore> },
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
    /// Undo whatever is currently on the history stack (toast after a prior undo).
    StackUndo,
    /// Redo whatever is currently on the redo stack.
    StackRedo,
}

/// One row in a batched Dates history entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatesChange {
    pub id: i64,
    pub before_start: Option<String>,
    pub before_target: Option<String>,
    pub after_start: Option<String>,
    pub after_target: Option<String>,
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
    /// Parent bar move folded with descendant shifts (one Ctrl+Z).
    DatesGroup { items: Vec<DatesChange> },
    Link {
        successor_id: i64,
        predecessor_id: i64,
        linked: bool,
        /// Date cascade moves an added link made, undone with it.
        dates: Vec<DatesChange>,
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
    pub redo: Option<UndoAction>,
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
        let message = message.into();
        if kind == ToastKind::Success {
            self.dismiss_success();
        } else if let Some(index) = self
            .items
            .iter()
            .position(|toast| toast.kind == kind && toast.message == message)
        {
            let mut toast = self.items.remove(index);
            toast.created_at = Instant::now();
            self.items.push(toast);
            return self.items.last().map(|toast| toast.id).unwrap_or(0);
        }
        self.next_id += 1;
        let copyable = kind == ToastKind::Error;
        self.items.push(Toast {
            id: self.next_id,
            kind,
            message,
            copyable,
            undo: None,
            redo: None,
            extra: None,
            created_at: Instant::now(),
        });
        self.cap();
        self.next_id
    }

    pub fn dismiss_undo(&mut self) {
        self.items
            .retain(|toast| toast.undo.is_none() && toast.redo.is_none());
    }

    pub fn dismiss_undo_for_item(&mut self, id: i64) {
        self.items.retain(|toast| match &toast.undo {
            Some(UndoAction::DeleteCreated { id: item, .. })
            | Some(UndoAction::RestoreDates { id: item, .. })
            | Some(UndoAction::RestoreFields { id: item, .. }) => *item != id,
            Some(UndoAction::RestoreDatesBatch { items }) => items.iter().all(|item| item.id != id),
            Some(UndoAction::RemoveLink {
                successor_id,
                predecessor_id,
            })
            | Some(UndoAction::RestoreLink {
                successor_id,
                predecessor_id,
            }) => *successor_id != id && *predecessor_id != id,
            Some(UndoAction::StackUndo | UndoAction::StackRedo) | None => true,
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
            redo: None,
            extra,
            created_at: Instant::now(),
        });
        self.cap();
        self.next_id
    }

    /// Success toast that can offer Undo and/or Redo from remaining history.
    pub fn push_stack(
        &mut self,
        message: impl Into<String>,
        undo: Option<UndoAction>,
        redo: Option<UndoAction>,
    ) -> u64 {
        self.dismiss_success();
        self.next_id += 1;
        self.items.push(Toast {
            id: self.next_id,
            kind: ToastKind::Success,
            message: message.into(),
            copyable: false,
            undo,
            redo,
            extra: None,
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

    pub fn clear(&mut self) {
        self.items.clear();
        self.paused = false;
    }

    fn ttl(toast: &Toast) -> Duration {
        if toast.undo.is_some() || toast.redo.is_some() {
            UNDO_TOAST_TTL
        } else {
            TOAST_TTL
        }
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
        self.items
            .retain(|toast| now.saturating_duration_since(toast.created_at) < Self::ttl(toast));
        self.cap();
    }

    /// Remaining time until the next toast should dismiss. `None` while
    /// paused or empty so the UI timer can sleep.
    pub fn next_expiry_in(&self) -> Option<Duration> {
        if self.paused || self.items.is_empty() {
            return None;
        }
        let now = Instant::now();
        self.items
            .iter()
            .map(|toast| {
                Self::ttl(toast).saturating_sub(now.saturating_duration_since(toast.created_at))
            })
            .min()
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
    fn a_duplicate_warning_replaces_the_stacked_copy() {
        let mut toasts = Toasts::default();
        toasts.push(ToastKind::Warning, "Link already exists");
        toasts.push(ToastKind::Warning, "Link already exists");
        assert_eq!(
            toasts.items.len(),
            1,
            "a second identical warning must replace, not stack, got {:?}",
            toasts
                .items
                .iter()
                .map(|toast| toast.message.as_str())
                .collect::<Vec<_>>()
        );
        assert_eq!(toasts.items[0].message, "Link already exists");
    }

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
    fn success_toasts_expire_after_five_seconds() {
        let mut toasts = Toasts::default();
        toasts.push(ToastKind::Success, "Saved");
        toasts.items[0].created_at = Instant::now() - (TOAST_TTL - Duration::from_millis(500));
        toasts.expire();
        assert_eq!(
            toasts.items.len(),
            1,
            "a success toast must still show just before TTL"
        );
        toasts.items[0].created_at = Instant::now() - TOAST_TTL;
        toasts.expire();
        assert!(
            toasts.items.is_empty(),
            "success toasts dismiss at {TOAST_TTL:?}"
        );
    }

    #[test]
    fn undo_toasts_expire_at_the_same_ttl_as_other_toasts() {
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
        toasts.items[0].created_at = Instant::now() - (UNDO_TOAST_TTL - Duration::from_millis(500));
        toasts.expire();
        assert_eq!(
            toasts.items.len(),
            1,
            "an undo toast with an action button stays just before TTL"
        );
        toasts.items[0].created_at = Instant::now() - UNDO_TOAST_TTL;
        toasts.expire();
        assert!(
            toasts.items.is_empty(),
            "undo toasts with an action dismiss at {UNDO_TOAST_TTL:?}"
        );
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
    fn stack_toast_after_undo_offers_redo_only() {
        let mut toasts = Toasts::default();
        toasts.push_stack("Undone moved A", None, Some(UndoAction::StackRedo));
        assert_eq!(toasts.items.len(), 1);
        assert!(toasts.items[0].undo.is_none());
        assert!(matches!(toasts.items[0].redo, Some(UndoAction::StackRedo)));
        toasts.push_undo(
            "Moved B",
            UndoAction::RestoreDates {
                id: 2,
                start_date: None,
                target_date: None,
                rev: 1,
            },
        );
        assert_eq!(toasts.items.len(), 1, "a new action must replace Undone");
        assert_eq!(toasts.items[0].message, "Moved B");
        assert!(toasts.items[0].undo.is_some());
        assert!(toasts.items[0].redo.is_none());
    }

    fn assert_expires(name: &str, mut toasts: Toasts, ttl: Duration) {
        toasts.items[0].created_at = Instant::now() - (ttl - Duration::from_millis(200));
        toasts.expire();
        assert_eq!(
            toasts.items.len(),
            1,
            "{name} must still show just before TTL"
        );
        toasts.items[0].created_at = Instant::now() - ttl;
        toasts.expire();
        assert!(
            toasts.items.is_empty(),
            "{name} must auto-dismiss at {ttl:?}"
        );
    }

    #[test]
    fn every_toast_kind_expires_at_its_ttl() {
        let mut success = Toasts::default();
        success.push(ToastKind::Success, "Saved");
        assert_expires("success", success, TOAST_TTL);

        let mut warning = Toasts::default();
        warning.push(ToastKind::Warning, "Nudge");
        assert_expires("warning", warning, TOAST_TTL);

        let mut error = Toasts::default();
        error.push(ToastKind::Error, "rev mismatch");
        assert_expires("error", error, TOAST_TTL);

        let mut undo = Toasts::default();
        undo.push_undo(
            "Moved Persist cart",
            UndoAction::RestoreDates {
                id: 1,
                start_date: None,
                target_date: None,
                rev: 1,
            },
        );
        assert_expires("undo", undo, UNDO_TOAST_TTL);

        let mut redo = Toasts::default();
        redo.push_stack("Undone", None, Some(UndoAction::StackRedo));
        assert_expires("redo", redo, UNDO_TOAST_TTL);
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

    #[test]
    fn next_expiry_in_tracks_remaining_ttl() {
        let mut toasts = Toasts::default();
        assert!(
            toasts.next_expiry_in().is_none(),
            "an empty stack has no timer"
        );
        toasts.push(ToastKind::Success, "Saved");
        let remaining = toasts.next_expiry_in().expect("armed toast");
        assert!(
            remaining <= TOAST_TTL && remaining > TOAST_TTL - Duration::from_secs(1),
            "a fresh success toast must expire in about {TOAST_TTL:?}, got {remaining:?}"
        );
        toasts.paused = true;
        assert!(
            toasts.next_expiry_in().is_none(),
            "a hovered toast must not schedule idle expiry"
        );
    }
}
