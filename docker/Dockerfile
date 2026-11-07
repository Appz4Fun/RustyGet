# build stage
FROM alpine:3.22.2 AS build

ARG MAKE_JOBS=3
ARG TARGETPLATFORM

# install build packages
RUN \
  echo "**** install build packages ****" && \
  apk add --no-cache bash python3 pkgconf g++ gcc git just cargo rust openssl-dev curl

# build rustyget
COPY ./ /rustyget
RUN \
  echo "**** build rustyget ****" && \
  mkdir -p /app/rustyget && \
  cd /rustyget && \
  just release jobs=${MAKE_JOBS} && \
  install -v -m755 /rustyget/build/release/rustyget /app/rustyget/rustyget && \
  ln -sf /app/rustyget/rustyget /app/rustyget/nzbget && \
  cp -r /rustyget/webui /app/rustyget/webui && \
  cp /rustyget/nzbget.conf /app/rustyget/webui/rustyget.conf.template && \
  cp /rustyget/nzbget.conf /app/rustyget/webui/nzbget.conf.template && \
  mkdir -p /app/rustyget/share/rustyget && \
  cp /rustyget/nzbget.conf /app/rustyget/share/rustyget/rustyget.conf && \
  cp /rustyget/pubkey.pem /app/rustyget/pubkey.pem && \
  curl -o /app/rustyget/cacert.pem -L "https://curl.se/ca/cacert.pem"

# change default config values
RUN \
  sed -i \
    -e "s|^MainDir=.*|MainDir=/downloads|g" \
    -e "s|^WebDir=.*|WebDir=$\{AppDir\}/webui|g" \
    -e "s|^ConfigTemplate=.*|ConfigTemplate=$\{AppDir\}/webui/rustyget.conf.template|g" \
    -e "s|^CertStore=.*|CertStore=$\{AppDir\}/cacert.pem|g" \
    -e "s|^CertCheck=.*|CertCheck=yes|g" \
    -e "s|^DestDir=.*|DestDir=$\{MainDir\}/completed|g" \
    -e "s|^InterDir=.*|InterDir=$\{MainDir\}/intermediate|g" \
    -e "s|^LogFile=.*|LogFile=$\{MainDir\}/rustyget.log|g" \
    -e "s|^AuthorizedIP=.*|AuthorizedIP=127.0.0.1|g" \
  /app/rustyget/share/rustyget/rustyget.conf && \
  cp /app/rustyget/share/rustyget/rustyget.conf /app/rustyget/webui/rustyget.conf.template && \
  cp /app/rustyget/share/rustyget/rustyget.conf /app/rustyget/webui/nzbget.conf.template

# runtime stage
FROM alpine:3.22.2
ARG RUSTYGET_RELEASE=develop

# labels
LABEL org.opencontainers.image.description="RustyGet"
LABEL org.opencontainers.image.source="https://github.com/Appz4Fun/RustyGet"

ENV TERM=linux
COPY --from=build /app/rustyget/ /app/rustyget/
RUN \
  echo "**** install packages ****" && \
  apk add --no-cache --update shadow su-exec openssl tzdata && \
  ln -sf /app/rustyget /app/nzbget && \
  echo "**** cleanup ****" && \
  rm -rf /root/.cache /root/.cargo /tmp/*
COPY docker/entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh && \
  echo "**** create non-root user ****" && \
  adduser -G users -D -u 1000 -h /config -s /bin/sh user && \
  mkdir -p /config && \
  mkdir -p /downloads && \
  chown -R user:users /app /config /downloads
EXPOSE 6789
ENTRYPOINT [ "/entrypoint.sh" ]
