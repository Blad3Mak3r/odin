# Runtime contract shared by every Podman game container. The Odin binary is
# deliberately bind-mounted by the host instead of baked into this image.
FROM debian:13.6-slim@sha256:d7e12182ce18b85b93007c1dedf31f2d29e01ccf3182cc4017c709b6259bc132

LABEL io.odin.runtime-contract="1"

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        lib32gcc-s1 \
        lib32stdc++6 \
        libatomic1 \
        libpulse-mainloop-glib0 \
        libpulse0 \
        curl \
        tar \
        gzip \
    && rm -rf /var/lib/apt/lists/*

RUN mkdir -p /usr/local/share/odin/steamcmd \
    && curl -fsSL https://steamcdn-a.akamaihd.net/client/installer/steamcmd_linux.tar.gz \
      | tar -xz -C /usr/local/share/odin/steamcmd \
    && chmod -R a+rX /usr/local/share/odin/steamcmd \
    && mkdir -p /srv/odin/runtime /srv/odin/instance /run/odin
