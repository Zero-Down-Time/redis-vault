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
	-cargo clippy
	-cargo deny check -s

fmt::
	-cargo fmt

clean::
	-cargo clean
