# wdht-web
This module is a simple wrapper for the [wdht](../wdht) module that exports its interface to JavaScript.

`index.html` is a very simple application to test the app usage, it can be served with `make serve`.

## Dependencies
The only dependency that is not installed by cargo is [wasm-pack](https://rustwasm.github.io/wasm-pack/installer/).

## Building
This module needs to export the JavaScript library, since it's a complex operation we created a Makefile for that:
- `make`: will compile the WebAssembly library and package it in the `./pkg` directory.
- `make serve`: will open a simple static server to aid in development
- `make dev`: Will compile the WebAssembly library in debug mode.
