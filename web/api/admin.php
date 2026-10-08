<?php
// Macula · panel del dueño del servicio (superadministrador): empresas,
// pagos, precios, prueba y gracia, y llaves de la pasarela.
//
//   POST api/admin.php?accion=instalar        (una sola vez, con el código de instalación)
//   GET  api/admin.php?accion=resumen | config | bitacora
//   POST api/admin.php?accion=empresa | config

declare(strict_types=1);
require __DIR__ . '/lib/base.php';
require __DIR__ . '/lib/cobro.php';

/** SHA-256 del código de instalación (el código no está en el repositorio). */
const INSTALAR_HASH = 'e1125f560a22f1f24b08a4b601d37250fc24ba9cf5fe10203fe6ba78f2ec53f4';

$accion = $_GET['accion'] ?? '';
$metodo = $_SERVER['REQUEST_METHOD'] ?? 'GET';
if ($metodo === 'POST' && ($_SERVER['HTTP_X_MACULA'] ?? '') !== '1') responder(400, ['error' => 'petición inválida']);
$d = $metodo === 'POST' ? cuerpo() : [];

if ($accion === 'instalar') {
    limitar('instalar:' . ip(), 5, 3600);
    if (!hash_equals(INSTALAR_HASH, hash('sha256', (string) ($d['codigo'] ?? '')))) responder(403, ['error' => 'código de instalación inválido']);
    if (uno("SELECT 1 FROM usuarios WHERE rol = 'super'")) responder(409, ['error' => 'Macula ya está instalada']);
    $email = normalizar_email((string) ($d['email'] ?? ''));
    if (!filter_var($email, FILTER_VALIDATE_EMAIL) || strlen((string) ($d['clave'] ?? '')) < 10) responder(400, ['error' => 'correo y contraseña (10+) requeridos']);
    $pem = (string) ($d['llave'] ?? '');
    if (!openssl_pkey_get_private($pem)) responder(400, ['error' => 'llave de licencias inválida']);
    file_put_contents(carpeta_datos() . '/licencia.pem', $pem, LOCK_EX);
    chmod(carpeta_datos() . '/licencia.pem', 0600);
    ejecutar('INSERT INTO empresas (nombre, documento, tipo, email, plan, periodo, creada) VALUES (?, ?, ?, ?, ?, ?, ?)', ['Macula', '0', 'empresa', $email, 'empresa', 'anual', ahora()]);
    $empresa = (int) bd()->lastInsertId();
    ejecutar('INSERT INTO usuarios (empresa_id, email, nombre, clave, rol, verificado, creado) VALUES (?, ?, ?, ?, ?, 1, ?)', [
        $empresa, $email, trim((string) ($d['nombre'] ?? 'Administrador')), password_hash((string) $d['clave'], PASSWORD_DEFAULT), 'super', ahora(),
    ]);
    bitacora($empresa, null, 'instalar', $email);
    responder(200, ['ok' => true]);
}

$s = exigir_sesion();
if ($s['usuario']['rol'] !== 'super') responder(403, ['error' => 'solo el administrador de Macula']);

switch ($accion) {
    case 'resumen':
        $empresas = [];
        $ingreso_mensual = 0;
        $conteo = ['activa' => 0, 'prueba' => 0, 'gracia' => 0, 'vencida' => 0, 'bloqueada' => 0];
        foreach (todos("SELECT e.*, (SELECT COUNT(*) FROM usuarios u WHERE u.empresa_id = e.id AND u.activo = 1) usuarios,
                (SELECT MAX(ultimo_ingreso) FROM usuarios u WHERE u.empresa_id = e.id) ultimo_ingreso,
                (SELECT COALESCE(SUM(monto), 0) FROM pagos p WHERE p.empresa_id = e.id AND p.estado = 'APPROVED') pagado_total
                FROM empresas e WHERE NOT EXISTS (SELECT 1 FROM usuarios u WHERE u.empresa_id = e.id AND u.rol = 'super') ORDER BY e.id DESC") as $e) {
            $st = estado_empresa($e);
            $conteo[$st['estado']]++;
            if ($st['estado'] === 'activa') $ingreso_mensual += (int) round(precio($e) / ($e['periodo'] === 'anual' ? 12 : 1));
            $empresas[] = [
                'id' => (int) $e['id'], 'nombre' => $e['nombre'], 'documento' => $e['documento'], 'tipo' => $e['tipo'], 'email' => $e['email'],
                'telefono' => $e['telefono'], 'plan' => $e['plan'], 'periodo' => $e['periodo'], 'estado' => $st['estado'], 'acceso_hasta' => $st['acceso_hasta'],
                'prueba_hasta' => (int) $e['prueba_hasta'], 'pagado_hasta' => (int) $e['pagado_hasta'], 'tarjeta' => $e['fuente_desc'], 'renovar' => (bool) $e['renovar'],
                'aliado' => (bool) $e['aliado'], 'descuento' => (int) $e['descuento'], 'bloqueada' => (bool) $e['bloqueada'], 'usuarios' => (int) $e['usuarios'],
                'ultimo_ingreso' => (int) $e['ultimo_ingreso'], 'pagado_total' => (int) $e['pagado_total'], 'precio' => precio($e), 'notas' => $e['notas'], 'creada' => (int) $e['creada'],
            ];
        }
        $pagos = todos('SELECT p.referencia, p.monto, p.plan, p.periodo, p.estado, p.metodo, p.automatico, p.creado, e.nombre empresa FROM pagos p JOIN empresas e ON e.id = p.empresa_id ORDER BY p.id DESC LIMIT 100');
        responder(200, ['empresas' => $empresas, 'conteo' => $conteo, 'ingreso_mensual' => $ingreso_mensual, 'pagos' => $pagos, 'pasarela' => wompi_lista(), 'modo' => wompi()['modo']]);

    case 'empresa':
        $e = empresa((int) ($d['id'] ?? 0));
        if (!$e) responder(404, ['error' => 'empresa no encontrada']);
        if (isset($d['dias'])) { // regalar o quitar días de acceso (cortesía, pago en efectivo o transferencia)
            $dias = (int) $d['dias'];
            $base = max(ahora(), (int) $e['pagado_hasta']);
            ejecutar('UPDATE empresas SET pagado_hasta = ? WHERE id = ?', [max(0, $base + $dias * 86400), $e['id']]);
            if (!empty($d['registrar_pago'])) {
                $ref = sprintf('MAN-%d-%s', $e['id'], date('ymdHis'));
                ejecutar("INSERT INTO pagos (empresa_id, referencia, monto, plan, periodo, estado, metodo, aplicado, creado, actualizado) VALUES (?, ?, ?, ?, ?, 'APPROVED', 'MANUAL', 1, ?, ?)", [
                    $e['id'], $ref, (int) $d['registrar_pago'], $e['plan'], $e['periodo'], ahora(), ahora(),
                ]);
            }
        }
        foreach (['bloqueada', 'aliado', 'renovar'] as $k) if (isset($d[$k])) ejecutar("UPDATE empresas SET $k = ? WHERE id = ?", [$d[$k] ? 1 : 0, $e['id']]);
        if (isset($d['descuento'])) ejecutar('UPDATE empresas SET descuento = ? WHERE id = ?', [max(0, min(100, (int) $d['descuento'])), $e['id']]);
        if (isset($d['plan']) && plan((string) $d['plan'])) ejecutar('UPDATE empresas SET plan = ? WHERE id = ?', [(string) $d['plan'], $e['id']]);
        if (isset($d['periodo'])) ejecutar('UPDATE empresas SET periodo = ? WHERE id = ?', [$d['periodo'] === 'anual' ? 'anual' : 'mensual', $e['id']]);
        if (isset($d['notas'])) ejecutar('UPDATE empresas SET notas = ? WHERE id = ?', [mb_substr((string) $d['notas'], 0, 2000), $e['id']]);
        if (isset($d['prueba_dias'])) ejecutar('UPDATE empresas SET prueba_hasta = ? WHERE id = ?', [ahora() + max(0, (int) $d['prueba_dias']) * 86400, $e['id']]);
        if (!empty($d['bloqueada'])) ejecutar('DELETE FROM sesiones WHERE usuario_id IN (SELECT id FROM usuarios WHERE empresa_id = ?)', [$e['id']]);
        bitacora((int) $e['id'], (int) $s['usuario']['id'], 'admin', json_encode($d, JSON_UNESCAPED_UNICODE));
        responder(200, ['ok' => true]);

    case 'config':
        if ($metodo === 'POST') {
            $c = [];
            foreach (['prueba_dias', 'gracia_dias', 'anual_meses'] as $k) if (isset($d[$k])) $c[$k] = max(0, (int) $d[$k]);
            if (isset($d['correo_remitente'])) $c['correo_remitente'] = filter_var($d['correo_remitente'], FILTER_VALIDATE_EMAIL) ?: '';
            if (isset($d['planes']) && is_array($d['planes'])) {
                $planes = [];
                foreach ($d['planes'] as $id => $p) {
                    if (!preg_match('/^[a-z0-9_-]{2,30}$/', (string) $id)) continue;
                    $planes[$id] = ['nombre' => mb_substr((string) ($p['nombre'] ?? $id), 0, 40), 'usuarios' => max(1, (int) ($p['usuarios'] ?? 1)),
                        'mensual' => max(0, (int) ($p['mensual'] ?? 0)), 'descripcion' => mb_substr((string) ($p['descripcion'] ?? ''), 0, 200)];
                }
                if ($planes) $c['planes'] = $planes;
            }
            if (isset($d['wompi']) && is_array($d['wompi'])) {
                $w = [];
                if (isset($d['wompi']['modo'])) $w['modo'] = $d['wompi']['modo'] === 'produccion' ? 'produccion' : 'sandbox';
                // Las llaves solo se reemplazan si llegan (el panel nunca las vuelve a mostrar completas).
                foreach (['publica' => '/^pub_(test|prod)_\w+$/', 'privada' => '/^prv_(test|prod)_\w+$/', 'eventos' => '/^\w+$/', 'integridad' => '/^\w+$/'] as $k => $re) {
                    $v = trim((string) ($d['wompi'][$k] ?? ''));
                    if ($v === '') continue;
                    if (!preg_match($re, $v)) responder(400, ['error' => "la llave «$k» no tiene el formato de Wompi"]);
                    $w[$k] = $v;
                }
                $c['wompi'] = $w;
            }
            guardar_config($c);
            bitacora(null, (int) $s['usuario']['id'], 'config', implode(',', array_keys($c)));
        }
        $c = config();
        $oculta = fn (string $v) => $v === '' ? '' : substr($v, 0, 9) . '…' . substr($v, -4);
        responder(200, [
            'prueba_dias' => (int) $c['prueba_dias'], 'gracia_dias' => (int) $c['gracia_dias'], 'anual_meses' => (int) $c['anual_meses'],
            'correo_remitente' => $c['correo_remitente'], 'planes' => $c['planes'],
            'wompi' => ['modo' => $c['wompi']['modo'], 'publica' => $oculta($c['wompi']['publica']), 'privada' => $oculta($c['wompi']['privada']),
                'eventos' => $oculta($c['wompi']['eventos']), 'integridad' => $oculta($c['wompi']['integridad']), 'lista' => wompi_lista()],
            'webhook' => 'https://' . ($_SERVER['HTTP_HOST'] ?? '') . '/api/wompi.php',
        ]);

    case 'bitacora':
        responder(200, ['eventos' => todos('SELECT b.*, e.nombre empresa FROM bitacora b LEFT JOIN empresas e ON e.id = b.empresa_id ORDER BY b.id DESC LIMIT 200')]);

    default:
        responder(404, ['error' => 'acción desconocida']);
}
