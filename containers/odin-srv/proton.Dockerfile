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
        mesa-vulkan-drivers \
        python3 \
        xauth \
        xvfb \
    && rm -rf /var/lib/apt/lists/* \
    && curl -fsSL "https://github.com/GloriousEggroll/proton-ge-custom/releases/download/${GE_PROTON_TAG}/${GE_PROTON_TAG}.tar.gz" \
      | tar -xz -C /usr/local/bin --strip-components=1

# Proton keeps its Wine prefix under STEAM_COMPAT_DATA_PATH and locks
# <path>/pfx.lock before creating it, so the directory must already exist.
# Containers run as the host user's arbitrary uid, hence the open mode.
ENV STEAM_COMPAT_DATA_PATH=/opt/odin/proton-prefix
RUN mkdir -p "${STEAM_COMPAT_DATA_PATH}" \
    && chmod 1777 "${STEAM_COMPAT_DATA_PATH}"

# Wine reads /etc/machine-id and the slim base ships none.
RUN cat /proc/sys/kernel/random/uuid | tr -d - > /etc/machine-id
