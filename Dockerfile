ARG ALPINE_VERSION=3.23

FROM alpine:${ALPINE_VERSION}

ARG ALPINE_VERSION

RUN cd /etc/apk/keys && \
    wget "https://cdn.zero-downtime.net/alpine/stefan@zero-downtime.net-61bb6bfb.rsa.pub" && \
    echo "@kubezero https://cdn.zero-downtime.net/alpine/v${ALPINE_VERSION}/kubezero" >> /etc/apk/repositories && \
    apk upgrade -U -a --no-cache && \
    apk add --no-cache \
    ca-certificates \
    tini \
    libgcc \
    libssl3 \
    redis-vault@kubezero && \
    rm -rf /var/cache/apk/*

# Create non-root user and group
RUN addgroup -g 1000 vault && \
    adduser -D -u 1000 -G vault vault && \
    mkdir -p /data && \
    chown vault:vault /data

USER vault

ENTRYPOINT ["tini", "--", "redis-vault"]
