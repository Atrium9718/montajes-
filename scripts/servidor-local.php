<?php
// Servidor de desarrollo que imita el .htaccess de Hostinger:
//   MACULA_DATOS=/tmp/datos php -S localhost:8772 -t web scripts/servidor-local.php
$ruta = parse_url($_SERVER['REQUEST_URI'], PHP_URL_PATH);
$raiz = dirname(__DIR__) . '/web';
if (preg_match('#^/api/lib/|(^|/)\.|\.zip$#', $ruta)) { http_response_code(403); exit; }
if (preg_match('#^/api/[a-z]+\.php$#', $ruta)) return false;
if (preg_match('#^/(acceso\.html|acceso\.js|estilos\.css|marca\.svg)$#', $ruta)) return false;
require $raiz . '/puerta.php';
