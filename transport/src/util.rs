use std::{sync::Arc};
use core::hash::Hash;

#[derive(Clone)]
pub struct ArcKey<T>(pub Arc<T>);

impl<T> PartialEq for ArcKey<T> {
    fn eq(&self, other: &Self) -> bool {
        Arc::as_ptr(&self.0) == Arc::as_ptr(&other.0)
    }
}

impl<T> Eq for ArcKey<T> {}

impl<T>  Hash for ArcKey<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.0).hash(state);
    }
}
