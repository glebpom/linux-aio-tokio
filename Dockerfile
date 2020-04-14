FROM rust:1.42.0

ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH

RUN apt-get update && apt-get install -y strace libclang-dev clang llvm cmake build-essential

ADD . /code
WORKDIR /code

CMD cargo check
