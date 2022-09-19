FROM rust:1.62-buster as builder

VOLUME ["/output"]

RUN apt-get update
RUN apt-get install -y pkg-config libssl-dev build-essential cmake clang

WORKDIR /code

COPY ./rust-toolchain.toml .
RUN rustup show

COPY . .
WORKDIR /code/server

RUN cargo install --path .

FROM debian:buster-slim
COPY --from=builder /usr/local/cargo/bin/wdht-server /usr/local/bin/wdht-server

RUN \
  apt-get update && \
  apt-get install -y ca-certificates && \
  apt-get clean
  

ENV RUST_LOG=info
ENTRYPOINT ["wdht-server"]


