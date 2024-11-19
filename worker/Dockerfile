FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y libssl3
COPY ./bin /usr/local/bin/worker
ENTRYPOINT ["worker"]
