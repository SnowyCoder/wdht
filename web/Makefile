SHELL = /bin/bash
.SHELLFLAGS := -eu -o pipefail -c
.DEFAULT_GOAL := build-web
.DELETE_ON_ERROR:
.SUFFIXES:

build-bundler-dev: src
	wasm-pack build --target bundler --dev

build-web-dev: src
	wasm-pack build --target web --dev

rename-target:
	sed -i -e 's/web\([-_]\)dht[-_]wasm/web\1dht/g' pkg/package.json pkg/web_dht_wasm.js
	for f in pkg/web_dht_wasm*; do mv "$$f" $$(echo "$$f" | sed 's/web_dht_wasm/web_dht/g'); done

pkg-web: build-web-dev rename-target
pkg-bundler: build-bundler-dev rename-target

build-bundler: pkg-bundler
build-web: pkg-web

serve: pkg-web
	python3 -m http.server
