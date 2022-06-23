SHELL = /bin/bash
.SHELLFLAGS := -eu -o pipefail -c
.DEFAULT_GOAL := build-web
.DELETE_ON_ERROR:
.SUFFIXES:

build-bundler-dev: src
	wasm-pack build --target bundler --dev

build-web-dev: src
	wasm-pack build --target web --dev

clean-js-json:
	sed -i -e 's/"name": "web-dht-wasm"/"name": "web-dht"/g' pkg/package.json

pkg-web: build-web-dev clean-js-json
pkg-bundler: build-bundler-dev clean-js-json

build-bundler: pkg-bundler
build-web: pkg-web

serve: pkg-web
	python3 -m http.server


