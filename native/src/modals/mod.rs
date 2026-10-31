//! Modal dialogs and overlay views.

pub mod confirm_delete;
pub mod cothink;
pub mod encrypt;
pub mod faq;
pub mod imap_sync;
pub mod mcp_connect;
pub mod toasts;

pub use confirm_delete::{draw_confirm_delete, ConfirmDeleteAction};
pub use cothink::{draw_cothink_modal, CoThinkModalAction};
pub use encrypt::{draw_encrypt_modal, EncryptAction, EncryptMode, EncryptState};
pub use faq::draw_faq_modal;
pub use imap_sync::{draw_imap_modal, ImapModalAction};
pub use mcp_connect::{draw_mcp_modal, McpModalAction};
pub use toasts::draw_toast;
