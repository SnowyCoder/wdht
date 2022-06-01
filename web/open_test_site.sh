#!/bin/bash
wasm-pack build --target web --dev && python3 -m http.server

