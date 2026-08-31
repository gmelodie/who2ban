#!/bin/sh
# Serves https from the moment it starts: a self-signed certificate holds the
# port until certbot lands the real one, then nginx reloads within a minute.
set -eu

LIVE="/etc/letsencrypt/live/$DOMAIN"
FALLBACK="/etc/letsencrypt/selfsigned/$DOMAIN"

command -v openssl >/dev/null 2>&1 || apk add --no-cache openssl

if [ ! -f "$FALLBACK/fullchain.pem" ]; then
    mkdir -p "$FALLBACK"
    openssl req -x509 -newkey rsa:2048 -nodes -days 3650 \
        -subj "/CN=$DOMAIN" \
        -keyout "$FALLBACK/privkey.pem" \
        -out "$FALLBACK/fullchain.pem" 2>/dev/null
fi

hash="$(printf '%s\n' "$BASIC_PASS" | openssl passwd -apr1 -stdin)"
printf '%s:%s\n' "$BASIC_USER" "$hash" > /etc/nginx/htpasswd

write_config() {
    if [ -f "$LIVE/fullchain.pem" ]; then
        CERT_DIR="$LIVE"
    else
        CERT_DIR="$FALLBACK"
    fi
    export CERT_DIR DOMAIN
    # shellcheck disable=SC2016  # envsubst wants the names, not their values
    envsubst '${DOMAIN} ${CERT_DIR}' \
        < /etc/nginx/nginx.conf.template > /etc/nginx/nginx.conf
    echo "$CERT_DIR $(md5sum "$CERT_DIR/fullchain.pem" | cut -d' ' -f1)"
}

serving="$(write_config)"
case "$serving" in
    "$LIVE"*) echo "serving the certificate of $DOMAIN" ;;
    *) echo "serving a self-signed certificate until certbot answers" ;;
esac

nginx -g 'daemon off;' &
nginx_pid=$!

while sleep 60; do
    kill -0 "$nginx_pid" 2>/dev/null || break
    current="$(write_config)"
    if [ "$current" != "$serving" ]; then
        serving="$current"
        echo "certificate changed, reloading"
        nginx -s reload
    fi
done &

wait "$nginx_pid"
