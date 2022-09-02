FROM rust:1.62-buster as builder

VOLUME ["/output"]

RUN apt-get update
RUN apt-get install -y pkg-config libssl-dev build-essential cmake clang

RUN rustup default nightly-2022-06-15

WORKDIR /code

COPY ./rust-toolchain.toml .
RUN rustup show

COPY . .
WORKDIR /code/server

RUN cargo install --path .

FROM debian:buster-slim
COPY --from=builder /usr/local/cargo/bin/wdht-server /usr/local/bin/wdht-server
ENV RUST_LOG=info
CMD ["wdht-server", "server", "--bind", "0.0.0.0:3141"]


