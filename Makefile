REGISTRY := public.ecr.aws/zero-downtime
IMAGE := redis-vault
REGION := us-east-1

include .ci/podman.mk

fmt::
	cargo fmt

clean::
	cargo clean
