FROM --platform=linux/amd64 messense/rust-musl-cross:x86_64-musl AS amd64
COPY . .
RUN cargo install --path . --root /x

FROM --platform=linux/amd64 messense/rust-musl-cross:aarch64-musl AS arm64
COPY . .
RUN cargo install --path . --root /x

FROM ${TARGETARCH} AS build

FROM alpine
LABEL maintainer="K8sCat <k8scat@gmail.com>"
COPY --from=build /x/bin/github-trending /usr/local/bin/github-trending
ENV RUST_LOG=info
CMD ["github-trending"]
