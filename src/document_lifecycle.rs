//! Save-before-transition decisions, independent of native panels and application state.
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocumentIntent {
    New,
    OpenDialog,
    Open(PathBuf),
    Close,
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveChoice {
    Save,
    Discard,
    Cancel,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum DocumentStep {
    #[default]
    None,
    PromptSaveChanges,
    Save,
    Perform(DocumentIntent),
}

#[derive(Debug, Default)]
pub struct DocumentLifecycle {
    pending: Option<DocumentIntent>,
    awaiting_save: bool,
}

impl DocumentLifecycle {
    /// Replaces the queued transition. The UI must serialize and identify asynchronous saves.
    pub fn request(&mut self, intent: DocumentIntent, dirty: bool) -> DocumentStep {
        self.cancel();
        if dirty {
            self.pending = Some(intent);
            DocumentStep::PromptSaveChanges
        } else {
            DocumentStep::Perform(intent)
        }
    }

    pub fn respond(&mut self, choice: SaveChoice) -> DocumentStep {
        match choice {
            SaveChoice::Cancel => {
                self.cancel();
                DocumentStep::None
            }
            SaveChoice::Discard => {
                self.awaiting_save = false;
                self.pending
                    .take()
                    .map_or(DocumentStep::None, DocumentStep::Perform)
            }
            SaveChoice::Save => {
                if self.pending.is_some() && !self.awaiting_save {
                    self.awaiting_save = true;
                    DocumentStep::Save
                } else {
                    DocumentStep::None
                }
            }
        }
    }

    /// Only the active lifecycle save may complete here; unrelated saves must not call this method.
    /// Panel cancellation and write failure both keep the current document open.
    pub fn saved(&mut self, success: bool, still_dirty: bool) -> DocumentStep {
        if !self.awaiting_save {
            return DocumentStep::None;
        }
        self.awaiting_save = false;
        if !success {
            self.cancel();
            DocumentStep::None
        } else if still_dirty {
            DocumentStep::PromptSaveChanges
        } else {
            self.pending
                .take()
                .map_or(DocumentStep::None, DocumentStep::Perform)
        }
    }

    pub fn cancel(&mut self) {
        self.pending = None;
        self.awaiting_save = false;
    }

    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn intents() -> Vec<DocumentIntent> {
        vec![
            DocumentIntent::New,
            DocumentIntent::OpenDialog,
            DocumentIntent::Open(PathBuf::from("another-project.voice")),
            DocumentIntent::Close,
            DocumentIntent::Quit,
        ]
    }

    #[test]
    fn destructive_intents_wait_for_the_requested_save_to_succeed() {
        for intent in intents() {
            let mut lifecycle = DocumentLifecycle::default();
            assert_eq!(
                lifecycle.request(intent.clone(), true),
                DocumentStep::PromptSaveChanges
            );
            assert!(lifecycle.pending());
            assert_eq!(lifecycle.respond(SaveChoice::Save), DocumentStep::Save);
            assert!(lifecycle.pending());
            assert_eq!(lifecycle.respond(SaveChoice::Save), DocumentStep::None);
            assert_eq!(lifecycle.saved(true, false), DocumentStep::Perform(intent));
            assert!(!lifecycle.pending());
            assert_eq!(lifecycle.saved(true, false), DocumentStep::None);
        }
    }

    #[test]
    fn clean_documents_perform_close_and_open_without_a_prompt() {
        for intent in intents() {
            let mut lifecycle = DocumentLifecycle::default();
            assert_eq!(
                lifecycle.request(intent.clone(), false),
                DocumentStep::Perform(intent)
            );
            assert!(!lifecycle.pending());
            assert_eq!(lifecycle.saved(true, false), DocumentStep::None);
        }
    }

    #[test]
    fn explicit_discard_performs_the_intent_without_a_save() {
        for intent in intents() {
            let mut lifecycle = DocumentLifecycle::default();
            lifecycle.request(intent.clone(), true);
            assert_eq!(
                lifecycle.respond(SaveChoice::Discard),
                DocumentStep::Perform(intent)
            );
            assert!(!lifecycle.pending());
            assert_eq!(lifecycle.saved(true, false), DocumentStep::None);
        }
    }

    #[test]
    fn canceling_the_save_changes_prompt_keeps_the_document_open() {
        let mut lifecycle = DocumentLifecycle::default();
        lifecycle.request(DocumentIntent::Close, true);
        assert_eq!(lifecycle.respond(SaveChoice::Cancel), DocumentStep::None);
        assert!(!lifecycle.pending());
        assert_eq!(lifecycle.saved(true, false), DocumentStep::None);
    }

    #[test]
    fn canceling_the_save_panel_clears_the_deferred_destructive_intent() {
        let mut lifecycle = DocumentLifecycle::default();
        lifecycle.request(DocumentIntent::OpenDialog, true);
        assert_eq!(lifecycle.respond(SaveChoice::Save), DocumentStep::Save);
        lifecycle.cancel();
        assert!(!lifecycle.pending());
        assert_eq!(lifecycle.saved(true, false), DocumentStep::None);
        assert_eq!(lifecycle.respond(SaveChoice::Discard), DocumentStep::None);
    }

    #[test]
    fn failed_disk_save_never_closes_or_replaces_the_current_session() {
        for intent in intents() {
            let mut lifecycle = DocumentLifecycle::default();
            lifecycle.request(intent, true);
            lifecycle.respond(SaveChoice::Save);
            assert_eq!(lifecycle.saved(false, true), DocumentStep::None);
            assert!(!lifecycle.pending());
            assert_eq!(lifecycle.saved(true, false), DocumentStep::None);
        }
    }

    #[test]
    fn edits_during_save_require_another_decision_before_transitioning() {
        let mut lifecycle = DocumentLifecycle::default();
        lifecycle.request(DocumentIntent::Quit, true);
        lifecycle.respond(SaveChoice::Save);
        assert_eq!(lifecycle.saved(true, true), DocumentStep::PromptSaveChanges);
        assert!(lifecycle.pending());
        assert_eq!(lifecycle.saved(true, false), DocumentStep::None);
        assert_eq!(lifecycle.respond(SaveChoice::Save), DocumentStep::Save);
        assert_eq!(
            lifecycle.saved(true, false),
            DocumentStep::Perform(DocumentIntent::Quit)
        );
        assert!(!lifecycle.pending());
    }

    #[test]
    fn newer_edits_can_cancel_the_transition_after_an_earlier_save_succeeded() {
        let mut lifecycle = DocumentLifecycle::default();
        lifecycle.request(DocumentIntent::New, true);
        lifecycle.respond(SaveChoice::Save);
        assert_eq!(lifecycle.saved(true, true), DocumentStep::PromptSaveChanges);
        assert_eq!(lifecycle.respond(SaveChoice::Cancel), DocumentStep::None);
        assert!(!lifecycle.pending());
    }

    #[test]
    fn unrelated_save_completion_cannot_bypass_an_unanswered_prompt() {
        let mut lifecycle = DocumentLifecycle::default();
        assert_eq!(lifecycle.saved(true, false), DocumentStep::None);
        lifecycle.request(DocumentIntent::Close, true);
        assert_eq!(lifecycle.saved(true, false), DocumentStep::None);
        assert_eq!(lifecycle.saved(false, true), DocumentStep::None);
        assert!(lifecycle.pending());
        assert_eq!(lifecycle.respond(SaveChoice::Save), DocumentStep::Save);
    }

    #[test]
    fn stray_prompt_responses_do_not_create_a_transition_or_save() {
        let mut lifecycle = DocumentLifecycle::default();
        for choice in [SaveChoice::Save, SaveChoice::Discard, SaveChoice::Cancel] {
            assert_eq!(lifecycle.respond(choice), DocumentStep::None);
            assert!(!lifecycle.pending());
        }
    }
}
