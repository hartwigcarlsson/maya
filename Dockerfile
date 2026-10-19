# Mayas image: binären med sajten inbäddad, i en minimal image som kör som icke-root.
# CI bygger den för linux/amd64 och linux/arm64 på var sin maskin och publicerar den på GHCR.
#
# webauthn-rs länkar mot systemets OpenSSL (det finns ingen rustls-variant): rust:bookworm har
# libssl-dev och distroless/cc-debian12 har libssl3. Byt inte till distroless/static eller en
# image utan libssl3 utan att vendra OpenSSL.

# cargo-chef bygger beroendena i ett eget lager ur recipe.json, som bara ändras när Cargo.toml
# eller Cargo.lock gör det. Då byggs bara Mayas egna crater om när källkoden ändras.
FROM rust:1.99.0-bookworm AS chef
# Förbyggda binärer i stället för `cargo install`. Byts en version, byts också dess sha256.
ARG CARGO_LEPTOS_VERSION=0.3.11
ARG CARGO_CHEF_VERSION=0.1.78
RUN set -eu; \
    case "$(dpkg --print-architecture)" in \
      amd64) arch=x86_64; \
        leptos_sha=149b88535fd8f44321da0d667fecc445c15af35f62931b2091687445297cc9e0; \
        chef_sha=aca691abfbfbbe00d482e0ed2249eec3091b65e96a0ce92947fb5b254d48b16d ;; \
      arm64) arch=aarch64; \
        leptos_sha=834d34b7bc837ed4bfdc5a26428f7c7b249550a5aecd5e6220b76b702008ab12; \
        chef_sha=cd59b90fce5fa84c4189648fec33404ec5491791eebdb812df5e7fca8336408d ;; \
      *) echo "ingen förbyggd cargo-leptos för $(dpkg --print-architecture)" >&2; exit 1 ;; \
    esac; \
    leptos="cargo-leptos-${arch}-unknown-linux-musl"; \
    chef="cargo-chef-${arch}-unknown-linux-musl"; \
    curl -fsSLo /tmp/leptos.tar.gz \
      "https://github.com/leptos-rs/cargo-leptos/releases/download/v${CARGO_LEPTOS_VERSION}/${leptos}.tar.gz"; \
    curl -fsSLo /tmp/chef.tar.xz \
      "https://github.com/LukeMathWalker/cargo-chef/releases/download/v${CARGO_CHEF_VERSION}/${chef}.tar.xz"; \
    echo "${leptos_sha}  /tmp/leptos.tar.gz" | sha256sum -c -; \
    echo "${chef_sha}  /tmp/chef.tar.xz" | sha256sum -c -; \
    tar -xzf /tmp/leptos.tar.gz -C /usr/local/cargo/bin --strip-components=1 "${leptos}/cargo-leptos"; \
    tar -xJf /tmp/chef.tar.xz -C /usr/local/cargo/bin --strip-components=1 "${chef}/cargo-chef"; \
    rm /tmp/leptos.tar.gz /tmp/chef.tar.xz; \
    cargo-leptos --version; cargo-chef --version
WORKDIR /src
COPY rust-toolchain.toml ./
RUN rustup show active-toolchain || rustup toolchain install

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS build
# Samma miljö som cargo-leptos ger cargo. Leptos läser variablerna när det kompileras, och med
# andra värden här skulle `cargo leptos build` bygga om de förkokta craterna.
ENV LEPTOS_OUTPUT_NAME=maya \
    LEPTOS_SITE_ROOT=/src/target/site \
    LEPTOS_SITE_PKG_DIR=pkg \
    LEPTOS_SITE_ADDR=127.0.0.1:3000 \
    LEPTOS_RELOAD_PORT=3001 \
    LEPTOS_LIB_DIR=crates/web \
    LEPTOS_BIN_DIR=crates/maya \
    LEPTOS_JS_MINIFY=true \
    LEPTOS_HASH_FILES=false
COPY --from=planner /src/recipe.json recipe.json
# Beroendena för de två byggen cargo-leptos gör, med samma paket, mål, profil och features som
# [[workspace.metadata.leptos]] ger: servern och hydreringens WASM (i target/front).
RUN cargo chef cook --recipe-path recipe.json --locked --release --package maya --bin maya
RUN cargo chef cook --recipe-path recipe.json --locked --profile wasm-release \
    --target wasm32-unknown-unknown --target-dir target/front \
    --package maya-web --no-default-features --features hydrate
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
