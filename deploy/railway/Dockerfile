# The Zecraft raffle on Railway: one image, one service, the whole stack
# (keyholders 1-3, the verify relay and the engine) behind one public port.
# The binaries and the web page are built on the operator's Mac
# (scripts/railway/deploy.sh) and must match the sha256 recorded next to
# them, so Railway's builder only copies files and what runs is exactly
# what that script printed. No secret is in the image: the key shares and
# databases reach the volume only through scripts/railway/import.sh.
FROM debian:bookworm-slim@sha256:7ff6ba31527b9e336162fa061746c366c4b2032f4d2cff2078bea43f78efe3f8
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates sqlite3 tini \
  && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY . /app/
RUN sha256sum -c --quiet /app/SHA256SUMS \
  && chmod 755 /app/entrypoint.sh /app/unpack.sh /app/ops.sh /app/zecraft-* \
  && chmod -R a+rX /app/web
# The engine and the keyholders shut down cleanly on SIGINT (Ctrl-C).
STOPSIGNAL SIGINT
# -g: signals go to the whole process group, so every child gets them.
ENTRYPOINT ["/usr/bin/tini", "-g", "--", "/app/entrypoint.sh"]
