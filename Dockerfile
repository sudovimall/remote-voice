FROM rust:1-bookworm AS builder

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY static ./static

RUN cargo build --release --locked

FROM debian:bookworm-slim AS runtime

WORKDIR /app

COPY --from=builder /app/target/release/voice /usr/local/bin/voice
COPY application.yaml ./application.yaml

# The Debian base image already reserves a `voice` group. A numeric
# unprivileged identity avoids coupling the runtime to base image accounts.
USER 10001:10001

EXPOSE 8080

CMD ["voice"]
