ARG BuildImage=ekidd/rust-musl-builder:stable
ARG BinName=do-fin

# Build
FROM $BuildImage AS Build
ADD --chown=rust:rust . ./
RUN cargo build --release

# Runtime
FROM alpine:3
ARG BinName

RUN echo $BinName

LABEL "project.namespace"="dolysis" "dolysis.binary"=$BinName

WORKDIR /home/$BinName
COPY --from=Build \
    /home/rust/src/target/x86_64-unknown-linux-musl/release/$BinName \
    .

ENTRYPOINT ["./${BinName}"]
