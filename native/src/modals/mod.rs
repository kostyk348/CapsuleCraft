//! Modal dialogs and overlay views.

pub mod appearance;
pub mod confirm_delete;
pub mod cothink;
pub mod encrypt;
pub mod faq;
pub mod imap_sync;
pub mod integrations;
pub mod mcp_connect;
pub mod onboarding;
pub mod shortcuts;
pub mod toasts;

pub use appearance::{draw_appearance, AppearanceAction};
pub use confirm_delete::{draw_confirm_delete, ConfirmDeleteAction};
pub use cothink::{draw_cothink_modal, CoThinkModalAction};
pub use encrypt::{draw_encrypt_modal, EncryptAction, EncryptMode, EncryptState};
pub use faq::draw_faq_modal;
pub use imap_sync::{draw_imap_modal, ImapModalAction};
pub use mcp_connect::{draw_mcp_modal, McpModalAction};
pub use onboarding::{draw_onboarding, mark_done, OnboardingAction, TryAction, STEP_COUNT};
pub use shortcuts::draw_shortcuts_modal;
pub use toasts::draw_toast;
