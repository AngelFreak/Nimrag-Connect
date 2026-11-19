# Build from the workspace root: docker build -f server/Dockerfile -t nimrag-server .
FROM rust:1-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends build-essential && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY core ./core
COPY server ./server
COPY tools ./tools
ENV SQLX_OFFLINE=true
RUN cargo build --release --locked -p nimrag-server

FROM debian:bookworm-slim
RUN useradd --system --uid 10001 --home-dir /data nimrag \
    && mkdir /data && chown nimrag /data
COPY --from=build /src/target/release/nimrag-server /usr/local/bin/nimrag-server
USER nimrag
ENV NIMRAG_DATA_DIR=/data
VOLUME /data
EXPOSE 8080
ENTRYPOINT ["nimrag-server"]
CMD ["serve", "--listen", "0.0.0.0:8080", "--data-dir", "/data"]
