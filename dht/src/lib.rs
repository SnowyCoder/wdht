
pub mod config;
mod search;
mod consts;
mod contacter;
mod storage;
mod dht;
mod id;
mod kbucket;
mod ktree;

pub use dht::KademliaDht;
pub use id::Id;

pub fn add(a: u32, b: u32) -> u32 {
    a + b
}


#[cfg(test)]
mod tests {
    use crate::add;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
