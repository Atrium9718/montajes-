<?php
// Macula · eventos de Wompi (webhook). Configúralo en el panel de Wompi como
// URL de eventos: https://<sitio>/api/wompi.php
//
// Se verifica la firma: SHA-256 de los valores de signature.properties, el
// timestamp y el secreto de eventos. Sin firma válida no se toca nada.

declare(strict_types=1);
require __DIR__ . '/lib/base.php';
require __DIR__ . '/lib/cobro.php';

if (($_SERVER['REQUEST_METHOD'] ?? '') !== 'POST') responder(405, ['error' => 'solo POST']);
$evento = cuerpo();
$secreto = wompi()['eventos'];
if ($secreto === '') responder(503, ['error' => 'sin secreto de eventos']);

$propiedades = $evento['signature']['properties'] ?? [];
$cadena = '';
foreach ($propiedades as $ruta) {
    $valor = $evento['data'] ?? [];
    foreach (explode('.', (string) $ruta) as $k) $valor = is_array($valor) ? ($valor[$k] ?? '') : '';
    $cadena .= is_scalar($valor) ? (string) $valor : '';
}
$esperado = hash('sha256', $cadena . ($evento['timestamp'] ?? '') . $secreto);
if (!$propiedades || !hash_equals($esperado, strtolower((string) ($evento['signature']['checksum'] ?? '')))) {
    bitacora(null, null, 'webhook_rechazado', ip());
    responder(401, ['error' => 'firma inválida']);
}

if (($evento['event'] ?? '') === 'transaction.updated' && isset($evento['data']['transaction'])) {
    $p = registrar_transaccion($evento['data']['transaction']);
    bitacora($p ? (int) $p['empresa_id'] : null, null, 'webhook', ($evento['data']['transaction']['reference'] ?? '') . ' ' . ($evento['data']['transaction']['status'] ?? ''));
}
responder(200, ['ok' => true]);
