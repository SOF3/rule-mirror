FROM rust:1.51-alpine AS base

RUN apk add --no-cache musl-dev pkgconfig openssl-dev

RUN mkdir -p /usr/src/app/web /usr/src/app/bot
WORKDIR /usr/src/app
ADD Cargo.toml Cargo.toml
ADD Cargo.lock Cargo.lock
ADD common common

WORKDIR /usr/src/app/web
RUN cargo init
WORKDIR /usr/src/app/bot
RUN cargo init

WORKDIR /usr/src/app
RUN cargo build --release

FROM base AS web-build
RUN rm -r web
ADD web web
RUN cargo build --release --bin web

FROM alpine AS web
USER root
RUN mkdir /web
RUN adduser --uid 1000 --disabled-password --home /web web
RUN chown 1000:1000 /web

USER web
WORKDIR /web
COPY --from=web-build /usr/src/app/target/release/web /usr/bin/web

ENV RUST_LOG=info
ENTRYPOINT ["web"]

FROM base AS bot-build
RUN rm -r bot
ADD bot bot
RUN cargo build --release --bin bot

FROM alpine AS bot
USER root
RUN mkdir /bot
RUN adduser --uid 1000 --disabled-password --home /bot bot
RUN chown 1000:1000 /bot

USER bot
WORKDIR /bot
COPY --from=bot-build /usr/src/app/target/release/bot /usr/bin/bot

ENV RUST_LOG=info
ENTRYPOINT ["bot"]
