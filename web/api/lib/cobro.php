<?php
// Macula · cobro con Wompi (Colombia): pago único por Web Checkout (tarjeta,
// PSE, Nequi, Bancolombia) y renovación automática con tarjeta tokenizada.
// Los datos de la tarjeta nunca pasan por este servidor: el navegador los
// envía directo a Wompi y aquí solo llega el token.

declare(strict_types=1);

function wompi(): array {
    return config()['wompi'];
}

function wompi_lista(): bool {
    $w = wompi();
    return $w['publica'] !== '' && $w['privada'] !== '' && $w['integridad'] !== '';
}

function wompi_api(): string {
    return wompi()['modo'] === 'produccion' ? 'https://production.wompi.co/v1' : 'https://sandbox.wompi.co/v1';
}

/** Firma de integridad: SHA-256(referencia + monto en centavos + moneda + secreto). */
function firma_integridad(string $referencia, int $centavos, string $moneda): string {
    return hash('sha256', $referencia . $centavos . $moneda . wompi()['integridad']);
}

function wompi_http(string $metodo, string $ruta, ?array $cuerpo = null, bool $privada = false): array {
    $c = curl_init(wompi_api() . $ruta);
    $cab = ['Accept: application/json'];
    if ($cuerpo !== null) $cab[] = 'Content-Type: application/json';
    $cab[] = 'Authorization: Bearer ' . ($privada ? wompi()['privada'] : wompi()['publica']);
    curl_setopt_array($c, [
        CURLOPT_CUSTOMREQUEST => $metodo, CURLOPT_RETURNTRANSFER => true, CURLOPT_TIMEOUT => 25, CURLOPT_HTTPHEADER => $cab,
    ]);
    if ($cuerpo !== null) curl_setopt($c, CURLOPT_POSTFIELDS, json_encode($cuerpo));
    $r = curl_exec($c);
    $estado = (int) curl_getinfo($c, CURLINFO_RESPONSE_CODE);
    curl_close($c);
    $datos = is_string($r) ? (json_decode($r, true) ?: []) : [];
    if ($estado >= 400 || $r === false) {
        $msg = $datos['error']['reason'] ?? $datos['error']['messages'] ?? $datos['error']['type'] ?? 'la pasarela no respondió';
        throw new RuntimeException('Wompi: ' . (is_array($msg) ? json_encode($msg, JSON_UNESCAPED_UNICODE) : $msg));
    }
    return $datos['data'] ?? $datos;
}

// ───────────── Pagos ─────────────
function dias_periodo(string $periodo): int {
    return $periodo === 'anual' ? 365 : 30;
}

/** Valor que aún no se ha usado del periodo pagado (para cambiar de plan). */
function credito(array $e): int {
    $restante = (int) $e['pagado_hasta'] - ahora();
    if ($restante <= 0) return 0;
    $diario = precio($e) / dias_periodo($e['periodo']);
    return (int) floor($diario * $restante / 86400);
}

/** Monto a cobrar ahora para un plan y periodo, y si reemplaza (cambio de plan) o suma (renovación). */
function cotizar_pago(array $e, string $plan, string $periodo): array {
    $mismo = $plan === $e['plan'] && $periodo === $e['periodo'];
    $vigente = (int) $e['pagado_hasta'] > ahora();
    $total = precio($e, $plan, $periodo);
    $credito = (!$mismo && $vigente) ? min(credito($e), $total) : 0;
    return ['monto' => max(0, $total - $credito), 'total' => $total, 'credito' => $credito, 'cambio' => !$mismo && $vigente];
}

function crear_pago(array $e, string $plan, string $periodo, bool $automatico): array {
    $c = cotizar_pago($e, $plan, $periodo);
    $ref = sprintf('MAC-%d-%s-%s', $e['id'], date('ymdHis'), bin2hex(random_bytes(3)));
    ejecutar('INSERT INTO pagos (empresa_id, referencia, monto, moneda, plan, periodo, automatico, detalle, creado, actualizado) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)', [
        $e['id'], $ref, $c['monto'], config()['moneda'], $plan, $periodo, $automatico ? 1 : 0,
        json_encode(['credito' => $c['credito'], 'total' => $c['total'], 'cambio' => $c['cambio']]), ahora(), ahora(),
    ]);
    return uno('SELECT * FROM pagos WHERE referencia = ?', [$ref]);
}

/** Suma el periodo pagado a la empresa (una sola vez por pago). */
function aplicar_pago(array $p): void {
    bd()->beginTransaction();
    $p = uno('SELECT * FROM pagos WHERE id = ?', [$p['id']]);
    if (!$p || (int) $p['aplicado'] || $p['estado'] !== 'APPROVED') { bd()->commit(); return; }
    $e = empresa((int) $p['empresa_id']);
    $detalle = json_decode($p['detalle'], true) ?: [];
    $desde = !empty($detalle['cambio']) ? ahora() : max(ahora(), (int) $e['pagado_hasta']);
    $hasta = strtotime($p['periodo'] === 'anual' ? '+1 year' : '+1 month', $desde);
    ejecutar('UPDATE empresas SET pagado_hasta = ?, plan = ?, periodo = ?, intentos = 0, avisos = ? WHERE id = ?', [$hasta, $p['plan'], $p['periodo'], '{}', $e['id']]);
    ejecutar('UPDATE pagos SET aplicado = 1, actualizado = ? WHERE id = ?', [ahora(), $p['id']]);
    bd()->commit();
    bitacora((int) $e['id'], null, 'pago_aplicado', $p['referencia'] . ' hasta ' . date('Y-m-d', $hasta));
    $plan = plan($p['plan'])['nombre'] ?? $p['plan'];
    enviar_correo($e['email'], 'Pago recibido · Macula', "Recibimos tu pago de " . pesos((int) $p['monto']) . " por el plan $plan ({$p['periodo']}).\n\nTu cuenta está al día hasta el " . fecha($hasta) . ".\nReferencia: {$p['referencia']}\n\nGracias por usar Macula.\n");
}

/** Actualiza un pago con una transacción de Wompi (webhook, retorno o consulta). */
function registrar_transaccion(array $tx): ?array {
    $p = uno('SELECT * FROM pagos WHERE referencia = ?', [(string) ($tx['reference'] ?? '')]);
    if (!$p) return null;
    $estado = (string) ($tx['status'] ?? 'PENDING');
    // El monto debe coincidir con lo que se cobró: si no, no se aplica.
    if ((int) ($tx['amount_in_cents'] ?? -1) !== (int) $p['monto'] * 100) $estado = 'ERROR';
    ejecutar('UPDATE pagos SET estado = ?, metodo = ?, pasarela_id = ?, actualizado = ? WHERE id = ? AND aplicado = 0', [
        $estado, (string) ($tx['payment_method_type'] ?? ''), (string) ($tx['id'] ?? ''), ahora(), $p['id'],
    ]);
    $p = uno('SELECT * FROM pagos WHERE id = ?', [$p['id']]);
    if ($estado === 'APPROVED') aplicar_pago($p);
    elseif (in_array($estado, ['DECLINED', 'ERROR', 'VOIDED'], true) && (int) $p['automatico']) {
        $e = empresa((int) $p['empresa_id']);
        enviar_correo($e['email'], 'No pudimos cobrar tu suscripción de Macula', "El cobro automático de " . pesos((int) $p['monto']) . " fue rechazado.\n\nEntra a Macula › Mi cuenta para actualizar la tarjeta o pagar con PSE o Nequi. Si no se paga, la cuenta se bloquea al terminar los días de gracia.\n");
    }
    return uno('SELECT * FROM pagos WHERE id = ?', [$p['id']]);
}

function consultar_transaccion(string $id): ?array {
    if (!preg_match('/^[A-Za-z0-9-]{4,80}$/', $id)) return null;
    return registrar_transaccion(wompi_http('GET', '/transactions/' . $id));
}

/** Enlace del Web Checkout de Wompi para pagar un pago pendiente. */
function enlace_checkout(array $p, array $e, string $volver): string {
    $centavos = (int) $p['monto'] * 100;
    $q = [
        'public-key' => wompi()['publica'],
        'currency' => $p['moneda'],
        'amount-in-cents' => $centavos,
        'reference' => $p['referencia'],
        'signature:integrity' => firma_integridad($p['referencia'], $centavos, $p['moneda']),
        'redirect-url' => $volver,
        'customer-data:email' => $e['email'],
        'customer-data:full-name' => $e['nombre'],
        'customer-data:legal-id' => $e['documento'],
        'customer-data:legal-id-type' => $e['tipo'] === 'empresa' ? 'NIT' : 'CC',
    ];
    if ($e['telefono'] !== '') {
        $q['customer-data:phone-number'] = $e['telefono'];
        $q['customer-data:phone-number-prefix'] = '+57';
    }
    return 'https://checkout.wompi.co/p/?' . http_build_query($q);
}

/** Cobra con la tarjeta guardada (renovación automática o primer pago con tarjeta). */
function cobrar_tarjeta(array $e, string $plan, string $periodo, bool $automatico): array {
    $p = crear_pago($e, $plan, $periodo, $automatico);
    ejecutar('UPDATE empresas SET ultimo_intento = ?, intentos = intentos + ? WHERE id = ?', [ahora(), $automatico ? 1 : 0, $e['id']]);
    if ((int) $p['monto'] === 0) {
        ejecutar("UPDATE pagos SET estado = 'APPROVED', metodo = 'CREDITO' WHERE id = ?", [$p['id']]);
        aplicar_pago($p);
        return uno('SELECT * FROM pagos WHERE id = ?', [$p['id']]);
    }
    $centavos = (int) $p['monto'] * 100;
    try {
        $tx = wompi_http('POST', '/transactions', [
            'amount_in_cents' => $centavos,
            'currency' => $p['moneda'],
            'signature' => firma_integridad($p['referencia'], $centavos, $p['moneda']),
            'customer_email' => $e['email'],
            'payment_method' => ['installments' => 1],
            'reference' => $p['referencia'],
            'payment_source_id' => (int) $e['fuente_pago'],
            'recurrent' => true,
        ], true);
        return registrar_transaccion($tx) ?? $p;
    } catch (Throwable $err) {
        ejecutar("UPDATE pagos SET estado = 'ERROR', detalle = json_set(detalle, '$.error', ?), actualizado = ? WHERE id = ?", [$err->getMessage(), ahora(), $p['id']]);
        return uno('SELECT * FROM pagos WHERE id = ?', [$p['id']]);
    }
}

/**
 * Cron perezoso (máx. cada 10 min, en cualquier petición a la API): renueva con
 * tarjeta las cuentas que vencen, consulta pagos pendientes y envía avisos.
 * Se puede llamar también desde un cron del hosting (api/cron.php).
 */
function renovaciones_pendientes(bool $forzar = false): void {
    $marca = carpeta_datos() . '/ultimo-cron';
    if (!$forzar && is_file($marca) && filemtime($marca) > ahora() - 600) return;
    touch($marca);
    try {
        if (wompi_lista()) {
            // 1. Pagos pendientes de más de un minuto: se consultan.
            foreach (todos("SELECT * FROM pagos WHERE estado = 'PENDING' AND pasarela_id != '' AND creado < ? AND creado > ?", [ahora() - 60, ahora() - 3 * 86400]) as $p) {
                try { consultar_transaccion($p['pasarela_id']); } catch (Throwable) { /* se reintenta luego */ }
            }
            // 2. Renovación automática: un día antes del vencimiento, un intento por día, máximo 4.
            $vencen = todos('SELECT * FROM empresas WHERE renovar = 1 AND fuente_pago IS NOT NULL AND bloqueada = 0 AND pagado_hasta > 0 AND pagado_hasta < ? AND ultimo_intento < ? AND intentos < 4', [ahora() + 86400, ahora() - 20 * 3600]);
            foreach ($vencen as $e) {
                if (uno("SELECT 1 FROM pagos WHERE empresa_id = ? AND estado = 'PENDING' AND creado > ?", [$e['id'], ahora() - 86400])) continue;
                cobrar_tarjeta($e, $e['plan'], $e['periodo'], true);
            }
        }
        // 3. Avisos por correo antes de que se bloquee la cuenta.
        foreach (todos('SELECT * FROM empresas WHERE bloqueada = 0') as $e) {
            $st = estado_empresa($e);
            if (!$st['puede_usar'] || ($e['fuente_pago'] && (int) $e['renovar'])) continue;
            $faltan = (int) ceil(($st['acceso_hasta'] - ahora()) / 86400);
            $avisos = json_decode($e['avisos'], true) ?: [];
            foreach ([7, 3, 1] as $dias) {
                if ($faltan <= $dias && $faltan > 0 && empty($avisos["d$dias"])) {
                    $que = $st['estado'] === 'prueba' ? 'tu prueba gratis' : 'tu suscripción';
                    enviar_correo($e['email'], "Macula: $que termina en $faltan " . ($faltan === 1 ? 'día' : 'días'), "Hola,\n\n" . ucfirst($que) . " de Macula para {$e['nombre']} termina el " . fecha($st['acceso_hasta']) . ".\n\nPara seguir montando sin interrupciones, entra a Macula › Mi cuenta y paga con tarjeta, PSE o Nequi. Si guardas una tarjeta, se renueva sola.\n");
                    $avisos["d$dias"] = ahora();
                    ejecutar('UPDATE empresas SET avisos = ? WHERE id = ?', [json_encode($avisos), $e['id']]);
                    break;
                }
            }
        }
    } catch (Throwable $err) {
        bitacora(null, null, 'cron_error', $err->getMessage());
    }
}
