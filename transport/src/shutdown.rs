use async_broadcast::TryRecvError;


pub struct ShutdownSender(async_broadcast::Sender<()>);

impl ShutdownSender {
    pub fn new() -> Self {
        let (mut sender, _receiver) = async_broadcast::broadcast(1);
        // There should always be only one
        sender.set_overflow(true);
        ShutdownSender(sender)
    }

    pub fn shutdown(self) {
        // Possible errors: channel is full (we should be the only sender)
        let _ = self.0.try_broadcast(());
    }

    pub fn subscribe(&self) -> async_broadcast::Receiver<()> {
        self.0.new_receiver()
    }
}


#[derive(Clone)]
pub struct ShutdownReceiver(Option<async_broadcast::Receiver<()>>);

impl ShutdownReceiver {
    pub async fn recv(&mut self) -> () {
        if let Some(x) = self.0.as_mut() {
            // There are two cases in which .await returns
            // - The channel has received a message (that is, a shutdown message has been sent)
            // - The channel is closed and we received an error (we interpret it as a shutdown nontheless)
            let _ = x.recv().await;
            self.0 = None;// Prevent re-listening
        };
        ()
    }

    pub fn try_recv(&mut self) -> bool {
        if let Some(x) = self.0.as_mut() {
            if x.try_recv() == Err(TryRecvError::Empty) {
                return false;
            }
            self.0 = None;// Prevent re-listening
        };
        true
    }
}
