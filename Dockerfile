ARG RUST_VERSION=1.89.0
ARG APP_NAME=gurtd

FROM rust:${RUST_VERSION}-alpine AS build
ARG APP_NAME
WORKDIR /app

RUN apk add --no-cache clang lld musl-dev git

RUN --mount=type=bind,source=gurt-api,target=gurt-api \
    --mount=type=bind,source=gurt-db,target=gurt-db \
    --mount=type=bind,source=gurt-index,target=gurt-index \
    --mount=type=bind,source=gurt-macros,target=gurt-macros \
    --mount=type=bind,source=gurt-query,target=gurt-query \
    --mount=type=bind,source=gurt-web,target=gurt-web \
    --mount=type=bind,source=gurtd,target=gurtd \
    --mount=type=bind,source=Cargo.toml,target=Cargo.toml \
    --mount=type=bind,source=Cargo.lock,target=Cargo.lock \
    --mount=type=cache,target=/app/target/ \
    --mount=type=cache,target=/usr/local/cargo/git/db \
    --mount=type=cache,target=/usr/local/cargo/registry/ \
cargo build --locked --release --bin $APP_NAME && \
cp ./target/release/$APP_NAME /bin/server

FROM alpine:3.18 AS final

ARG UID=10001
RUN adduser \
    --disabled-password \
    --gecos "" \
    --home "/nonexistent" \
    --shell "/sbin/nologin" \
    --no-create-home \
    --uid "${UID}" \
    appuser
USER appuser

COPY --from=build /bin/server /bin/

EXPOSE 4878

CMD ["/bin/server"]
