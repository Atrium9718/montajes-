<?php
// Macula · puerta de la app. Todo lo que no es la página de acceso ni la API
// pasa por aquí (ver .htaccess): solo se entrega a una sesión cuya empresa
// esté al día (prueba, activa o en días de gracia). Sin pago no hay app.

declare(strict_types=1);
require __DIR__ . '/api/lib/base.php';

const TIPOS = [
    'html' => 'text/html; charset=utf-8', 'js' => 'text/javascript; charset=utf-8', 'mjs' => 'text/javascript; charset=utf-8',
    'css' => 'text/css; charset=utf-8', 'wasm' => 'application/wasm', 'json' => 'application/json', 'svg' => 'image/svg+xml',
    'png' => 'image/png', 'jpg' => 'image/jpeg', 'webp' => 'image/webp', 'ico' => 'image/x-icon', 'woff2' => 'font/woff2',
    'webmanifest' => 'application/manifest+json', 'txt' => 'text/plain; charset=utf-8',
];

$ruta = rawurldecode(parse_url((string) ($_SERVER['REQUEST_URI'] ?? '/'), PHP_URL_PATH) ?: '/');
$ruta = ltrim($ruta, '/');
if ($ruta === '' || str_ends_with($ruta, '/')) $ruta .= 'index.html';
$ext = strtolower(pathinfo($ruta, PATHINFO_EXTENSION));
$archivo = realpath(__DIR__ . '/' . $ruta);
$valido = $archivo !== false && str_starts_with($archivo, __DIR__ . DIRECTORY_SEPARATOR) && isset(TIPOS[$ext])
    && !preg_match('#(^|/)\.|(^|/)api/#', $ruta) && is_file($archivo);
if (!$valido) {
    http_response_code(404);
    exit('No encontrado');
}

$s = sesion();
if (!$s || !$s['suscripcion']['puede_usar']) {
    if ($ext === 'html') {
        header('Cache-Control: no-store');
        header('Location: /acceso.html' . ($s ? '#pagar' : ''), true, 302);
        exit;
    }
    http_response_code($s ? 402 : 401);
    header('Cache-Control: no-store');
    exit($s ? 'Suscripción vencida' : 'Inicia sesión');
}

header('Content-Type: ' . TIPOS[$ext]);
header('X-Content-Type-Options: nosniff');
// Privado: ningún intermediario guarda copias; el navegador revalida siempre.
header('Cache-Control: private, no-cache');
$etag = '"' . substr(sha1(filemtime($archivo) . ':' . filesize($archivo)), 0, 20) . '"';
header('ETag: ' . $etag);
if (($_SERVER['HTTP_IF_NONE_MATCH'] ?? '') === $etag) {
    http_response_code(304);
    exit;
}
header('Content-Length: ' . filesize($archivo));
readfile($archivo);
