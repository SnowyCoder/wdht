#[cfg(target_arch = "wasm32")]
mod inner {
    use gloo_timers::future::TimeoutFuture;
    use std::time::Duration;

    pub use std::rc::Rc as Orc;
    pub use std::rc::Weak;

    pub use wasm_bindgen_futures::spawn_local as spawn;

    pub fn sleep(duration: Duration) -> TimeoutFuture {
        TimeoutFuture::new(duration.as_millis() as u32)
    }

    pub trait MaybeSend {}

    impl<T> MaybeSend for T {}
}

#[cfg(not(target_arch = "wasm32"))]
mod inner {
    pub use std::sync::Arc as Orc;
    pub use std::sync::Weak;

    pub use tokio::spawn;

    pub use tokio::time::sleep;

    pub use core::marker::Send as MaybeSend;
}

pub use inner::{sleep, spawn, Orc, Weak, MaybeSend};

pub trait SenderExt<T> {
    fn maybe_spawn_send(&self, mex: T);
}

impl<T: 'static + MaybeSend> SenderExt<T> for tokio::sync::mpsc::Sender<T> {
    fn maybe_spawn_send(&self, mex: T) {
        if let Err(tokio::sync::mpsc::error::TrySendError::Closed(mex)) = self.try_send(mex) {
            let this = self.clone();
            spawn(async move {
                let _ = this.send(mex).await;
            });
        }
    }
}
