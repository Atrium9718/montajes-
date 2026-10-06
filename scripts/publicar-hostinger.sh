#!/usr/bin/env sh
# Publica web/ en Hostinger como sitio estático.
#
#   HOSTINGER_TOKEN=... scripts/publicar-hostinger.sh [dominio] [usuario]
#
# El token se lee de la variable de entorno; nunca lo guarde en el repositorio.
# ATENCIÓN: el despliegue reemplaza el contenido del sitio.
set -eu
DOMINIO="${1:-montajes.atrioagencia.com}"
USUARIO="${2:-u442727583}"
API="https://developers.hostinger.com/api/hosting/v1"
: "${HOSTINGER_TOKEN:?defina HOSTINGER_TOKEN}"
cd "$(dirname "$0")/.."

ZIP="$(mktemp -d)/montajes-web.zip"
(cd web && zip -q -r "$ZIP" index.html estilos.css app.js .htaccess motor)
TAMANO=$(wc -c < "$ZIP" | tr -d ' ')

campo() { python3 -c "import sys,json; print(json.load(sys.stdin)['$1'])"; }
R=$(curl -fsS -X POST "$API/files/upload-urls" \
  -H "Authorization: Bearer $HOSTINGER_TOKEN" -H "Content-Type: application/json" \
  -d "{\"username\":\"$USUARIO\",\"domain\":\"$DOMINIO\"}")
URL=$(echo "$R" | campo url); AK=$(echo "$R" | campo auth_key); RK=$(echo "$R" | campo rest_auth_key)

# Subida TUS 1.0.0 a public_html/montajes-web.zip
curl -fsS -o /dev/null -X POST "$URL/montajes-web.zip?override=true" \
  -H "X-Auth: $AK" -H "X-Auth-Rest: $RK" -H "Tus-Resumable: 1.0.0" \
  -H "Upload-Length: $TAMANO" -H "Upload-Offset: 0"
curl -fsS -o /dev/null -X PATCH "$URL/montajes-web.zip?override=true" \
  -H "X-Auth: $AK" -H "X-Auth-Rest: $RK" -H "Tus-Resumable: 1.0.0" \
  -H "Content-Type: application/offset+octet-stream" -H "Upload-Offset: 0" \
  --data-binary "@$ZIP"

curl -fsS -X POST "$API/accounts/$USUARIO/websites/$DOMINIO/deploy" \
  -H "Authorization: Bearer $HOSTINGER_TOKEN" -H "Content-Type: application/json" \
  -d '{"archive_path":"montajes-web.zip"}'
echo
echo "Publicado: https://$DOMINIO"
