# Runtime image for one Podman-managed game server.  The game payload is not
# included: each instance keeps SteamCMD and its game in a private bind mount.
FROM debian:13.6-slim@sha256:d7e12182ce18b85b93007c1dedf31f2d29e01ccf3182cc4017c709b6259bc132

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

# Ship a known SteamCMD bootstrap so a newly-created instance does not need
# to fetch the launcher itself. Odin copies this seed into the instance's
# private runtime volume before SteamCMD updates Valheim there.
RUN mkdir -p /usr/local/share/odin/steamcmd \
    && curl -fsSL https://steamcdn-a.akamaihd.net/client/installer/steamcmd_linux.tar.gz \
      | tar -xz -C /usr/local/share/odin/steamcmd \
    && chmod -R a+rX /usr/local/share/odin/steamcmd

COPY dist/odin-linux-amd64 /usr/local/bin/odin
RUN chmod 0755 /usr/local/bin/odin \
    && mkdir -p /srv/odin/runtime /srv/odin/instance /run/odin

ENTRYPOINT ["/usr/local/bin/odin"]
