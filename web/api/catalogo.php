<?php
// Catálogos del taller (máquinas y papeles) guardados en el servidor.
//
// Cada taller se identifica con un código secreto que se crea en la app. El
// archivo se guarda fuera de public_html, con el nombre del hash del código,
// y se conservan las últimas versiones como respaldo.
//
//   GET  api/catalogo.php            (cabecera X-Taller: código)  → catálogo
//   POST api/catalogo.php            (cabecera X-Taller, cuerpo JSON) → guarda

declare(strict_types=1);

header('Content-Type: application/json; charset=utf-8');
header('Cache-Control: no-store');
header('X-Content-Type-Options: nosniff');

const MAX_BYTES = 2_000_000;
const RESPALDOS = 30;

function responder(int $estado, array $datos): void {
    http_response_code($estado);
    echo json_encode($datos, JSON_UNESCAPED_UNICODE);
    exit;
}

$codigo = $_SERVER['HTTP_X_TALLER'] ?? '';
if (!preg_match('/^[A-Za-z0-9-]{16,64}$/', $codigo)) {
    responder(400, ['error' => 'código de taller inválido']);
}

// domains/<sitio>/datos-montajes: fuera de public_html, no se publica ni se borra al desplegar.
$carpeta = dirname(__DIR__, 2) . '/datos-montajes';
if (!is_dir($carpeta) && !mkdir($carpeta, 0700, true) && !is_dir($carpeta)) {
    responder(500, ['error' => 'no se pudo crear la carpeta de datos']);
}
$id = hash('sha256', 'montajes:' . $codigo);
$archivo = "$carpeta/$id.json";

$metodo = $_SERVER['REQUEST_METHOD'] ?? 'GET';

if ($metodo === 'GET') {
    if (!is_file($archivo)) {
        responder(200, ['nuevo' => true]);
    }
    $contenido = file_get_contents($archivo);
    echo $contenido === false ? '{}' : $contenido;
    exit;
}

if ($metodo === 'POST') {
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
