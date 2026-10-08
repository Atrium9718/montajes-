<?php
// Macula · tareas periódicas (renovaciones, pagos pendientes, avisos). Ya
// corren solas con el uso de la app; para más precisión programa en el
// hosting un cron diario: curl -s https://<sitio>/api/cron.php
declare(strict_types=1);
require __DIR__ . '/lib/base.php';
require __DIR__ . '/lib/cobro.php';
limitar('cron', 3, 60);
renovaciones_pendientes(true);
responder(200, ['ok' => true]);
