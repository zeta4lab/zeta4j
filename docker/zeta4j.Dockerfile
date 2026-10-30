# syntax=docker/dockerfile:1
FROM rust:1.99-bookworm AS build
RUN apt-get update \
    && apt-get install -y --no-install-recommends protobuf-compiler clang cmake \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY . .
RUN cargo build --locked --release -p zeta4j -p zeta4j-cli \
    && install -D -m 0755 target/release/zeta4jd /out/zeta4jd \
    && install -D -m 0755 target/release/z4j /out/z4j

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates openssl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --home-dir /var/lib/zeta4j --create-home zeta4j
COPY --from=build /out/zeta4jd /out/z4j /usr/local/bin/
COPY docker/zeta4j-entrypoint.sh /usr/local/bin/zeta4j-entrypoint
ENV ZETA4J_HOME=/var/lib/zeta4j \
    ZETA4J_BIND_ADDRESS=0.0.0.0
USER zeta4j
WORKDIR /var/lib/zeta4j
VOLUME ["/var/lib/zeta4j"]
EXPOSE 9044 9045 9046
ENTRYPOINT ["zeta4j-entrypoint"]
CMD ["zeta4jd"]
