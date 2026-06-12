FROM rust:1-slim AS builder
WORKDIR /app

# ── Layer 1: dependency cache ──────────────────────────────────────────────
# Compile dependencies with a stub binary so this layer is only invalidated
# when Cargo.toml / Cargo.lock change, keeping iterative image builds fast.
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src && echo 'fn main() {}' > src/main.rs && \
    cargo build --release --locked && \
    rm -f target/release/atoma target/release/deps/atoma-* target/release/deps/atoma_*

# ── Layer 2: real source ───────────────────────────────────────────────────
COPY src ./src
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
# Run as non-root for security
RUN useradd -r -s /sbin/nologin atoma
COPY --from=builder /app/target/release/atoma /usr/local/bin/atoma
USER atoma
ENTRYPOINT ["atoma"]
CMD ["--help"]