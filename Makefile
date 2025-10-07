REGISTRY := public.ecr.aws/zero-downtime
IMAGE := redis-vault
REGION := us-east-1

include .ci/podman.mk

CRATE_VER = $(shell echo $$TAG | sed -e 's/^v//g')

prepare::
	sed -i -e "s/^version = \".*/version = \"$$CRATE_VER\"/" Cargo.toml

fmt::
	which cargo >/dev/null 2>&1 && cargo fmt

clean::
	which cargo >/dev/null 2>&1 && cargo clean
