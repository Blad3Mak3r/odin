ARG BASE_IMAGE=ghcr.io/blad3mak3r/odin-srv:base
FROM ${BASE_IMAGE}

ARG GE_PROTON_TAG=GE-Proton9-25
LABEL io.odin.runtime-contract="1"
LABEL io.odin.proton="${GE_PROTON_TAG}"

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        bzip2 \
        fontconfig \
        fonts-liberation \
        libasound2 \
        libgl1 \
        libnss3 \
        libvulkan1 \
        libx11-6 \
        libxcomposite1 \
        libxdamage1 \
        libxfixes3 \
        libxrandr2 \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /opt/odin/proton \
    && curl -fsSL "https://github.com/GloriousEggroll/proton-ge-custom/releases/download/${GE_PROTON_TAG}/${GE_PROTON_TAG}.tar.gz" \
      | tar -xz -C /opt/odin/proton --strip-components=1 \
    && ln -s /opt/odin/proton/proton /usr/local/bin/proton
