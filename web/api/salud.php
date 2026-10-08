<?php
// Estado del servidor (sin datos privados): para verificar despliegues.
declare(strict_types=1);
header('Content-Type: application/json; charset=utf-8');
header('Cache-Control: no-store');
echo json_encode([
    'ok' => true,
    'php' => PHP_VERSION,
    'sqlite' => extension_loaded('pdo_sqlite'),
    'sodium' => function_exists('sodium_crypto_sign_detached'),
    'curl' => function_exists('curl_init'),
    'mail' => function_exists('mail'),
    'openssl' => function_exists('openssl_sign'),
]);
