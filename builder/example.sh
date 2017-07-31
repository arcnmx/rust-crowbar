#!/bin/sh

docker run --rm \
	-v $PWD:/code:ro \
	-v $PWD/target/crowbar:/target \
	-v $PWD/target/crowbar/registry:/root/.cargo/registry \
	-e CARGO_TARGET_DIR=/target \
	-w /code \
	crowbar-builder \
	cargo build --release
