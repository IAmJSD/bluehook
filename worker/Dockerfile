FROM rust:1.82.0-bookworm AS base
WORKDIR /builder
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y libssl3 libpq5 ca-certificates
COPY --from=base /builder/target/release/worker /usr/local/bin/worker
ENTRYPOINT ["worker"]
