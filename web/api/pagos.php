<?php
// Macula · pagos de la suscripción.
//
//   GET  api/pagos.php?accion=historial | comercio | confirmar&id=<transacción>
//   POST api/pagos.php?accion=checkout | tarjeta | quitar_tarjeta | renovar | cotizar

declare(strict_types=1);
require __DIR__ . '/lib/base.php';
require __DIR__ . '/lib/cobro.php';

$accion = $_GET['accion'] ?? '';
$metodo = $_SERVER['REQUEST_METHOD'] ?? 'GET';
if ($metodo === 'POST' && ($_SERVER['HTTP_X_MACULA'] ?? '') !== '1') responder(400, ['error' => 'petición inválida']);
$s = exigir_sesion();
$e = $s['empresa'];
$d = $metodo === 'POST' ? cuerpo() : [];

// Pagar lo hace el dueño o un administrador de la empresa, aunque la cuenta esté vencida.
if (in_array($accion, ['checkout', 'tarjeta', 'tarjeta_existente', 'quitar_tarjeta', 'renovar'], true)) exigir_rol($s, ['dueno', 'admin']);

function plan_y_periodo(array $d, array $e, array $s): array {
    $plan = (string) ($d['plan'] ?? $e['plan']);
    $periodo = ($d['periodo'] ?? $e['periodo']) === 'anual' ? 'anual' : 'mensual';
    if (!plan($plan)) responder(400, ['error' => 'plan desconocido']);
    $activos = (int) uno('SELECT COUNT(*) n FROM usuarios WHERE empresa_id = ? AND activo = 1', [$e['id']])['n'];
    if ($activos > (int) plan($plan)['usuarios']) responder(400, ['error' => "el plan {$plan} permite " . plan($plan)['usuarios'] . " usuarios y tienes $activos activos; desactiva algunos primero"]);
    return [$plan, $periodo];
}

function sin_pasarela(): never {
    responder(503, ['error' => 'los pagos en línea aún no están configurados; escríbenos para activar tu cuenta']);
}

switch ($accion) {
    case 'cotizar':
        [$plan, $periodo] = plan_y_periodo($d, $e, $s);
        responder(200, cotizar_pago($e, $plan, $periodo));

    case 'checkout': // pago único: tarjeta, PSE, Nequi o Bancolombia en la página de Wompi
        if (!wompi_lista()) sin_pasarela();
        [$plan, $periodo] = plan_y_periodo($d, $e, $s);
        $p = crear_pago($e, $plan, $periodo, false);
        if ((int) $p['monto'] === 0) {
            ejecutar("UPDATE pagos SET estado = 'APPROVED', metodo = 'CREDITO' WHERE id = ?", [$p['id']]);
            aplicar_pago($p);
            responder(200, ['ok' => true, 'aplicado' => true]);
        }
        $volver = 'https://' . ($_SERVER['HTTP_HOST'] ?? '') . '/acceso.html?pago=' . rawurlencode($p['referencia']);
        bitacora((int) $e['id'], (int) $s['usuario']['id'], 'checkout', $p['referencia']);
        responder(200, ['url' => enlace_checkout($p, $e, $volver), 'referencia' => $p['referencia'], 'monto' => (int) $p['monto']]);

    case 'confirmar': // al volver de Wompi con ?id=<transacción>
        if (!wompi_lista()) sin_pasarela();
        try {
            $p = consultar_transaccion((string) ($_GET['id'] ?? ''));
        } catch (Throwable $err) {
            responder(502, ['error' => $err->getMessage()]);
        }
        if (!$p || (int) $p['empresa_id'] !== (int) $e['id']) responder(404, ['error' => 'no encontramos ese pago']);
        responder(200, ['estado' => $p['estado'], 'aplicado' => (bool) $p['aplicado'], 'monto' => (int) $p['monto']]);

    case 'comercio': // lo que el navegador necesita para tokenizar la tarjeta en Wompi
        if (!wompi_lista()) sin_pasarela();
        try {
            $m = wompi_http('GET', '/merchants/' . wompi()['publica']);
        } catch (Throwable $err) {
            responder(502, ['error' => $err->getMessage()]);
        }
        responder(200, [
            'publica' => wompi()['publica'], 'api' => wompi_api(),
            'aceptacion' => $m['presigned_acceptance']['acceptance_token'] ?? '',
            'terminos' => $m['presigned_acceptance']['permalink'] ?? '',
            'datos_personales' => $m['presigned_personal_data_auth']['acceptance_token'] ?? '',
            'politica_datos' => $m['presigned_personal_data_auth']['permalink'] ?? '',
        ]);

    case 'tarjeta': // guarda la tarjeta (token de Wompi) para cobro automático y cobra si hace falta
        if (!wompi_lista()) sin_pasarela();
        [$plan, $periodo] = plan_y_periodo($d, $e, $s);
        $token = (string) ($d['token'] ?? '');
        if (!preg_match('/^tok_[A-Za-z0-9_-]+$/', $token)) responder(400, ['error' => 'token de tarjeta inválido']);
        try {
            $cuerpo = ['type' => 'CARD', 'token' => $token, 'customer_email' => $e['email'], 'acceptance_token' => (string) ($d['aceptacion'] ?? '')];
            if (!empty($d['datos_personales'])) $cuerpo['accept_personal_auth'] = (string) $d['datos_personales'];
            $fuente = wompi_http('POST', '/payment_sources', $cuerpo, true);
        } catch (Throwable $err) {
            responder(502, ['error' => $err->getMessage()]);
        }
        $desc = trim(preg_replace('/[^A-Za-z0-9 •]/u', '', (string) ($d['marca'] ?? 'Tarjeta')) . ' •••• ' . preg_replace('/\D/', '', (string) ($d['ultimos'] ?? '')));
        ejecutar('UPDATE empresas SET fuente_pago = ?, fuente_desc = ?, renovar = 1, intentos = 0 WHERE id = ?', [(string) $fuente['id'], $desc, $e['id']]);
        bitacora((int) $e['id'], (int) $s['usuario']['id'], 'tarjeta', $desc);
        $e = empresa((int) $e['id']);
        $cambio = $plan !== $e['plan'] || $periodo !== $e['periodo'];
        // Se cobra ya si la cuenta no está pagada o si cambia de plan; si no, en la próxima renovación.
        if ($cambio || (int) $e['pagado_hasta'] <= ahora()) {
            $p = cobrar_tarjeta($e, $plan, $periodo, false);
            responder(200, ['ok' => true, 'estado' => $p['estado'], 'aplicado' => (bool) $p['aplicado'], 'referencia' => $p['referencia']]);
        }
        responder(200, ['ok' => true, 'estado' => 'GUARDADA']);

    case 'tarjeta_existente': // cobra a la tarjeta guardada (cambio de plan o pago adelantado)
        if (!wompi_lista()) sin_pasarela();
        if (!$e['fuente_pago']) responder(400, ['error' => 'no hay tarjeta guardada']);
        [$plan, $periodo] = plan_y_periodo($d, $e, $s);
        $p = cobrar_tarjeta($e, $plan, $periodo, false);
        responder(200, ['ok' => true, 'estado' => $p['estado'], 'aplicado' => (bool) $p['aplicado']]);

    case 'quitar_tarjeta':
        ejecutar('UPDATE empresas SET fuente_pago = NULL, fuente_desc = NULL WHERE id = ?', [$e['id']]);
        bitacora((int) $e['id'], (int) $s['usuario']['id'], 'quitar_tarjeta');
        responder(200, ['ok' => true]);

    case 'renovar':
        ejecutar('UPDATE empresas SET renovar = ? WHERE id = ?', [!empty($d['renovar']) ? 1 : 0, $e['id']]);
        responder(200, ['ok' => true]);

    case 'historial':
        $lista = todos('SELECT referencia, monto, moneda, plan, periodo, estado, metodo, automatico, creado FROM pagos WHERE empresa_id = ? ORDER BY id DESC LIMIT 60', [$e['id']]);
        // Un pago pendiente reciente se consulta en la pasarela al pedir el historial.
        if (wompi_lista()) {
            foreach (todos("SELECT pasarela_id FROM pagos WHERE empresa_id = ? AND estado = 'PENDING' AND pasarela_id != '' AND creado > ?", [$e['id'], ahora() - 86400]) as $p) {
                try { consultar_transaccion($p['pasarela_id']); } catch (Throwable) { /* luego */ }
            }
        }
        responder(200, ['pagos' => $lista]);

    default:
        responder(404, ['error' => 'acción desconocida']);
}
