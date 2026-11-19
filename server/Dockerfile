# Build from the workspace root: docker build -f server/Dockerfile -t nimrag-server .
# Stage 1 builds the web interface; stage 2 embeds web/dist into the server binary (rust-embed).
FROM node:24-bookworm-slim AS web
WORKDIR /src/web
COPY web/package.json web/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY web ./
RUN npm run build

FROM rust:1-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends build-essential && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY core ./core
COPY ffi ./ffi
COPY server ./server
COPY tools ./tools
COPY --from=web /src/web/dist ./web/dist
ENV SQLX_OFFLINE=true
RUN cargo build --release --locked -p nimrag-server

FROM debian:bookworm-slim
# ca-certificates: Forward mode talks to Garmin over TLS.
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home-dir /data nimrag \
    && mkdir /data && chown nimrag /data
COPY --from=build /src/target/release/nimrag-server /usr/local/bin/nimrag-server
USER nimrag
ENV NIMRAG_DATA_DIR=/data
VOLUME /data
EXPOSE 8080
ENTRYPOINT ["nimrag-server"]
CMD ["serve", "--listen", "0.0.0.0:8080", "--data-dir", "/data"]
