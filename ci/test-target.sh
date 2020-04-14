#!/usr/bin/env bash

set -ex

TARGET=$1

docker build -f ci/docker/Dockerfile.$TARGET -t linux-aio-tokio/tests-$TARGET ci/docker
cross test eventfd --target=$TARGET
