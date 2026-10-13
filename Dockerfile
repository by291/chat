# syntax=docker/dockerfile:1
FROM rust:1.99.0-bookworm AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY src ./src
COPY migrations ./migrations
RUN --mount=type=cache,target=/usr/local/cargo/registry --mount=type=cache,target=/app/target cargo build --locked --release --bin api --bin worker --bin admin && cp target/release/api target/release/worker target/release/admin /usr/local/bin/
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && rm -rf /var/lib/apt/lists/* && useradd --system --uid 10001 chat
COPY --from=build /usr/local/bin/api /usr/local/bin/worker /usr/local/bin/admin /usr/local/bin/
USER chat
WORKDIR /app
EXPOSE 8080 8081 8082
CMD ["api"]
