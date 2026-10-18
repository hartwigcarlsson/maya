# Mayas image: binären med sajten inbäddad, i en minimal image som kör som icke-root.
# CI bygger den för linux/amd64 och linux/arm64 på var sin maskin och publicerar den på GHCR.
#
# webauthn-rs länkar mot systemets OpenSSL (det finns ingen rustls-variant): rust:bookworm har
# libssl-dev och distroless/cc-debian12 har libssl3. Byt inte till distroless/static eller en
# image utan libssl3 utan att vendra OpenSSL.

FROM rust:1.99.0-bookworm AS build
ARG CARGO_LEPTOS_VERSION=0.3.11
RUN cargo install cargo-leptos --version "${CARGO_LEPTOS_VERSION}" --locked
WORKDIR /src
COPY rust-toolchain.toml ./
RUN rustup show active-toolchain || rustup toolchain install
COPY . .
# Den enda sekventiella vägen genom cargo-leptos: sajten byggs före servern, som bäddar in den.
# --cargo-locked: bygget använder exakt Cargo.lock, som CI (`--locked`).
RUN cargo leptos build --release --precompress --cargo-locked \
    && install -D target/release/maya /out/maya \
    && mkdir -p /out/data /out/backup

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /out/maya /usr/local/bin/maya
COPY --from=build --chown=65532:65532 /out/data /data
COPY --from=build --chown=65532:65532 /out/backup /backup
ENV MAYA_DATA_DIR=/data \
    MAYA_LISTEN=0.0.0.0:8080 \
    RUST_LOG=info
USER nonroot
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=30s CMD ["/usr/local/bin/maya", "healthcheck"]
ENTRYPOINT ["/usr/local/bin/maya"]
CMD ["serve"]
