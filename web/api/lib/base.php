<?php
// Macula · base del servidor: datos (SQLite), sesiones, estado de la
// suscripción de cada empresa, licencias firmadas y correo.
//
// Los datos viven fuera de public_html (domains/<sitio>/datos-montajes), así
// no se publican ni se borran al desplegar. Las llaves de la pasarela las
// guarda el administrador desde el panel; nunca están en el repositorio.

declare(strict_types=1);

const SESION_COOKIE = 'macula_sesion';
const SESION_DIAS = 30;
const LICENCIA_HORAS = 72; // la app sigue funcionando sin conexión hasta 3 días

/** Planes por defecto (el administrador cambia precios y límites en el panel). */
const PLANES_BASE = [
    'independiente' => ['nombre' => 'Independiente', 'usuarios' => 1, 'mensual' => 79000, 'descripcion' => 'Un usuario. Imposición, diagramación, portadas y cotizador.'],
    'taller' => ['nombre' => 'Taller', 'usuarios' => 5, 'mensual' => 189000, 'descripcion' => 'Hasta 5 usuarios con catálogos compartidos en la nube del taller.'],
    'empresa' => ['nombre' => 'Empresa', 'usuarios' => 20, 'mensual' => 390000, 'descripcion' => 'Hasta 20 usuarios, varias sedes y soporte prioritario.'],
];

function carpeta_datos(): string {
    $c = getenv('MACULA_DATOS') ?: dirname(__DIR__, 3) . '/datos-montajes';
    if (!is_dir($c) && !mkdir($c, 0700, true) && !is_dir($c)) {
        responder(500, ['error' => 'no se pudo crear la carpeta de datos']);
    }
    return $c;
}

function responder(int $estado, array $datos): never {
    http_response_code($estado);
    header('Content-Type: application/json; charset=utf-8');
    header('Cache-Control: no-store');
    header('X-Content-Type-Options: nosniff');
    echo json_encode($datos, JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES);
    exit;
}

function cuerpo(): array {
    $t = file_get_contents('php://input', false, null, 0, 200_000);
    $d = json_decode($t ?: '[]', true);
    return is_array($d) ? $d : [];
}

function ahora(): int {
    return time();
}

// ───────────── Base de datos ─────────────
function bd(): PDO {
    static $pdo = null;
    if ($pdo) return $pdo;
    $pdo = new PDO('sqlite:' . carpeta_datos() . '/macula.sqlite', null, null, [
        PDO::ATTR_ERRMODE => PDO::ERRMODE_EXCEPTION,
        PDO::ATTR_DEFAULT_FETCH_MODE => PDO::FETCH_ASSOC,
    ]);
    $pdo->exec('PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;');
    $pdo->exec(<<<SQL
        CREATE TABLE IF NOT EXISTS empresas (
            id INTEGER PRIMARY KEY, nombre TEXT NOT NULL, documento TEXT NOT NULL, tipo TEXT NOT NULL,
            email TEXT NOT NULL, telefono TEXT NOT NULL DEFAULT '', ciudad TEXT NOT NULL DEFAULT '',
            plan TEXT NOT NULL DEFAULT 'taller', periodo TEXT NOT NULL DEFAULT 'mensual',
            bloqueada INTEGER NOT NULL DEFAULT 0, aliado INTEGER NOT NULL DEFAULT 0, descuento INTEGER NOT NULL DEFAULT 0,
            prueba_hasta INTEGER NOT NULL DEFAULT 0, pagado_hasta INTEGER NOT NULL DEFAULT 0,
            fuente_pago TEXT, fuente_desc TEXT, renovar INTEGER NOT NULL DEFAULT 1,
            intentos INTEGER NOT NULL DEFAULT 0, ultimo_intento INTEGER NOT NULL DEFAULT 0,
            avisos TEXT NOT NULL DEFAULT '{}', notas TEXT NOT NULL DEFAULT '', creada INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS usuarios (
            id INTEGER PRIMARY KEY, empresa_id INTEGER NOT NULL REFERENCES empresas(id),
            email TEXT NOT NULL UNIQUE, nombre TEXT NOT NULL, clave TEXT NOT NULL,
            rol TEXT NOT NULL, activo INTEGER NOT NULL DEFAULT 1, verificado INTEGER NOT NULL DEFAULT 0,
            creado INTEGER NOT NULL, ultimo_ingreso INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS sesiones (
            token TEXT PRIMARY KEY, usuario_id INTEGER NOT NULL REFERENCES usuarios(id) ON DELETE CASCADE,
            expira INTEGER NOT NULL, creada INTEGER NOT NULL, ip TEXT NOT NULL DEFAULT '', agente TEXT NOT NULL DEFAULT ''
        );
        CREATE TABLE IF NOT EXISTS codigos (
            email TEXT NOT NULL, proposito TEXT NOT NULL, codigo TEXT NOT NULL, expira INTEGER NOT NULL,
            intentos INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (email, proposito)
        );
        CREATE TABLE IF NOT EXISTS pagos (
            id INTEGER PRIMARY KEY, empresa_id INTEGER NOT NULL REFERENCES empresas(id), referencia TEXT NOT NULL UNIQUE,
            monto INTEGER NOT NULL, moneda TEXT NOT NULL DEFAULT 'COP', plan TEXT NOT NULL, periodo TEXT NOT NULL,
            estado TEXT NOT NULL DEFAULT 'PENDING', metodo TEXT NOT NULL DEFAULT '', pasarela_id TEXT NOT NULL DEFAULT '',
            automatico INTEGER NOT NULL DEFAULT 0, aplicado INTEGER NOT NULL DEFAULT 0, detalle TEXT NOT NULL DEFAULT '',
            creado INTEGER NOT NULL, actualizado INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS pruebas_usadas (huella TEXT PRIMARY KEY, empresa_id INTEGER, momento INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS intentos (clave TEXT NOT NULL, momento INTEGER NOT NULL);
        CREATE INDEX IF NOT EXISTS intentos_clave ON intentos (clave, momento);
        CREATE TABLE IF NOT EXISTS bitacora (id INTEGER PRIMARY KEY, empresa_id INTEGER, usuario_id INTEGER, evento TEXT NOT NULL, detalle TEXT NOT NULL DEFAULT '', momento INTEGER NOT NULL);
    SQL);
    return $pdo;
}

function uno(string $sql, array $p = []): ?array {
    $s = bd()->prepare($sql);
    $s->execute($p);
    $f = $s->fetch();
    return $f === false ? null : $f;
}

function todos(string $sql, array $p = []): array {
    $s = bd()->prepare($sql);
    $s->execute($p);
    return $s->fetchAll();
}

function ejecutar(string $sql, array $p = []): int {
    $s = bd()->prepare($sql);
    $s->execute($p);
    return $s->rowCount();
}

function bitacora(?int $empresa, ?int $usuario, string $evento, string $detalle = ''): void {
    ejecutar('INSERT INTO bitacora (empresa_id, usuario_id, evento, detalle, momento) VALUES (?, ?, ?, ?, ?)', [$empresa, $usuario, $evento, $detalle, ahora()]);
}

// ───────────── Configuración (llaves y precios, solo en el servidor) ─────────────
function config(bool $recargar = false): array {
    static $c = null;
    if ($c !== null && !$recargar) return $c;
    $archivo = carpeta_datos() . '/config.json';
    $guardada = is_file($archivo) ? (json_decode((string) file_get_contents($archivo), true) ?: []) : [];
    $c = array_replace_recursive([
        'nombre' => 'Macula',
        'prueba_dias' => 14,
        'gracia_dias' => 3,
        'anual_meses' => 10, // el plan anual cobra 10 meses (2 gratis)
        'moneda' => 'COP',
        'planes' => PLANES_BASE,
        'wompi' => ['modo' => 'sandbox', 'publica' => '', 'privada' => '', 'eventos' => '', 'integridad' => ''],
        'correo_remitente' => '',
    ], $guardada);
    return $c;
}

function guardar_config(array $cambios): array {
    $actual = config();
    $nueva = array_replace_recursive($actual, $cambios);
    // Los planes se reemplazan completos para poder quitar alguno.
    if (isset($cambios['planes'])) $nueva['planes'] = $cambios['planes'];
    $archivo = carpeta_datos() . '/config.json';
    file_put_contents($archivo, json_encode($nueva, JSON_PRETTY_PRINT | JSON_UNESCAPED_UNICODE), LOCK_EX);
    chmod($archivo, 0600);
    return config(true);
}

function plan(string $id): ?array {
    $p = config()['planes'][$id] ?? null;
    return $p ? ['id' => $id] + $p : null;
}

/** Precio en pesos de un plan para una empresa (periodo y descuento de aliado). */
function precio(array $empresa, ?string $plan = null, ?string $periodo = null): int {
    $p = plan($plan ?? $empresa['plan']);
    if (!$p) return 0;
    $meses = ($periodo ?? $empresa['periodo']) === 'anual' ? (int) config()['anual_meses'] : 1;
    $total = (int) $p['mensual'] * $meses;
    $descuento = max(0, min(100, (int) $empresa['descuento']));
    return (int) round($total * (100 - $descuento) / 100);
}

// ───────────── Estado de la suscripción ─────────────
/**
 * activa · prueba · gracia (cobro vencido, aún funciona) · vencida · bloqueada.
 * Solo «activa», «prueba» y «gracia» dejan usar la app.
 */
function estado_empresa(array $e): array {
    $t = ahora();
    $gracia = (int) config()['gracia_dias'] * 86400;
    $acceso_hasta = max((int) $e['prueba_hasta'], (int) $e['pagado_hasta'] > 0 ? (int) $e['pagado_hasta'] + $gracia : 0);
    if ((int) $e['bloqueada']) $estado = 'bloqueada';
    elseif ($t < (int) $e['pagado_hasta']) $estado = 'activa';
    elseif ($t < (int) $e['prueba_hasta']) $estado = 'prueba';
    elseif ((int) $e['pagado_hasta'] > 0 && $t < (int) $e['pagado_hasta'] + $gracia) $estado = 'gracia';
    else $estado = 'vencida';
    $puede = in_array($estado, ['activa', 'prueba', 'gracia'], true);
    return ['estado' => $estado, 'puede_usar' => $puede, 'acceso_hasta' => $puede ? $acceso_hasta : 0];
}

function empresa(int $id): ?array {
    return uno('SELECT * FROM empresas WHERE id = ?', [$id]);
}

// ───────────── Sesiones ─────────────
function ip(): string {
    return substr((string) ($_SERVER['REMOTE_ADDR'] ?? ''), 0, 64);
}

function crear_sesion(int $usuario): void {
    $token = bin2hex(random_bytes(32));
    ejecutar('INSERT INTO sesiones (token, usuario_id, expira, creada, ip, agente) VALUES (?, ?, ?, ?, ?, ?)', [
        hash('sha256', $token), $usuario, ahora() + SESION_DIAS * 86400, ahora(), ip(), substr((string) ($_SERVER['HTTP_USER_AGENT'] ?? ''), 0, 200),
    ]);
    ejecutar('UPDATE usuarios SET ultimo_ingreso = ? WHERE id = ?', [ahora(), $usuario]);
    poner_cookie($token, ahora() + SESION_DIAS * 86400);
}

function poner_cookie(string $valor, int $expira): void {
    $segura = ($_SERVER['HTTPS'] ?? '') !== '' && ($_SERVER['HTTPS'] ?? '') !== 'off' || ($_SERVER['HTTP_X_FORWARDED_PROTO'] ?? '') === 'https';
    setcookie(SESION_COOKIE, $valor, ['expires' => $expira, 'path' => '/', 'secure' => $segura, 'httponly' => true, 'samesite' => 'Lax']);
}

function cerrar_sesion(): void {
    $token = $_COOKIE[SESION_COOKIE] ?? '';
    if ($token !== '') ejecutar('DELETE FROM sesiones WHERE token = ?', [hash('sha256', $token)]);
    poner_cookie('', 1);
}

/** Usuario de la sesión (con su empresa y estado), o null. */
function sesion(): ?array {
    static $cache = false;
    if ($cache !== false) return $cache;
    $token = $_COOKIE[SESION_COOKIE] ?? '';
    $cache = null;
    if (!preg_match('/^[a-f0-9]{64}$/', $token)) return null;
    $u = uno('SELECT u.* FROM sesiones s JOIN usuarios u ON u.id = s.usuario_id WHERE s.token = ? AND s.expira > ? AND u.activo = 1', [hash('sha256', $token), ahora()]);
    if (!$u) return null;
    $e = empresa((int) $u['empresa_id']);
    if (!$e) return null;
    // Sesión deslizante: renueva el vencimiento una vez al día.
    ejecutar('UPDATE sesiones SET expira = ? WHERE token = ? AND expira < ?', [ahora() + SESION_DIAS * 86400, hash('sha256', $token), ahora() + (SESION_DIAS - 1) * 86400]);
    $estado = $u['rol'] === 'super' ? ['estado' => 'activa', 'puede_usar' => true, 'acceso_hasta' => ahora() + 365 * 86400] : estado_empresa($e);
    return $cache = ['usuario' => $u, 'empresa' => $e, 'suscripcion' => $estado];
}

function exigir_sesion(): array {
    $s = sesion();
    if (!$s) responder(401, ['error' => 'inicia sesión', 'codigo' => 'sin_sesion']);
    return $s;
}

function exigir_rol(array $s, array $roles): void {
    if ($s['usuario']['rol'] !== 'super' && !in_array($s['usuario']['rol'], $roles, true)) {
        responder(403, ['error' => 'no tienes permiso para esto']);
    }
}

// ───────────── Límite de intentos ─────────────
function limitar(string $clave, int $maximo, int $segundos): void {
    ejecutar('DELETE FROM intentos WHERE momento < ?', [ahora() - 86400]);
    $n = (int) (uno('SELECT COUNT(*) n FROM intentos WHERE clave = ? AND momento > ?', [$clave, ahora() - $segundos])['n'] ?? 0);
    if ($n >= $maximo) responder(429, ['error' => 'demasiados intentos; espera unos minutos']);
    ejecutar('INSERT INTO intentos (clave, momento) VALUES (?, ?)', [$clave, ahora()]);
}

// ───────────── Licencia firmada para el motor ─────────────
/** Llave privada P-256 (PEM), guardada en la carpeta de datos al instalar. */
function llave_licencia(): ?OpenSSLAsymmetricKey {
    $archivo = carpeta_datos() . '/licencia.pem';
    if (!is_file($archivo)) return null;
    $k = openssl_pkey_get_private((string) file_get_contents($archivo));
    return $k ?: null;
}

function b64url(string $b): string {
    return rtrim(strtr(base64_encode($b), '+/', '-_'), '=');
}

/** Token que el motor (WebAssembly) exige para generar PDF: empresa, plan y vigencia. */
function licencia(array $s): ?string {
    $k = llave_licencia();
    if (!$k || !$s['suscripcion']['puede_usar']) return null;
    $hasta = min(ahora() + LICENCIA_HORAS * 3600, (int) $s['suscripcion']['acceso_hasta']);
    $datos = json_encode([
        'e' => (int) $s['empresa']['id'], 'n' => $s['empresa']['nombre'], 'p' => $s['empresa']['plan'],
        'h' => $hasta, 'i' => ahora(),
    ], JSON_UNESCAPED_UNICODE);
    if (!openssl_sign($datos, $firma, $k, OPENSSL_ALGO_SHA256)) return null;
    return b64url($datos) . '.' . b64url($firma);
}

// ───────────── Correo ─────────────
function enviar_correo(string $para, string $asunto, string $texto): bool {
    $c = config();
    $host = preg_replace('/^www\./', '', (string) ($_SERVER['HTTP_HOST'] ?? 'localhost'));
    $de = $c['correo_remitente'] ?: "no-responder@$host";
    $cabeceras = implode("\r\n", [
        'From: ' . mb_encode_mimeheader($c['nombre']) . " <$de>",
        'Content-Type: text/plain; charset=UTF-8',
        'Content-Transfer-Encoding: 8bit',
    ]);
    if (getenv('MACULA_CORREO_ARCHIVO')) { // pruebas locales
        file_put_contents(getenv('MACULA_CORREO_ARCHIVO'), "Para: $para\nAsunto: $asunto\n\n$texto\n---\n", FILE_APPEND);
        return true;
    }
    return @mail($para, mb_encode_mimeheader($asunto), $texto, $cabeceras);
}

function crear_codigo(string $email, string $proposito): string {
    $codigo = str_pad((string) random_int(0, 999999), 6, '0', STR_PAD_LEFT);
    ejecutar('INSERT OR REPLACE INTO codigos (email, proposito, codigo, expira, intentos) VALUES (?, ?, ?, ?, 0)', [$email, $proposito, hash('sha256', $codigo), ahora() + 1800]);
    return $codigo;
}

function validar_codigo(string $email, string $proposito, string $codigo): bool {
    $f = uno('SELECT * FROM codigos WHERE email = ? AND proposito = ?', [$email, $proposito]);
    if (!$f || (int) $f['expira'] < ahora() || (int) $f['intentos'] >= 5) return false;
    if (!hash_equals($f['codigo'], hash('sha256', trim($codigo)))) {
        ejecutar('UPDATE codigos SET intentos = intentos + 1 WHERE email = ? AND proposito = ?', [$email, $proposito]);
        return false;
    }
    ejecutar('DELETE FROM codigos WHERE email = ? AND proposito = ?', [$email, $proposito]);
    return true;
}

// ───────────── Utilidades ─────────────
function normalizar_email(string $e): string {
    return strtolower(trim($e));
}

function normalizar_documento(string $d): string {
    // NIT con o sin dígito de verificación: se guarda solo el número base.
    $d = preg_replace('/[^0-9]/', '', explode('-', $d)[0]) ?? '';
    return ltrim($d, '0');
}

function normalizar_telefono(string $t): string {
    $t = preg_replace('/[^0-9]/', '', $t) ?? '';
    return strlen($t) > 10 ? substr($t, -10) : $t;
}

function pesos(int $v): string {
    return '$' . number_format($v, 0, ',', '.');
}

function fecha(int $t): string {
    $meses = ['enero', 'febrero', 'marzo', 'abril', 'mayo', 'junio', 'julio', 'agosto', 'septiembre', 'octubre', 'noviembre', 'diciembre'];
    return date('j', $t) . ' de ' . $meses[(int) date('n', $t) - 1] . ' de ' . date('Y', $t);
}

/** Datos de la sesión para el navegador (sin secretos). */
function resumen_sesion(array $s): array {
    $e = $s['empresa'];
    $p = plan($e['plan']);
    $usuarios = (int) (uno('SELECT COUNT(*) n FROM usuarios WHERE empresa_id = ? AND activo = 1', [$e['id']])['n'] ?? 0);
    return [
        'usuario' => ['id' => (int) $s['usuario']['id'], 'nombre' => $s['usuario']['nombre'], 'email' => $s['usuario']['email'], 'rol' => $s['usuario']['rol']],
        'empresa' => [
            'id' => (int) $e['id'], 'nombre' => $e['nombre'], 'documento' => $e['documento'], 'tipo' => $e['tipo'],
            'plan' => $e['plan'], 'plan_nombre' => $p['nombre'] ?? $e['plan'], 'periodo' => $e['periodo'],
            'usuarios' => $usuarios, 'usuarios_max' => (int) ($p['usuarios'] ?? 1),
            'prueba_hasta' => (int) $e['prueba_hasta'], 'pagado_hasta' => (int) $e['pagado_hasta'],
            'tarjeta' => $e['fuente_desc'], 'renovar' => (bool) $e['renovar'], 'aliado' => (bool) $e['aliado'], 'descuento' => (int) $e['descuento'],
            'precio' => precio($e),
        ],
        'suscripcion' => $s['suscripcion'] + ['gracia_dias' => (int) config()['gracia_dias']],
        'licencia' => licencia($s),
    ];
}
