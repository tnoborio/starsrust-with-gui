use std::sync::mpsc;

/// Events sent from the TCP server threads to the Bevy visualization.
#[derive(Debug, Clone)]
pub enum ServerEvent {
    NodeConnected {
        name: String,
    },
    NodeDisconnected {
        name: String,
    },
    MessageRouted {
        from: String,
        to: String,
    },
}

pub type EventSender = mpsc::Sender<ServerEvent>;
pub type EventReceiver = mpsc::Receiver<ServerEvent>;
