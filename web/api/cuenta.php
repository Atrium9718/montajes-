<?php
// Macula · cuentas: registro de empresas, verificación por correo, ingreso,
// recuperación de contraseña y usuarios de cada empresa.
//
//   POST api/cuenta.php?accion=registrar | verificar | reenviar | entrar | salir
//                              | recuperar | restablecer | clave | invitar | usuario
//   GET  api/cuenta.php?accion=yo | usuarios

declare(strict_types=1);
require __DIR__ . '/lib/base.php';
require __DIR__ . '/lib/cobro.php';

$accion = $_GET['accion'] ?? '';
$metodo = $_SERVER['REQUEST_METHOD'] ?? 'GET';
if ($metodo === 'POST' && ($_SERVER['HTTP_X_MACULA'] ?? '') !== '1') {
    // Solo la app (fetch con esta cabecera) puede hacer cambios: evita envíos desde otros sitios.
    responder(400, ['error' => 'petición inválida']);
}
renovaciones_pendientes(); // cobros automáticos atrasados (cron perezoso)
$d = $metodo === 'POST' ? cuerpo() : [];

switch ($accion) {
    case 'yo':
        $s = sesion();
        if (!$s) responder(200, ['sesion' => false, 'planes' => planes_publicos()]);
        responder(200, ['sesion' => true] + resumen_sesion($s) + ['planes' => planes_publicos()]);

    case 'registrar':
        limitar('registro:' . ip(), 5, 3600);
        $email = normalizar_email((string) ($d['email'] ?? ''));
        $nombre = trim((string) ($d['nombre'] ?? ''));
        $empresa = trim((string) ($d['empresa'] ?? '')) ?: $nombre;
        $tipo = ($d['tipo'] ?? '') === 'independiente' ? 'independiente' : 'empresa';
        $documento = normalizar_documento((string) ($d['documento'] ?? ''));
        $telefono = normalizar_telefono((string) ($d['telefono'] ?? ''));
        $clave = (string) ($d['clave'] ?? '');
        $plan = plan((string) ($d['plan'] ?? 'taller')) ? (string) $d['plan'] : 'taller';
        $periodo = ($d['periodo'] ?? '') === 'anual' ? 'anual' : 'mensual';
        if (!filter_var($email, FILTER_VALIDATE_EMAIL)) responder(400, ['error' => 'escribe un correo válido']);
        if (mb_strlen($nombre) < 3) responder(400, ['error' => 'escribe tu nombre']);
        if (strlen($documento) < 5) responder(400, ['error' => $tipo === 'empresa' ? 'escribe el NIT de la empresa' : 'escribe tu número de cédula']);
        if (strlen($telefono) < 7) responder(400, ['error' => 'escribe un teléfono de contacto']);
        if (strlen($clave) < 8) responder(400, ['error' => 'la contraseña debe tener al menos 8 caracteres']);
        $existente = uno('SELECT * FROM usuarios WHERE email = ?', [$email]);
        if ($existente && (int) $existente['verificado']) responder(409, ['error' => 'ya hay una cuenta con ese correo; inicia sesión o recupera la contraseña']);
        bd()->beginTransaction();
        if ($existente) { // registro sin verificar: se reemplaza
            ejecutar('DELETE FROM usuarios WHERE id = ?', [$existente['id']]);
            if (!uno('SELECT 1 FROM usuarios WHERE empresa_id = ?', [$existente['empresa_id']])) ejecutar('DELETE FROM empresas WHERE id = ? AND pagado_hasta = 0', [$existente['empresa_id']]);
        }
        ejecutar('INSERT INTO empresas (nombre, documento, tipo, email, telefono, plan, periodo, creada) VALUES (?, ?, ?, ?, ?, ?, ?, ?)', [
            mb_substr($empresa, 0, 120), $documento, $tipo, $email, $telefono, $plan, $periodo, ahora(),
        ]);
        $empresa_id = (int) bd()->lastInsertId();
        ejecutar('INSERT INTO usuarios (empresa_id, email, nombre, clave, rol, creado) VALUES (?, ?, ?, ?, ?, ?)', [
            $empresa_id, $email, mb_substr($nombre, 0, 120), password_hash($clave, PASSWORD_DEFAULT), 'dueno', ahora(),
        ]);
        bd()->commit();
        $codigo = crear_codigo($email, 'verificar');
        enviar_correo($email, 'Tu código de Macula: ' . $codigo, "Hola $nombre,\n\nTu código para activar la cuenta de $empresa en Macula es:\n\n    $codigo\n\nVence en 30 minutos. Si no fuiste tú, ignora este correo.\n");
        bitacora($empresa_id, null, 'registro', "$tipo $documento");
        responder(200, ['ok' => true, 'verificar' => $email, 'prueba' => prueba_disponible($documento, $email, $telefono)]);

    case 'reenviar':
        $email = normalizar_email((string) ($d['email'] ?? ''));
        limitar('reenviar:' . $email, 3, 900);
        $u = uno('SELECT * FROM usuarios WHERE email = ? AND verificado = 0', [$email]);
        if ($u) {
            $codigo = crear_codigo($email, 'verificar');
            enviar_correo($email, 'Tu código de Macula: ' . $codigo, "Tu código para activar la cuenta en Macula es:\n\n    $codigo\n\nVence en 30 minutos.\n");
        }
        responder(200, ['ok' => true]);

    case 'verificar':
        $email = normalizar_email((string) ($d['email'] ?? ''));
        limitar('verificar:' . ip(), 20, 3600);
        $u = uno('SELECT * FROM usuarios WHERE email = ? AND verificado = 0', [$email]);
        if (!$u || !validar_codigo($email, 'verificar', (string) ($d['codigo'] ?? ''))) responder(400, ['error' => 'el código no es válido o ya venció']);
        ejecutar('UPDATE usuarios SET verificado = 1 WHERE id = ?', [$u['id']]);
        $e = empresa((int) $u['empresa_id']);
        $prueba = false;
        if ($u['rol'] === 'dueno' && (int) $e['prueba_hasta'] === 0 && (int) $e['pagado_hasta'] === 0
            && prueba_disponible($e['documento'], $e['email'], $e['telefono'])) {
            // Una sola prueba por empresa: se marcan NIT/cédula, correo y teléfono.
            ejecutar('UPDATE empresas SET prueba_hasta = ? WHERE id = ?', [ahora() + (int) config()['prueba_dias'] * 86400, $e['id']]);
            foreach (huellas($e['documento'], $e['email'], $e['telefono']) as $h) {
                ejecutar('INSERT OR IGNORE INTO pruebas_usadas (huella, empresa_id, momento) VALUES (?, ?, ?)', [$h, $e['id'], ahora()]);
            }
            $prueba = true;
        }
        bitacora((int) $e['id'], (int) $u['id'], 'verificado', $prueba ? 'con prueba' : 'sin prueba');
        crear_sesion((int) $u['id']);
        responder(200, ['ok' => true, 'prueba' => $prueba]);

    case 'entrar':
        $email = normalizar_email((string) ($d['email'] ?? ''));
        limitar('entrar:' . ip(), 20, 900);
        limitar('entrar:' . $email, 8, 900);
        $u = uno('SELECT * FROM usuarios WHERE email = ?', [$email]);
        if (!$u || !password_verify((string) ($d['clave'] ?? ''), $u['clave'])) responder(401, ['error' => 'correo o contraseña incorrectos']);
        if (!(int) $u['activo']) responder(403, ['error' => 'este usuario fue desactivado por su empresa']);
        if (!(int) $u['verificado']) {
            $codigo = crear_codigo($email, 'verificar');
            enviar_correo($email, 'Tu código de Macula: ' . $codigo, "Tu código para activar la cuenta en Macula es:\n\n    $codigo\n");
            responder(200, ['ok' => false, 'verificar' => $email]);
        }
        if (password_needs_rehash($u['clave'], PASSWORD_DEFAULT)) ejecutar('UPDATE usuarios SET clave = ? WHERE id = ?', [password_hash((string) $d['clave'], PASSWORD_DEFAULT), $u['id']]);
        crear_sesion((int) $u['id']);
        bitacora((int) $u['empresa_id'], (int) $u['id'], 'ingreso', ip());
        responder(200, ['ok' => true]);

    case 'salir':
        cerrar_sesion();
        responder(200, ['ok' => true]);

    case 'recuperar':
        $email = normalizar_email((string) ($d['email'] ?? ''));
        limitar('recuperar:' . ip(), 5, 3600);
        if (uno('SELECT 1 FROM usuarios WHERE email = ? AND activo = 1', [$email])) {
            $codigo = crear_codigo($email, 'recuperar');
            enviar_correo($email, 'Restablece tu contraseña de Macula', "Tu código para crear una contraseña nueva es:\n\n    $codigo\n\nVence en 30 minutos. Si no lo pediste, ignora este correo.\n");
        }
        responder(200, ['ok' => true]); // misma respuesta exista o no (no revela cuentas)

    case 'restablecer':
        $email = normalizar_email((string) ($d['email'] ?? ''));
        limitar('restablecer:' . ip(), 10, 3600);
        $clave = (string) ($d['clave'] ?? '');
        if (strlen($clave) < 8) responder(400, ['error' => 'la contraseña debe tener al menos 8 caracteres']);
        $u = uno('SELECT * FROM usuarios WHERE email = ? AND activo = 1', [$email]);
        if (!$u || !validar_codigo($email, 'recuperar', (string) ($d['codigo'] ?? ''))) responder(400, ['error' => 'el código no es válido o ya venció']);
        ejecutar('UPDATE usuarios SET clave = ?, verificado = 1 WHERE id = ?', [password_hash($clave, PASSWORD_DEFAULT), $u['id']]);
        ejecutar('DELETE FROM sesiones WHERE usuario_id = ?', [$u['id']]);
        crear_sesion((int) $u['id']);
        responder(200, ['ok' => true]);

    case 'clave':
        $s = exigir_sesion();
        if (!password_verify((string) ($d['actual'] ?? ''), $s['usuario']['clave'])) responder(400, ['error' => 'la contraseña actual no es correcta']);
        if (strlen((string) ($d['nueva'] ?? '')) < 8) responder(400, ['error' => 'la contraseña debe tener al menos 8 caracteres']);
        ejecutar('UPDATE usuarios SET clave = ? WHERE id = ?', [password_hash((string) $d['nueva'], PASSWORD_DEFAULT), $s['usuario']['id']]);
        responder(200, ['ok' => true]);

    case 'usuarios':
        $s = exigir_sesion();
        responder(200, ['usuarios' => todos('SELECT id, email, nombre, rol, activo, verificado, ultimo_ingreso FROM usuarios WHERE empresa_id = ? ORDER BY id', [$s['empresa']['id']])]);

    case 'invitar':
        $s = exigir_sesion();
        exigir_rol($s, ['dueno', 'admin']);
        if (!$s['suscripcion']['puede_usar']) responder(402, ['error' => 'la suscripción no está al día']);
        $email = normalizar_email((string) ($d['email'] ?? ''));
        $nombre = trim((string) ($d['nombre'] ?? ''));
        $rol = in_array($d['rol'] ?? '', ['admin', 'operador'], true) ? $d['rol'] : 'operador';
        if (!filter_var($email, FILTER_VALIDATE_EMAIL) || $nombre === '') responder(400, ['error' => 'escribe nombre y correo']);
        if (uno('SELECT 1 FROM usuarios WHERE email = ?', [$email])) responder(409, ['error' => 'ese correo ya tiene una cuenta']);
        $max = (int) (plan($s['empresa']['plan'])['usuarios'] ?? 1);
        $activos = (int) uno('SELECT COUNT(*) n FROM usuarios WHERE empresa_id = ? AND activo = 1', [$s['empresa']['id']])['n'];
        if ($activos >= $max) responder(402, ['error' => "tu plan permite $max " . ($max === 1 ? 'usuario' : 'usuarios') . '; cambia de plan para agregar más']);
        $temporal = substr(strtr(base64_encode(random_bytes(9)), '+/', 'xy'), 0, 12);
        ejecutar('INSERT INTO usuarios (empresa_id, email, nombre, clave, rol, verificado, creado) VALUES (?, ?, ?, ?, ?, 1, ?)', [
            $s['empresa']['id'], $email, mb_substr($nombre, 0, 120), password_hash($temporal, PASSWORD_DEFAULT), $rol, ahora(),
        ]);
        $sitio = 'https://' . ($_SERVER['HTTP_HOST'] ?? '');
        enviar_correo($email, "Te invitaron a Macula ({$s['empresa']['nombre']})", "Hola $nombre,\n\n{$s['usuario']['nombre']} te dio acceso a Macula para {$s['empresa']['nombre']}.\n\nEntra en $sitio con:\n  Correo: $email\n  Contraseña temporal: $temporal\n\nCámbiala en «Mi cuenta» después de entrar.\n");
        bitacora((int) $s['empresa']['id'], (int) $s['usuario']['id'], 'invitar', $email);
        responder(200, ['ok' => true, 'temporal' => $temporal]);

    case 'usuario': // activar/desactivar o cambiar rol de un usuario de la empresa
        $s = exigir_sesion();
        exigir_rol($s, ['dueno', 'admin']);
        $u = uno('SELECT * FROM usuarios WHERE id = ? AND empresa_id = ?', [(int) ($d['id'] ?? 0), $s['empresa']['id']]);
        if (!$u || $u['rol'] === 'dueno' || $u['rol'] === 'super') responder(400, ['error' => 'no se puede cambiar este usuario']);
        if (isset($d['activo'])) {
            if ($d['activo']) {
                $max = (int) (plan($s['empresa']['plan'])['usuarios'] ?? 1);
                $activos = (int) uno('SELECT COUNT(*) n FROM usuarios WHERE empresa_id = ? AND activo = 1', [$s['empresa']['id']])['n'];
                if ($activos >= $max) responder(402, ['error' => 'tu plan no permite más usuarios activos']);
            }
            ejecutar('UPDATE usuarios SET activo = ? WHERE id = ?', [$d['activo'] ? 1 : 0, $u['id']]);
            if (!$d['activo']) ejecutar('DELETE FROM sesiones WHERE usuario_id = ?', [$u['id']]);
        }
        if (isset($d['rol']) && in_array($d['rol'], ['admin', 'operador'], true)) ejecutar('UPDATE usuarios SET rol = ? WHERE id = ?', [$d['rol'], $u['id']]);
        responder(200, ['ok' => true]);

    default:
        responder(404, ['error' => 'acción desconocida']);
}

function huellas(string $documento, string $email, string $telefono): array {
    $h = ['doc:' . hash('sha256', $documento), 'email:' . hash('sha256', $email)];
    if ($telefono !== '') $h[] = 'tel:' . hash('sha256', $telefono);
    return $h;
}

function prueba_disponible(string $documento, string $email, string $telefono): bool {
    foreach (huellas($documento, $email, $telefono) as $h) {
        if (uno('SELECT 1 FROM pruebas_usadas WHERE huella = ?', [$h])) return false;
    }
    return (int) config()['prueba_dias'] > 0;
}

function planes_publicos(): array {
    $c = config();
    $lista = [];
    foreach ($c['planes'] as $id => $p) {
        $lista[] = ['id' => $id, 'nombre' => $p['nombre'], 'usuarios' => (int) $p['usuarios'], 'mensual' => (int) $p['mensual'],
            'anual' => (int) $p['mensual'] * (int) $c['anual_meses'], 'descripcion' => $p['descripcion'] ?? ''];
    }
    return ['lista' => $lista, 'prueba_dias' => (int) $c['prueba_dias'], 'moneda' => $c['moneda'], 'anual_meses' => (int) $c['anual_meses']];
}
