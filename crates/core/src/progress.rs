//! How progress leaves the Core.
//!
//! This is a return channel, not a third injected boundary: the Core sends events
//! down a plain `mpsc` sender it was handed for the duration of one [`crate::Core::execute`]
//! call. Nothing is asked of the receiver and no behaviour is injected.

use std::sync::mpsc::Sender;

/// The stages of a Deployment, in order - Sync comes last so the game stays untouched
/// until everything that can fail has succeeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Fetch,
    /// Settling the DLL: taking the one a Release ships, keeping the one already deployed, or
    /// compiling it - and the Lua engine, when LuaJIT is asked for.
    Build,
    /// Assembling a Modpack. Its own stage, because a Modpack built on a Release compiles
    /// nothing, and its lines must not read as a DLL build.
    Modpack,
    Sync,
}

impl Stage {
    pub fn label(self) -> &'static str {
        match self {
            Self::Fetch => "Fetching sources",
            // "Preparing", not "Building": a Release's DLL is taken as shipped, and one
            // already deployed is kept. The messages say when it is actually compiled.
            Self::Build => "Preparing the DLL",
            Self::Modpack => "Building the modpack",
            Self::Sync => "Installing into the game",
        }
    }
}

/// One thing that happened, phrased for a player rather than a programmer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressEvent {
    pub stage: Stage,
    pub message: String,
}

/// Where the Core sends [`ProgressEvent`]s. Silent unless given a channel.
///
/// Implementations of the two boundaries are handed this too, so a long download or compile
/// can report while it runs.
pub struct ProgressReporter {
    sender: Option<Sender<ProgressEvent>>,
}

impl ProgressReporter {
    /// Discards every event. Used by tests that assert on the file tree instead.
    pub fn silent() -> Self {
        Self { sender: None }
    }

    pub fn to_channel(sender: Sender<ProgressEvent>) -> Self {
        Self {
            sender: Some(sender),
        }
    }

    /// Report one event. A hung-up receiver is not an error - the install carries on.
    pub fn report(&self, stage: Stage, message: impl Into<String>) {
        if let Some(sender) = &self.sender {
            let _ = sender.send(ProgressEvent {
                stage,
                message: message.into(),
            });
        }
    }
}
