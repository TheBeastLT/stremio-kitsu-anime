#!/bin/sh
set -e

for name in mongodb_uri metrics_user metrics_password fanart_apikey; do
  file="/run/secrets/$name"
  if [ -f "$file" ]; then
    export "$(printf '%s' "$name" | tr '[:lower:]' '[:upper:]')=$(cat "$file")"
  fi
done

if [ "$#" -eq 0 ]; then
  set -- node index.js
fi

exec "$@"
