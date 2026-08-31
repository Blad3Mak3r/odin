FROM debian:13.6-slim@sha256:d7e12182ce18b85b93007c1dedf31f2d29e01ccf3182cc4017c709b6259bc132 AS runtime

RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install --yes --no-install-recommends \
        ca-certificates \
        curl \
        lib32gcc-s1 \
        lib32stdc++6 \
        libatomic1 \
        libpulse0 \
        passwd \
        tini \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 odin \
    && useradd --uid 10001 --gid 10001 \
        --home-dir /var/lib/odin --create-home \
        --shell /usr/sbin/nologin odin \
    && install -d -o root -g odin -m 0755 /etc/odin \
    && install -d -o odin -g odin -m 0750 /run/odin /var/lib/odin

COPY --chown=0:0 --chmod=0755 dist/odin-linux-amd64 /usr/local/bin/odin
COPY --chown=0:10001 --chmod=0640 packaging/config.toml.default /etc/odin/config.toml

ENV HOME=/var/lib/odin \
    ODIN_STOP_INSTANCES_ON_SHUTDOWN=1

WORKDIR /var/lib/odin
VOLUME ["/var/lib/odin"]

EXPOSE 7331/tcp
STOPSIGNAL SIGTERM

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD ["curl", "--fail", "--silent", "http://127.0.0.1:7331/"]

USER 10001:10001

ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/odin"]
CMD ["serve", "--bind", "127.0.0.1", "--port", "7331"]
