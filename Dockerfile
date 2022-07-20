FROM rust:latest AS builder

WORKDIR /app


ADD ./src ./src
ADD ./Cargo.lock .
ADD ./Cargo.toml .

RUN cargo build --release 

FROM debian:bullseye

ARG USER=downloader
ARG UID=10001

ENV USER=$USER
ENV UID=$UID

RUN adduser \
    --disabled-password \
    --gecos "" \
    --home "/home/${USER}" \
    --shell "/sbin/nologin" \
    --uid "${UID}" \
    "${USER}"

WORKDIR /app

# Copy our build
ADD ./entrypoint.sh .

COPY --from=builder /app/target/release/jenkins_bot .

RUN chown -R "${USER}":"${USER}" /app

USER $USER:$USER

ENTRYPOINT ["/app/entrypoint.sh"]