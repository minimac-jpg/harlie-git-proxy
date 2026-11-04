FROM lukemathwalker/cargo-chef:0.1.77-rust-1.96.0-slim-bookworm AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
ARG GIT_SHA=unknown
ENV GIT_SHA=${GIT_SHA}
RUN cargo build --release --bin harlie-git-proxy \
    && mv target/release/harlie-git-proxy /usr/local/bin/service

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/local/bin/service /usr/local/bin/service
ENV RUST_LOG=info
USER 65534:65534
EXPOSE 3010
ENTRYPOINT ["/usr/local/bin/service"]
