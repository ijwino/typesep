.PHONY: build release dist test docs licenses licenses-check parity render-check epub-check baselines baselines-pages clean

build:
	cargo build

release:
	cargo build --release

dist:
	cargo build --profile dist

test:
	cargo test

docs: build
	target/debug/typesep reference > docs/style-reference.md

parity: build
	cargo run -q -p xtask -- parity $(ARGS)

render-check: release
	cargo run -q -p xtask -- render-check $(ARGS)

epub-check: release
	cargo run -q -p xtask -- epub-check $(ARGS)

baselines: release
	cargo run -q -p xtask -- baselines $(ARGS)

baselines-pages:
	cargo test -q --release -p sep-cli --test render -- --ignored

licenses:
	cargo run -q -p xtask -- licenses

licenses-check:
	cargo run -q -p xtask -- licenses --validate

clean:
	rm -rf out/*
