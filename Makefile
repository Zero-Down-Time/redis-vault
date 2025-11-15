REGISTRY := public.ecr.aws/zero-downtime
IMAGE := redis-vault
REGION := us-east-1

include .ci/podman.mk

bump-version::
	. .ci/utils.sh
	new_version=$$(bumpVersion $(TAG) | sed -e 's/^v//' )
	sed -i -e "s/^version = \".*/version = \"$$new_version\"/" Cargo.toml
	echo "set cargo package version to $$new_version in Cargo.toml"
	addCommitTagPush Cargo.toml v$$new_version

fmt::
	cargo fmt || echo "cargo unavailable. Noop"
	cargo clippy || echo "cargo unavailable. Noop"
	cargo deny check -s|| echo "cargo unavailable. Noop"

clean::
	cargo clean || echo "cargo unavailable. Noop"
