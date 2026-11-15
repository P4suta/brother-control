use crate::types::{Failure, FailureKind};
use std::sync::mpsc::{self, Receiver, TryRecvError, TrySendError};

pub(crate) struct Cancellation {
    receiver: Receiver<()>,
    interrupted: bool,
}

impl Cancellation {
    fn new(receiver: Receiver<()>) -> Self {
        Self {
            receiver,
            interrupted: false,
        }
    }

    pub(crate) fn install() -> Result<Self, Failure> {
        let (sender, receiver) = mpsc::sync_channel(1);
        ctrlc::try_set_handler(move || match sender.try_send(()) {
            Ok(()) | Err(TrySendError::Full(()) | TrySendError::Disconnected(())) => (),
        })
        .map_err(|_| {
            Failure::new(
                FailureKind::Storage,
                "unable to register the cancellation handler",
            )
        })?;
        Ok(Self::new(receiver))
    }

    pub(crate) fn poll(&mut self) -> bool {
        if !self.interrupted {
            self.interrupted = match self.receiver.try_recv() {
                Ok(()) | Err(TryRecvError::Disconnected) => true,
                Err(TryRecvError::Empty) => false,
            };
        }
        self.interrupted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_stays_set_after_the_signal_is_consumed() -> Result<(), String> {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let mut cancellation = Cancellation::new(receiver);
        assert!(!cancellation.poll());
        sender.try_send(()).map_err(|error| error.to_string())?;
        assert!(cancellation.poll());
        assert!(cancellation.poll());
        drop(sender);
        assert!(cancellation.poll());
        Ok(())
    }
}
