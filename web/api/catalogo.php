<?php
// Catálogos de la empresa (máquinas y papeles) guardados en el servidor.
//
// Cada empresa tiene el suyo (según la sesión). Se guarda fuera de
// public_html y se conservan las últimas versiones como respaldo.
//
//   GET  api/catalogo.php                 → catálogo
//   POST api/catalogo.php  (cuerpo JSON)  → guarda

declare(strict_types=1);

const MAX_BYTES = 2_000_000;
const RESPALDOS = 30;

require __DIR__ . '/lib/base.php';

// El catálogo es de la empresa de la sesión, y solo con la suscripción al día.
$sesion = exigir_sesion();
if (!$sesion['suscripcion']['puede_usar']) {
    responder(402, ['error' => 'la suscripción no está al día', 'codigo' => 'vencida']);
}

$carpeta = carpeta_datos();
$id = hash('sha256', 'macula-empresa:' . $sesion['empresa']['id']);
$archivo = "$carpeta/$id.json";

// Migración: el catálogo guardado antes con el código del taller pasa a la empresa.
$codigo = $_SERVER['HTTP_X_TALLER'] ?? '';
if (!is_file($archivo) && preg_match('/^[A-Za-z0-9-]{16,64}$/', $codigo)) {
    $viejo = "$carpeta/" . hash('sha256', 'montajes:' . $codigo) . '.json';
    if (is_file($viejo)) copy($viejo, $archivo);
}

$metodo = $_SERVER['REQUEST_METHOD'] ?? 'GET';

if ($metodo === 'GET') {
    if (!is_file($archivo)) {
        responder(200, ['nuevo' => true]);
    }
    $contenido = file_get_contents($archivo);
    header('Content-Type: application/json; charset=utf-8');
    header('Cache-Control: no-store');
    echo $contenido === false ? '{}' : $contenido;
    exit;
}

if ($metodo === 'POST') {
    if (($_SERVER['HTTP_X_MACULA'] ?? '') !== '1') responder(400, ['error' => 'petición inválida']);
    $cuerpo = file_get_contents('php://input', false, null, 0, MAX_BYTES + 1);
    if ($cuerpo === false || strlen($cuerpo) > MAX_BYTES) {
        responder(413, ['error' => 'el catálogo es demasiado grande']);
    }
    $datos = json_decode($cuerpo, true);
    if (!is_array($datos) || !isset($datos['maquinas'], $datos['papeles'])
        || !is_array($datos['maquinas']) || !is_array($datos['papeles'])) {
        responder(400, ['error' => 'formato de catálogo inválido']);
    }
    $guardar = [
        'formato' => 'montajes-catalogo',
        'version' => 1,
        'actualizado' => time(),
        'maquinas' => array_values($datos['maquinas']),
        'papeles' => array_values($datos['papeles']),
        'borrados' => is_array($datos['borrados'] ?? null) ? $datos['borrados'] : new stdClass(),
    ];
    // Sin cambios: no se reescribe ni se crea un respaldo.
    if (is_file($archivo)) {
        $previo = json_decode((string) file_get_contents($archivo), true);
        $igual = fn(string $k) => json_encode($previo[$k] ?? null) === json_encode($guardar[$k]);
        if (is_array($previo) && $igual('maquinas') && $igual('papeles') && $igual('borrados')) {
            responder(200, ['ok' => true, 'actualizado' => $previo['actualizado'] ?? time(), 'sinCambios' => true]);
        }
    }
    // Respaldo de la versión anterior antes de reemplazarla.
    if (is_file($archivo)) {
        copy($archivo, "$carpeta/$id." . date('Ymd-His') . '.json');
        $viejos = glob("$carpeta/$id.*.json") ?: [];
        sort($viejos);
        foreach (array_slice($viejos, 0, max(0, count($viejos) - RESPALDOS)) as $v) {
            @unlink($v);
        }
    }
    $temporal = "$archivo.tmp";
    if (file_put_contents($temporal, json_encode($guardar, JSON_UNESCAPED_UNICODE), LOCK_EX) === false
        || !rename($temporal, $archivo)) {
        responder(500, ['error' => 'no se pudo guardar']);
    }
    responder(200, ['ok' => true, 'actualizado' => $guardar['actualizado']]);
}

responder(405, ['error' => 'método no permitido']);
