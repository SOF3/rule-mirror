FROM rust:1.51-alpine AS base
RUN apk add --no-cache musl-dev pkgconfig openssl-dev
WORKDIR /usr/src/app

FROM base AS chef-base
RUN cargo install cargo-chef

FROM chef-base AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef-base AS cacher
COPY --from=planner /usr/src/app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

FROM base AS builder
COPY . .
COPY --from=cacher /usr/src/app/target target
RUN cargo build --release --all

FROM alpine AS runtime
RUN apk add --no-cache musl-dev pkgconfig openssl-dev
WORKDIR /app
RUN adduser --uid 1000 --disabled-password --home /app app
RUN chown 1000:1000 /app
USER app

FROM runtime AS web
COPY --from=builder /usr/src/app/target/release/web /usr/local/bin/web

ENV RUST_LOG=info
ENTRYPOINT ["web"]

FROM runtime AS bot
COPY --from=builder /usr/src/app/target/release/bot /usr/local/bin/bot

ENV RUST_LOG=info
ENTRYPOINT ["bot"]
