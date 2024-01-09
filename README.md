# WebDHT

Distributed Signaling in Web pages!

## About
WebDHT is a project that tries to merge the flexibility of Distributed Hash Tables and the Web technology.
The project is mainly developed in Rust and offers both a high-performance server (natively compiled and docker-compatible) and a JS-compatible library.

You can see a minimal example of its usage [here](https://wdhtchat.rossilorenzo.dev/).

## How
Every user of WebDHT is a node in a Kademlia-like Distributed Hash Table, both native and browser nodes join in the same manner and share the load of the network. Every primitive is implemented on top of that.

Peer to peer connections between browsers are achieved using WebRTC as a stadard protocol and using other already-opened connections as a signaling channel. Some bootstrap (native) nodes are required to create the initial connections, but as they are also part of the DHT, their load gets distributed too.

## Project structure
The project is structured as follows:
- **wdht**: the main part of the application, implements the transport logic and re-exports the DHT.
- **logic**: contains the custom Kademlia implementation with a pluggalbe transport protocol.
- **web**: exports wdht as a JavaScript library.

Applications:
- **server**: uses wdht to create a minimal, high-performance native server
- **example_chat**: simple application to demonstrate how WebDHT primitives can be used to connect to other peers.

Some minor adapter libraries are used internally to abstract different features across browser and native contexts.
- **crypto**: implements the cryptographic primitives.
- **wrtc**: implements the WebRTC transport protocol.
- **wasync**: small library to help with asynchronous operations and runtimes (through some strange hacks).

## Building
The project is mainly developed in rust, using the excellent package manager `cargo`. Most of the projects can be built with the standard `cargo` commands:
- `cargo check`: to check for errors
- `cargo build`: to build the code
- `cargo run`: to run the application

When an application uses conditional compilation you can select the wasm compilation by using `--target warm32-unknown-unknown`, but apart from the three adapters most of the code should be shared.

There are only two exceptions: `web` and `example_chat`, they both have their own `README.md` explaining their commands.

## Presentations
You can see a bit more about the project by reading the slides that I provided both for the [the 21st IEEE International Symposium on Network Computing and Applications (NCA2022, english)](https://nca2022.rossilorenzo.dev) and for [my thesis (italian)](https://tesi22.rossilorenzo.dev).

For a longer description check out the [WebDHT paper](https://secloud.ing.unimore.it/static/profiles/ferretti/papers/webdht_nca22.pdf).


## Rust toolchain
Currently the project is using the Rust nightly channel as it's being blocked only by [type_alias_impl_trait](https://rust-lang.github.io/rfcs/2515-type_alias_impl_trait.html). We require it because the project's futures are too complex to build without async functions (and using Box would mean giving up zero-const abstractions).
