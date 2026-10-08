// Macula · Mi cuenta (plan, pagos, tarjeta, usuarios) y panel del
// administrador del servicio. El cobro lo hace Wompi: la tarjeta va directo
// del navegador a Wompi y al servidor solo llega el token.

const pesos = (v) => "$" + Math.round(v || 0).toLocaleString("es-CO");
const fecha = (t) => (t ? new Date(t * 1000).toLocaleDateString("es-CO", { day: "numeric", month: "short", year: "numeric" }) : "—");
const ESTADOS = { activa: ["Al día", "ok"], prueba: ["Prueba gratis", "aviso"], gracia: ["Pago pendiente", "error"], vencida: ["Vencida", "error"], bloqueada: ["Suspendida", "error"] };
const ROLES = { dueno: "Dueño", admin: "Administrador", operador: "Operador", super: "Administrador de Macula" };
const ESTADO_PAGO = { APPROVED: "Aprobado", PENDING: "En proceso", DECLINED: "Rechazado", ERROR: "Error", VOIDED: "Anulado" };

export async function api(ruta, datos) {
  const r = await fetch(`api/${ruta}`, datos === undefined
    ? { cache: "no-store" }
    : { method: "POST", headers: { "Content-Type": "application/json", "X-Macula": "1" }, body: JSON.stringify(datos) });
  const j = await r.json().catch(() => ({}));
  if (!r.ok) throw new Error(j.error || `error ${r.status}`);
  return j;
}

const local = { plan: null, periodo: null, pagos: null, usuarios: null, admin: null, config: null };

export function vistaCuenta(main, h) {
  const { esc, chips } = h;
  const c = h.estado.cuenta;
  if (!c) { main.innerHTML = `<div class="error-caja">No hay sesión.</div>`; return; }
  const e = c.empresa, s = c.suscripcion, u = c.usuario;
  const puedeAdministrar = ["dueno", "admin", "super"].includes(u.rol);
  local.plan ??= e.plan;
  local.periodo ??= e.periodo;
  const [nombreEstado, tono] = ESTADOS[s.estado] || [s.estado, "aviso"];
  const hasta = s.estado === "prueba" ? e.prueba_hasta : e.pagado_hasta;
  const planes = c.planes;
  const cambio = local.plan !== e.plan || local.periodo !== e.periodo;
  main.innerHTML = `
    <div class="encabezado"><div><h1>Mi <span class="serif">cuenta</span></h1><p>${esc(e.nombre)} · ${esc(u.nombre)} (${ROLES[u.rol] || u.rol})</p></div>
      <div class="acciones-resultado">${u.rol === "super" ? `<a class="boton boton-claro boton-chico" href="#admin">Panel de Macula</a>` : ""}<button class="boton boton-claro boton-chico" id="cu-salir" type="button">Cerrar sesión</button></div></div>
    <div class="trabajo">
      <div class="panel">
        <section class="tarjeta-oscura">
          <span class="sello sello-${tono}">${nombreEstado}</span>
          <div class="metricas metricas-cuenta" style="margin-top:18px">
            <div class="metrica"><b>${esc(e.plan_nombre)}</b><span>${e.periodo === "anual" ? "anual" : "mensual"} · ${pesos(e.precio)}${e.descuento ? ` (−${e.descuento} %)` : ""}</span></div>
            <div class="metrica"><b>${e.usuarios}/${e.usuarios_max}</b><span>usuarios</span></div>
            <div class="metrica"><b>${fecha(s.estado === "gracia" ? e.pagado_hasta : hasta)}</b><span>${s.estado === "prueba" ? "fin de la prueba" : s.estado === "gracia" ? `venció · se bloquea en ${s.gracia_dias} días` : "pagado hasta"}</span></div>
          </div>
          <p class="explicacion" style="margin-top:18px">${e.tarjeta ? `Renovación ${e.renovar ? "automática" : "<b>apagada</b>"} con ${esc(e.tarjeta)}.` : "Sin tarjeta guardada: cada periodo se paga a mano con PSE, Nequi o tarjeta."}</p>
        </section>

        ${puedeAdministrar ? `
        <section class="tarjeta paso">
          <div class="paso-titulo"><span class="paso-num">1</span><h3>Plan</h3></div>
          ${chips("periodo", [["mensual", "Mensual"], ["anual", `Anual (paga ${planes.anual_meses} meses)`]], local.periodo)}
          <div class="planes">${planes.lista.map((p) => `
            <button type="button" class="plan ${p.id === local.plan ? "elegido" : ""}" data-plan="${esc(p.id)}" aria-pressed="${p.id === local.plan}">
              <b>${esc(p.nombre)}${p.id === e.plan ? ` <small class="tenue">· actual</small>` : ""}</b>
              <span class="plan-precio">${pesos(local.periodo === "anual" ? p.anual : p.mensual)}<small>/${local.periodo === "anual" ? "año" : "mes"}</small></span>
              <span class="plan-usuarios">${p.usuarios === 1 ? "1 usuario" : `Hasta ${p.usuarios} usuarios`}</span>
              <span class="tenue">${esc(p.descripcion)}</span>
            </button>`).join("")}</div>
          <div id="cu-cotizacion" class="tenue"></div>
          <div class="acciones-resultado">
            <button class="boton boton-naranja" id="cu-checkout" type="button">${cambio ? "Cambiar y pagar" : "Pagar el siguiente periodo"} con PSE, Nequi o tarjeta</button>
            ${e.tarjeta && cambio ? `<button class="boton boton-claro" id="cu-cobrar-tarjeta" type="button">Cambiar y cobrar a ${esc(e.tarjeta)}</button>` : ""}
          </div>
          <small class="tenue">Al cambiar de plan, lo que te queda del periodo pagado se descuenta del nuevo.</small>
        </section>

        <section class="tarjeta paso">
          <div class="paso-titulo"><span class="paso-num">2</span><h3>Cobro automático con tarjeta</h3></div>
          ${e.tarjeta ? `<div class="archivo"><span class="orbe" aria-hidden="true"></span><div><b>${esc(e.tarjeta)}</b><span>Se cobra un día antes de cada vencimiento</span></div>
              <button class="boton boton-claro boton-chico" id="cu-quitar-tarjeta" type="button">Quitar</button></div>
            <label class="interruptor"><span>Renovar automáticamente</span><input type="checkbox" id="cu-renovar" ${e.renovar ? "checked" : ""}></label>` : ""}
          <details class="avanzado" ${e.tarjeta ? "" : "open"}><summary>${e.tarjeta ? "Cambiar tarjeta" : "Guardar una tarjeta"}</summary>
            <form id="cu-tarjeta" class="paso" autocomplete="on">
              <label class="campo"><span>Número de la tarjeta</span><input type="text" name="number" inputmode="numeric" autocomplete="cc-number" required placeholder="0000 0000 0000 0000"></label>
              <div class="fila-3">
                <label class="campo"><span>Mes</span><input type="text" name="exp_month" inputmode="numeric" maxlength="2" autocomplete="cc-exp-month" required placeholder="MM"></label>
                <label class="campo"><span>Año</span><input type="text" name="exp_year" inputmode="numeric" maxlength="2" autocomplete="cc-exp-year" required placeholder="AA"></label>
                <label class="campo"><span>CVC</span><input type="password" name="cvc" inputmode="numeric" maxlength="4" autocomplete="cc-csc" required></label>
              </div>
              <label class="campo"><span>Nombre en la tarjeta</span><input type="text" name="card_holder" autocomplete="cc-name" required></label>
              <label class="interruptor"><span id="cu-terminos">Acepto los términos de Wompi y autorizo el cobro recurrente.</span><input type="checkbox" name="acepto" required></label>
              <button class="boton" type="submit">Guardar tarjeta${s.estado !== "activa" || cambio ? " y pagar" : ""}</button>
              <small class="tenue">Los datos van cifrados directo a Wompi (Bancolombia); Macula nunca los ve ni los guarda.</small>
            </form>
          </details>
        </section>` : ""}

        <section class="tarjeta paso">
          <div class="paso-titulo"><span class="paso-num">${puedeAdministrar ? 3 : 1}</span><h3>Seguridad</h3></div>
          <form id="cu-clave" class="paso">
            <div class="fila-2">
              <label class="campo"><span>Contraseña actual</span><input type="password" name="actual" required autocomplete="current-password"></label>
              <label class="campo"><span>Nueva (8 o más)</span><input type="password" name="nueva" minlength="8" required autocomplete="new-password"></label>
            </div>
            <button class="boton boton-claro boton-chico" type="submit">Cambiar contraseña</button>
          </form>
        </section>
      </div>
      <div class="columna-resultado">
        <section class="tarjeta paso">
          <div class="paso-titulo"><h3>Usuarios de ${esc(e.nombre)}</h3></div>
          <div id="cu-usuarios" class="tenue">Cargando…</div>
          ${puedeAdministrar ? `<details class="avanzado"><summary>Invitar a un usuario</summary>
            <form id="cu-invitar" class="paso">
              <div class="fila-2">
                <label class="campo"><span>Nombre</span><input type="text" name="nombre" required></label>
                <label class="campo"><span>Correo</span><input type="email" name="email" required></label>
              </div>
              ${chips("rol", [["operador", "Operador"], ["admin", "Administrador"]], "operador")}
              <button class="boton boton-chico" type="submit">Enviar invitación</button>
              <small class="tenue">Le llega un correo con una contraseña temporal. Los administradores pueden pagar e invitar.</small>
            </form></details>` : ""}
        </section>
        <section class="tarjeta paso">
          <div class="paso-titulo"><h3>Pagos</h3></div>
          <div id="cu-pagos" class="tenue">Cargando…</div>
        </section>
      </div>
    </div>`;

  const redibujar = () => vistaCuenta(main, h);
  const refrescarCuenta = async () => { await h.refrescarCuenta(); redibujar(); };
  const ocupar = async (boton, fn) => {
    const texto = boton?.textContent;
    if (boton) { boton.disabled = true; boton.textContent = "Un momento…"; }
    try { await fn(); } catch (err) { h.avisar(err.message); }
    if (boton && boton.isConnected) { boton.disabled = false; boton.textContent = texto; }
  };

  $(main, "#cu-salir")?.addEventListener("click", async () => { await api("cuenta.php?accion=salir", {}); location.href = "/acceso.html"; });
  main.querySelectorAll("[data-chips=periodo]").forEach((g) => g.addEventListener("click", (ev) => { const b = ev.target.closest(".chip"); if (b) { local.periodo = b.dataset.valor; redibujar(); } }));
  main.querySelectorAll("[data-plan]").forEach((b) => b.addEventListener("click", () => { local.plan = b.dataset.plan; redibujar(); }));
  let rol = "operador";
  main.querySelectorAll("[data-chips=rol]").forEach((g) => g.addEventListener("click", (ev) => {
    const b = ev.target.closest(".chip");
    if (!b) return;
    rol = b.dataset.valor;
    g.querySelectorAll(".chip").forEach((c) => c.setAttribute("aria-pressed", String(c === b)));
  }));

  if (puedeAdministrar) {
    api("pagos.php?accion=cotizar", { plan: local.plan, periodo: local.periodo }).then((q) => {
      const caja = $(main, "#cu-cotizacion");
      if (caja) caja.innerHTML = `A pagar ahora: <b>${pesos(q.monto)}</b>${q.credito ? ` (${pesos(q.total)} menos ${pesos(q.credito)} que te quedan del plan actual)` : ""}.`;
    }).catch((err) => { const caja = $(main, "#cu-cotizacion"); if (caja) caja.textContent = err.message; });
  }
  $(main, "#cu-checkout")?.addEventListener("click", (ev) => ocupar(ev.currentTarget, async () => {
    const r = await api("pagos.php?accion=checkout", { plan: local.plan, periodo: local.periodo });
    if (r.aplicado) { h.avisar("Plan cambiado"); return refrescarCuenta(); }
    location.href = r.url;
  }));
  $(main, "#cu-cobrar-tarjeta")?.addEventListener("click", (ev) => ocupar(ev.currentTarget, async () => {
    const r = await api("pagos.php?accion=tarjeta_existente", { plan: local.plan, periodo: local.periodo });
    h.avisar(r.aplicado ? "Pago aprobado" : "Pago en proceso");
    await refrescarCuenta();
  }));
  $(main, "#cu-quitar-tarjeta")?.addEventListener("click", (ev) => ocupar(ev.currentTarget, async () => {
    if (!confirm("¿Quitar la tarjeta? La suscripción ya no se renovará sola.")) return;
    await api("pagos.php?accion=quitar_tarjeta", {});
    await refrescarCuenta();
  }));
  $(main, "#cu-renovar")?.addEventListener("change", (ev) => ocupar(null, async () => { await api("pagos.php?accion=renovar", { renovar: ev.target.checked }); await refrescarCuenta(); }));

  // Tarjeta: se tokeniza en Wompi desde el navegador.
  const formTarjeta = $(main, "#cu-tarjeta");
  if (formTarjeta) {
    let comercio = null;
    formTarjeta.addEventListener("focusin", async () => {
      if (comercio) return;
      comercio = api("pagos.php?accion=comercio").then((m) => {
        const t = $(main, "#cu-terminos");
        if (t && m.terminos) t.innerHTML = `Acepto los <a href="${esc(m.terminos)}" target="_blank" rel="noopener">términos de Wompi</a>${m.politica_datos ? ` y la <a href="${esc(m.politica_datos)}" target="_blank" rel="noopener">autorización de datos</a>` : ""}, y autorizo el cobro recurrente.`;
        return m;
      }).catch((err) => { comercio = null; h.avisar(err.message); });
    }, { once: false });
    formTarjeta.addEventListener("submit", (ev) => {
      ev.preventDefault();
      ocupar(formTarjeta.querySelector("button[type=submit]"), async () => {
        const m = await (comercio || api("pagos.php?accion=comercio"));
        if (!m) throw new Error("los pagos con tarjeta no están disponibles");
        const d = Object.fromEntries(new FormData(formTarjeta));
        const r = await fetch(`${m.api}/tokens/cards`, {
          method: "POST",
          headers: { "Content-Type": "application/json", Authorization: `Bearer ${m.publica}` },
          body: JSON.stringify({ number: d.number.replace(/\D/g, ""), cvc: d.cvc, exp_month: d.exp_month.padStart(2, "0"), exp_year: d.exp_year.slice(-2), card_holder: d.card_holder }),
        });
        const tok = await r.json().catch(() => ({}));
        if (!r.ok || !tok.data?.id) throw new Error(tok.error?.messages ? "Revisa los datos de la tarjeta" : "Wompi no aceptó la tarjeta");
        const res = await api("pagos.php?accion=tarjeta", {
          token: tok.data.id, marca: tok.data.brand, ultimos: tok.data.last_four,
          aceptacion: m.aceptacion, datos_personales: m.datos_personales, plan: local.plan, periodo: local.periodo,
        });
        formTarjeta.reset();
        h.avisar(res.estado === "GUARDADA" ? "Tarjeta guardada" : res.aplicado ? "Tarjeta guardada y pago aprobado" : res.estado === "PENDING" ? "Tarjeta guardada; el pago está en proceso" : "Tarjeta guardada, pero el cobro fue rechazado");
        await refrescarCuenta();
      });
    });
  }

  $(main, "#cu-clave")?.addEventListener("submit", (ev) => {
    ev.preventDefault();
    ocupar(ev.target.querySelector("button"), async () => {
      await api("cuenta.php?accion=clave", Object.fromEntries(new FormData(ev.target)));
      ev.target.reset();
      h.avisar("Contraseña cambiada");
    });
  });
  $(main, "#cu-invitar")?.addEventListener("submit", (ev) => {
    ev.preventDefault();
    ocupar(ev.target.querySelector("button"), async () => {
      const d = Object.fromEntries(new FormData(ev.target));
      const r = await api("cuenta.php?accion=invitar", { ...d, rol });
      h.avisar(`Invitación enviada. Contraseña temporal: ${r.temporal}`);
      local.usuarios = null;
      await refrescarCuenta();
    });
  });

  // Usuarios y pagos (se piden al servidor).
  api("cuenta.php?accion=usuarios").then(({ usuarios }) => {
    const caja = $(main, "#cu-usuarios");
    if (!caja) return;
    caja.classList.remove("tenue");
    caja.innerHTML = `<ul class="lista-usuarios">${usuarios.map((x) => `<li class="${x.activo ? "" : "inactivo"}">
      <div><b>${esc(x.nombre)}</b><span class="tenue">${esc(x.email)} · ${ROLES[x.rol] || x.rol}${x.ultimo_ingreso ? ` · entró ${fecha(x.ultimo_ingreso)}` : ""}</span></div>
      ${puedeAdministrar && !["dueno", "super"].includes(x.rol) && x.id !== u.id ? `<button class="boton boton-claro boton-chico" data-usuario="${x.id}" data-activo="${x.activo ? 0 : 1}" type="button">${x.activo ? "Desactivar" : "Activar"}</button>` : ""}
    </li>`).join("")}</ul>`;
    caja.querySelectorAll("[data-usuario]").forEach((b) => b.addEventListener("click", () => ocupar(b, async () => {
      await api("cuenta.php?accion=usuario", { id: Number(b.dataset.usuario), activo: b.dataset.activo === "1" });
      await refrescarCuenta();
    })));
  }).catch((err) => { const caja = $(main, "#cu-usuarios"); if (caja) caja.textContent = err.message; });
  api("pagos.php?accion=historial").then(({ pagos }) => {
    const caja = $(main, "#cu-pagos");
    if (!caja) return;
    caja.classList.remove("tenue");
    caja.innerHTML = pagos.length ? `<table class="tabla"><thead><tr><th>Fecha</th><th>Plan</th><th>Valor</th><th>Estado</th></tr></thead><tbody>${pagos.map((p) => `<tr>
      <td>${fecha(p.creado)}</td><td>${esc(planes.lista.find((x) => x.id === p.plan)?.nombre || p.plan)} · ${p.periodo}${p.automatico ? " · auto" : ""}</td><td class="num">${pesos(p.monto)}</td>
      <td><span class="sello sello-${p.estado === "APPROVED" ? "ok" : p.estado === "PENDING" ? "aviso" : "error"}">${ESTADO_PAGO[p.estado] || p.estado}</span></td></tr>`).join("")}</tbody></table>`
      : `<p class="tenue">Aún no hay pagos.</p>`;
  }).catch((err) => { const caja = $(main, "#cu-pagos"); if (caja) caja.textContent = err.message; });
}

function $(raiz, sel) { return raiz.querySelector(sel); }

// ───────────── Panel del administrador de Macula ─────────────
export function vistaAdmin(main, h) {
  const { esc } = h;
  if (h.estado.cuenta?.usuario?.rol !== "super") { main.innerHTML = `<div class="error-caja">Solo el administrador de Macula.</div>`; return; }
  main.innerHTML = `<div class="encabezado"><div><h1>Panel de <span class="serif">Macula</span></h1><p>Empresas, pagos, precios y pasarela.</p></div></div><div id="ad-contenido"><div class="cargando"><span class="orbe orbe-respira"></span></div></div>`;
  Promise.all([api("admin.php?accion=resumen"), api("admin.php?accion=config")]).then(([r, c]) => {
    const caja = $(main, "#ad-contenido");
    const filtro = local.filtro || "";
    const empresas = r.empresas.filter((x) => !filtro || x.estado === filtro);
    caja.innerHTML = `
      <section class="tarjeta-oscura">
        <div class="metricas metricas-5">
          <div class="metrica"><b>${pesos(r.ingreso_mensual)}</b><span>ingreso mensual recurrente</span></div>
          ${Object.entries(r.conteo).map(([k, v]) => `<div class="metrica"><b>${v}</b><span>${ESTADOS[k]?.[0] || k}</span></div>`).join("")}
        </div>
        ${!r.pasarela ? `<p class="explicacion" style="margin-top:16px">⚠ Falta configurar las llaves de Wompi: mientras tanto nadie puede pagar en línea (solo activaciones manuales).</p>` : `<p class="explicacion" style="margin-top:16px">Wompi en modo <span class="resaltado">${r.modo === "produccion" ? "producción" : "pruebas (sandbox)"}</span>.</p>`}
      </section>
      <section class="tarjeta paso">
        <div class="paso-titulo"><h3>Empresas</h3></div>
        ${h.chips("filtro", [["", "Todas"], ["prueba", "En prueba"], ["activa", "Al día"], ["gracia", "Pago pendiente"], ["vencida", "Vencidas"], ["bloqueada", "Suspendidas"]], filtro)}
        <div class="tabla-desliza"><table class="tabla"><thead><tr><th>Empresa</th><th>Plan</th><th>Estado</th><th>Hasta</th><th>Pagado</th><th></th></tr></thead><tbody>
          ${empresas.map((x) => `<tr>
            <td><b>${esc(x.nombre)}</b>${x.aliado ? ` <span class="etiqueta">aliado</span>` : ""}<br><span class="tenue">${x.tipo === "empresa" ? "NIT" : "CC"} ${esc(x.documento)} · ${esc(x.email)} · ${esc(x.telefono)} · ${x.usuarios} usuarios${x.ultimo_ingreso ? ` · último ingreso ${fecha(x.ultimo_ingreso)}` : ""}</span></td>
            <td>${esc(x.plan)} · ${x.periodo}<br><span class="tenue">${pesos(x.precio)}${x.descuento ? ` (−${x.descuento} %)` : ""}${x.tarjeta ? ` · ${esc(x.tarjeta)}` : ""}</span></td>
            <td><span class="sello sello-${ESTADOS[x.estado]?.[1] || "aviso"}">${ESTADOS[x.estado]?.[0] || x.estado}</span></td>
            <td class="num">${fecha(x.acceso_hasta || x.pagado_hasta || x.prueba_hasta)}</td>
            <td class="num">${pesos(x.pagado_total)}</td>
            <td><details class="acciones-empresa"><summary class="boton boton-claro boton-chico">Acciones</summary><div class="paso">
              <div class="fila-2"><label class="campo"><span>Días de acceso (+/−)</span><input type="number" data-k="dias" value="30"></label>
                <label class="campo"><span>Registrar pago manual ($)</span><input type="number" data-k="registrar_pago" value="0"></label></div>
              <button class="boton boton-chico" data-accion="dias" data-id="${x.id}">Aplicar días</button>
              <div class="fila-2"><label class="campo"><span>Descuento %</span><input type="number" data-k="descuento" value="${x.descuento}" min="0" max="100"></label>
                <label class="campo"><span>Plan</span><select data-k="plan">${Object.entries(c.planes).map(([id, p]) => `<option value="${esc(id)}" ${id === x.plan ? "selected" : ""}>${esc(p.nombre)}</option>`).join("")}</select></label></div>
              <button class="boton boton-claro boton-chico" data-accion="guardar" data-id="${x.id}">Guardar plan y descuento</button>
              <div class="acciones-resultado">
                <button class="boton boton-claro boton-chico" data-accion="aliado" data-id="${x.id}" data-valor="${x.aliado ? 0 : 1}">${x.aliado ? "Quitar aliado" : "Marcar aliado"}</button>
                <button class="boton boton-claro boton-chico" data-accion="bloquear" data-id="${x.id}" data-valor="${x.bloqueada ? 0 : 1}">${x.bloqueada ? "Reactivar" : "Suspender"}</button>
              </div>
              <label class="campo"><span>Notas</span><textarea data-k="notas" rows="2">${esc(x.notas)}</textarea></label>
              <button class="boton boton-claro boton-chico" data-accion="notas" data-id="${x.id}">Guardar notas</button>
            </div></details></td></tr>`).join("") || `<tr><td colspan="6" class="tenue">Sin empresas en este estado.</td></tr>`}
        </tbody></table></div>
      </section>
      <div class="trabajo">
        <div class="panel">
          <section class="tarjeta paso">
            <div class="paso-titulo"><h3>Pasarela Wompi</h3></div>
            <p class="tenue">Copia las llaves desde el panel de comercios de Wompi › Desarrolladores. Las llaves guardadas no se vuelven a mostrar completas. En Wompi, pon esta URL de eventos: <code>${esc(c.webhook)}</code></p>
            <form id="ad-wompi" class="paso">
              ${h.chips("modo", [["sandbox", "Pruebas (sandbox)"], ["produccion", "Producción"]], c.wompi.modo)}
              <label class="campo"><span>Llave pública ${c.wompi.publica ? `<small>(${esc(c.wompi.publica)})</small>` : ""}</span><input type="text" name="publica" placeholder="pub_prod_…" autocomplete="off"></label>
              <label class="campo"><span>Llave privada ${c.wompi.privada ? `<small>(${esc(c.wompi.privada)})</small>` : ""}</span><input type="password" name="privada" placeholder="prv_prod_…" autocomplete="off"></label>
              <label class="campo"><span>Secreto de eventos ${c.wompi.eventos ? `<small>(${esc(c.wompi.eventos)})</small>` : ""}</span><input type="password" name="eventos" placeholder="prod_events_…" autocomplete="off"></label>
              <label class="campo"><span>Secreto de integridad ${c.wompi.integridad ? `<small>(${esc(c.wompi.integridad)})</small>` : ""}</span><input type="password" name="integridad" placeholder="prod_integrity_…" autocomplete="off"></label>
              <button class="boton" type="submit">Guardar pasarela</button>
            </form>
          </section>
          <section class="tarjeta paso">
            <div class="paso-titulo"><h3>Prueba, gracia y correo</h3></div>
            <form id="ad-reglas" class="paso">
              <div class="fila-3">
                <label class="campo"><span>Días de prueba</span><input type="number" name="prueba_dias" value="${c.prueba_dias}" min="0"></label>
                <label class="campo"><span>Días de gracia</span><input type="number" name="gracia_dias" value="${c.gracia_dias}" min="0"></label>
                <label class="campo"><span>Meses que cobra el anual</span><input type="number" name="anual_meses" value="${c.anual_meses}" min="1" max="12"></label>
              </div>
              <label class="campo"><span>Remitente de los correos</span><input type="email" name="correo_remitente" value="${esc(c.correo_remitente)}" placeholder="no-responder@tudominio.com"></label>
              <button class="boton boton-claro" type="submit">Guardar reglas</button>
            </form>
          </section>
        </div>
        <div class="columna-resultado">
          <section class="tarjeta paso">
            <div class="paso-titulo"><h3>Planes y precios (COP al mes)</h3></div>
            <form id="ad-planes" class="paso">
              ${Object.entries(c.planes).map(([id, p]) => `<div class="plan-edicion" data-plan-id="${esc(id)}">
                <div class="fila-3">
                  <label class="campo"><span>Nombre</span><input type="text" data-p="nombre" value="${esc(p.nombre)}"></label>
                  <label class="campo"><span>Usuarios</span><input type="number" data-p="usuarios" value="${p.usuarios}" min="1"></label>
                  <label class="campo"><span>Precio mensual</span><input type="number" data-p="mensual" value="${p.mensual}" min="0" step="1000"></label>
                </div>
                <label class="campo"><span>Descripción</span><input type="text" data-p="descripcion" value="${esc(p.descripcion || "")}"></label>
              </div>`).join("")}
              <button class="boton boton-claro" type="submit">Guardar precios</button>
            </form>
          </section>
          <section class="tarjeta paso">
            <div class="paso-titulo"><h3>Últimos pagos</h3></div>
            <div class="tabla-desliza"><table class="tabla"><thead><tr><th>Fecha</th><th>Empresa</th><th>Valor</th><th>Estado</th></tr></thead><tbody>
              ${r.pagos.map((p) => `<tr><td>${fecha(p.creado)}</td><td>${esc(p.empresa)}<br><span class="tenue">${esc(p.metodo)}${p.automatico ? " · automático" : ""}</span></td><td class="num">${pesos(p.monto)}</td><td><span class="sello sello-${p.estado === "APPROVED" ? "ok" : p.estado === "PENDING" ? "aviso" : "error"}">${ESTADO_PAGO[p.estado] || p.estado}</span></td></tr>`).join("") || `<tr><td colspan="4" class="tenue">Sin pagos.</td></tr>`}
            </tbody></table></div>
          </section>
        </div>
      </div>`;
    const redibujar = () => vistaAdmin(main, h);
    let modo = c.wompi.modo;
    caja.querySelectorAll("[data-chips]").forEach((g) => g.addEventListener("click", (ev) => {
      const b = ev.target.closest(".chip");
      if (!b) return;
      if (g.dataset.chips === "filtro") { local.filtro = b.dataset.valor; redibujar(); return; }
      modo = b.dataset.valor;
      g.querySelectorAll(".chip").forEach((x) => x.setAttribute("aria-pressed", String(x === b)));
    }));
    const guardar = async (datos, msg) => { try { await api("admin.php?accion=config", datos); h.avisar(msg); redibujar(); } catch (err) { h.avisar(err.message); } };
    $(caja, "#ad-wompi").addEventListener("submit", (ev) => { ev.preventDefault(); guardar({ wompi: { modo, ...Object.fromEntries(new FormData(ev.target)) } }, "Pasarela guardada"); });
    $(caja, "#ad-reglas").addEventListener("submit", (ev) => { ev.preventDefault(); guardar(Object.fromEntries(new FormData(ev.target)), "Reglas guardadas"); });
    $(caja, "#ad-planes").addEventListener("submit", (ev) => {
      ev.preventDefault();
      const planes = {};
      caja.querySelectorAll("[data-plan-id]").forEach((el) => {
        const p = {};
        el.querySelectorAll("[data-p]").forEach((i) => { p[i.dataset.p] = i.type === "number" ? Number(i.value) : i.value; });
        planes[el.dataset.planId] = p;
      });
      guardar({ planes }, "Precios guardados");
    });
    caja.querySelectorAll("[data-accion]").forEach((b) => b.addEventListener("click", async () => {
      const id = Number(b.dataset.id);
      const panel = b.closest(".paso");
      const valor = (k) => panel.querySelector(`[data-k="${k}"]`)?.value;
      const datos = { id };
      if (b.dataset.accion === "dias") { datos.dias = Number(valor("dias")); if (Number(valor("registrar_pago")) > 0) datos.registrar_pago = Number(valor("registrar_pago")); }
      if (b.dataset.accion === "guardar") { datos.descuento = Number(valor("descuento")); datos.plan = valor("plan"); }
      if (b.dataset.accion === "aliado") datos.aliado = b.dataset.valor === "1";
      if (b.dataset.accion === "bloquear") { if (b.dataset.valor === "1" && !confirm("¿Suspender esta empresa? Sus usuarios no podrán entrar.")) return; datos.bloqueada = b.dataset.valor === "1"; }
      if (b.dataset.accion === "notas") datos.notas = valor("notas");
      try { await api("admin.php?accion=empresa", datos); h.avisar("Listo"); redibujar(); } catch (err) { h.avisar(err.message); }
    }));
  }).catch((err) => { $(main, "#ad-contenido").innerHTML = `<div class="error-caja">${h.esc(err.message)}</div>`; });
}
