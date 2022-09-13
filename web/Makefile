SHELL = /bin/bash
.SHELLFLAGS := -eu -o pipefail -c
.DEFAULT_GOAL := pkg-web
.DELETE_ON_ERROR:
.SUFFIXES:

build-web: src
	wasm-pack build --target web

build-web-dev: src
	wasm-pack build --target web --dev

rename-target:
	sed -i -e 's/web\([-_]\)dht[-_]wasm/web\1dht/g' pkg/package.json pkg/web_dht_wasm.js
	for f in pkg/web_dht_wasm*; do mv "$$f" $$(echo "$$f" | sed 's/web_dht_wasm/web_dht/g'); done
	sed -i -e 's/"module":/"main":/g' pkg/package.json

pkg-web-dev: build-web-dev rename-target
pkg-web: build-web rename-target

serve: pkg-web-dev
	python3 -m http.server

dev: pkg-web-dev
