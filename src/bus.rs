use std::sync::mpsc::{self, Receiver, Sender};

use crate::brain::{BrainCommand, BrainResponse, BrainService};
use crate::config::Config;
use crate::windows::callout_window::{create_callout_channel, CalloutCommand, CalloutSender};
use crate::windows::chat_window::{create_chat_channel, ChatReceiver, ChatSender};
use crate::windows::log_window::{create_Log_channel, LogReceiver, LogSender};

/// All inter-component channel endpoints, created once and distributed to windows.
///
/// Senders (Sender<T>) can be freely cloned.
/// Receivers must be consumed by exactly one window.
pub struct AppBus {
    // -- senders --
    pub chat_tx: ChatSender,
    pub callout_tx: CalloutSender,
    pub log_tx: LogSender,
    pub brain_tx: Option<Sender<BrainCommand>>,

    // -- receivers (each moved into exactly one window) --
    pub chat_rx: ChatReceiver,
    pub callout_rx: Receiver<CalloutCommand>,
    pub log_rx: LogReceiver,
    pub brain_rx: Option<Receiver<BrainResponse>>,
}

impl AppBus {
    /// Create all channels and spawn the brain thread if configured.
    pub fn create(config: &Config) -> Self {
        let (chat_tx, chat_rx) = create_chat_channel();
        let (callout_tx, callout_rx) = create_callout_channel();
        let (log_tx, log_rx) = create_Log_channel();

        let (brain_tx, brain_rx) = if let Some(ref brain_config) = config.brain {
            log::info!("Brain config found, spawning BrainService...");
            let (resp_tx, resp_rx) = mpsc::channel::<BrainResponse>();
            let cmd_tx = BrainService::spawn(brain_config.clone(), resp_tx);
            (Some(cmd_tx), Some(resp_rx))
        } else {
            log::info!("No brain config — chat will echo messages");
            (None, None)
        };

        Self {
            chat_tx,
            callout_tx,
            log_tx,
            brain_tx,
            chat_rx,
            callout_rx,
            log_rx,
            brain_rx,
        }
    }
}
