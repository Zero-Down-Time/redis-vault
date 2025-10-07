REGISTRY := public.ecr.aws/zero-downtime
IMAGE := redis-vault
REGION := us-east-1

include .ci/podman.mk

fmt::
	which cargo > /dev/null && cargo fmt

clean::
	which cargo > /dev/null && cargo clean
