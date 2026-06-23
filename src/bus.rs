use std::sync::mpsc::{self, Receiver, Sender};

use crate::brain::{BrainCommand, BrainResponse, BrainService};
use crate::config::Config;
use crate::windows::callout_window::{create_callout_channel, CalloutCommand, CalloutSender};
use crate::windows::chat_window::{create_chat_channel, ChatReceiver, ChatSender};
use crate::windows::control_window::{create_control_channel, ControlReceiver, ControlSender};
use crate::windows::log_window::{create_Log_channel, LogReceiver, LogSender};

/// Clonable sender-only view of AppBus. Pass to components that only need to send.
#[derive(Clone)]
pub struct AppSenders {
    pub chat_tx: ChatSender,
    pub callout_tx: CalloutSender,
    pub log_tx: LogSender,
    pub ctrl_tx: ControlSender,
    pub brain_tx: Option<Sender<BrainCommand>>,
}

/// All inter-component channel endpoints, created once and distributed to windows.
///
/// Senders can be cloned freely via `senders()`.
/// Receivers are `Option<T>` — each window takes its own via `take()`.
pub struct AppBus {
    // -- senders --
    pub chat_tx: ChatSender,
    pub callout_tx: CalloutSender,
    pub log_tx: LogSender,
    pub ctrl_tx: ControlSender,
    pub brain_tx: Option<Sender<BrainCommand>>,

    // -- receivers (consumed by exactly one window via take()) --
    pub chat_rx: Option<ChatReceiver>,
    pub callout_rx: Option<Receiver<CalloutCommand>>,
    pub log_rx: Option<LogReceiver>,
    pub ctrl_rx: Option<ControlReceiver>,
    pub brain_rx: Option<Receiver<BrainResponse>>,
}

impl AppBus {
    /// Create all channels and spawn the brain thread if configured.
    pub fn create(config: &Config) -> Self {
        let (chat_tx, chat_rx) = create_chat_channel();
        let (callout_tx, callout_rx) = create_callout_channel();
        let (log_tx, log_rx) = create_Log_channel();
        let (ctrl_tx, ctrl_rx) = create_control_channel();

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
            ctrl_tx,
            brain_tx,
            chat_rx: Some(chat_rx),
            callout_rx: Some(callout_rx),
            log_rx: Some(log_rx),
            ctrl_rx: Some(ctrl_rx),
            brain_rx,
        }
    }

    /// Clone all senders into a standalone, clonable struct.
    pub fn senders(&self) -> AppSenders {
        AppSenders {
            chat_tx: self.chat_tx.clone(),
            callout_tx: self.callout_tx.clone(),
            log_tx: self.log_tx.clone(),
            ctrl_tx: self.ctrl_tx.clone(),
            brain_tx: self.brain_tx.clone(),
        }
    }
}
