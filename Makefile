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

lint::
	which cargo-clippy 1>/dev/null && cargo-clippy
	which cargo-deny 1>/dev/null && cargo-deny check -s

fmt::
	which cargo-fmt 1>/dev/null && cargo-fmt

clean::
	which cargo 1>/dev/null && cargo clean
