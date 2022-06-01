

#[cfg(target_arch = "wasm32")]
mod inner {
    use std::time::Duration;
    use gloo_timers::future::TimeoutFuture;

    pub use std::rc::Rc as Orc;
    pub use std::rc::Weak;

    pub use wasm_bindgen_futures::spawn_local as spawn;

    pub fn sleep(duration: Duration) -> TimeoutFuture {
        TimeoutFuture::new(duration.as_millis() as u32)
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod inner {
    pub use std::sync::Arc as Orc;
    pub use std::sync::Weak;

    pub use tokio::spawn;

    pub use tokio::time::sleep;
}

pub use inner::{Orc, Weak, spawn, sleep};
