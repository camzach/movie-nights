FROM rust:bookworm as builder

WORKDIR /usr/src/app
COPY . .
ENV SQLX_OFFLINE=true
# Will build and cache the binary and dependent crates in release mode
RUN --mount=type=cache,target=/usr/local/cargo,from=rust:latest,source=/usr/local/cargo \
    --mount=type=cache,target=target \
    cargo build --release && mv ./target/release/movie-nights ./movie-nights

# Runtime image
FROM debian:bookworm-slim

RUN apt-get update && apt install -y openssl

# Run as "app" user
RUN useradd -ms /bin/bash app

USER app
WORKDIR /app

# Get compiled binaries from builder's cargo install directory
COPY --from=builder /usr/src/app/movie-nights /app/movie-nights

# Run the app
CMD ./movie-nights /app/
