// Macula · acceso: entrar, crear la cuenta de la empresa, verificar el correo,
// recuperar la contraseña y pagar cuando la suscripción no está al día.
// Esta página es pública; la app solo se entrega a cuentas al día.

const $ = (s, el = document) => el.querySelector(s);
const $$ = (s, el = document) => [...el.querySelectorAll(s)];
const esc = (t) => String(t ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
const pesos = (v) => "$" + Math.round(v).toLocaleString("es-CO");
const fecha = (t) => new Date(t * 1000).toLocaleDateString("es-CO", { day: "numeric", month: "long", year: "numeric" });

const est = { yo: null, paso: "entrar", email: "", plan: "taller", periodo: "mensual", tipo: "empresa", error: "", ocupado: false };

async function api(ruta, datos) {
  const r = await fetch(`api/${ruta}`, datos === undefined
    ? { cache: "no-store" }
    : { method: "POST", headers: { "Content-Type": "application/json", "X-Macula": "1" }, body: JSON.stringify(datos) });
  const j = await r.json().catch(() => ({}));
  if (!r.ok) throw new Error(j.error || `error ${r.status}`);
  return j;
}

function avisar(texto) {
  const a = $("#aviso");
  a.textContent = texto;
  a.classList.add("visible");
  clearTimeout(avisar.t);
  avisar.t = setTimeout(() => a.classList.remove("visible"), 3500);
}

function chips(nombre, opciones, valor) {
  return `<div class="chips" role="group" data-chips="${nombre}">${opciones.map(([v, t]) => `<button type="button" class="chip" data-valor="${esc(v)}" aria-pressed="${v === valor}">${t}</button>`).join("")}</div>`;
}

function tarjetasPlanes(planes, elegido, periodo) {
  return `<div class="planes">${planes.lista.map((p) => `
    <button type="button" class="plan ${p.id === elegido ? "elegido" : ""}" data-plan="${esc(p.id)}" aria-pressed="${p.id === elegido}">
      <b>${esc(p.nombre)}</b>
      <span class="plan-precio">${pesos(periodo === "anual" ? p.anual : p.mensual)}<small>/${periodo === "anual" ? "año" : "mes"}</small></span>
      <span class="plan-usuarios">${p.usuarios === 1 ? "1 usuario" : `Hasta ${p.usuarios} usuarios`}</span>
      <span class="tenue">${esc(p.descripcion)}</span>
    </button>`).join("")}</div>
    <p class="tenue" style="font-size:13px">Precios en pesos colombianos. El plan anual cobra ${planes.anual_meses} meses.</p>`;
}

function pintar() {
  const m = $("#acceso");
  const yo = est.yo;
  const planes = yo?.planes;
  let html = "";
  const error = est.error ? `<div class="error-caja">${esc(est.error)}</div>` : "";
  const boton = (texto) => `<button class="boton boton-naranja" type="submit" ${est.ocupado ? "disabled" : ""}>${est.ocupado ? "Un momento…" : texto}</button>`;

  if (est.paso === "entrar") {
    html = `<section class="acceso-tarjeta tarjeta">
      <h1>Entra a <span class="serif">Macula</span></h1>
      <p class="tenue">Imposición, diagramación y portadas para tu imprenta.</p>
      ${error}
      <form id="f-entrar" class="paso">
        <label class="campo"><span>Correo</span><input type="email" name="email" autocomplete="username" required value="${esc(est.email)}"></label>
        <label class="campo"><span>Contraseña</span><input type="password" name="clave" autocomplete="current-password" required></label>
        ${boton("Entrar")}
      </form>
      <p class="acceso-enlaces"><button class="enlace" data-ir="recuperar">Olvidé mi contraseña</button> · <button class="enlace" data-ir="registro">Crear cuenta para mi empresa</button></p>
    </section>`;
  } else if (est.paso === "registro") {
    html = `<section class="acceso-tarjeta acceso-ancha tarjeta">
      <h1>Crea la cuenta de tu <span class="serif">empresa</span></h1>
      <p class="tenue">${planes?.prueba_dias ? `Cada empresa nueva tiene una prueba gratis de ${planes.prueba_dias} días, sin tarjeta. Una sola prueba por NIT, cédula, correo y teléfono.` : ""}</p>
      ${error}
      <form id="f-registro" class="paso">
        ${chips("tipo", [["empresa", "Empresa (NIT)"], ["independiente", "Independiente (cédula)"]], est.tipo)}
        <div class="fila-2">
          <label class="campo"><span>${est.tipo === "empresa" ? "Razón social" : "Nombre comercial (opcional)"}</span><input type="text" name="empresa" ${est.tipo === "empresa" ? "required" : ""} autocomplete="organization"></label>
          <label class="campo"><span>${est.tipo === "empresa" ? "NIT" : "Cédula"}</span><input type="text" name="documento" required inputmode="numeric" autocomplete="off"></label>
        </div>
        <div class="fila-2">
          <label class="campo"><span>Tu nombre</span><input type="text" name="nombre" required autocomplete="name"></label>
          <label class="campo"><span>Teléfono</span><input type="tel" name="telefono" required autocomplete="tel"></label>
        </div>
        <div class="fila-2">
          <label class="campo"><span>Correo</span><input type="email" name="email" required autocomplete="email" value="${esc(est.email)}"></label>
          <label class="campo"><span>Contraseña (8 o más)</span><input type="password" name="clave" required minlength="8" autocomplete="new-password"></label>
        </div>
        ${planes ? `<div class="campo"><span>Plan</span>${chips("periodo", [["mensual", "Mensual"], ["anual", "Anual (2 meses gratis)"]], est.periodo)}${tarjetasPlanes(planes, est.plan, est.periodo)}</div>` : ""}
        <label class="interruptor"><span>Acepto los términos del servicio y el tratamiento de mis datos para operar la cuenta.</span><input type="checkbox" name="acepto" required></label>
        ${boton("Crear cuenta")}
      </form>
      <p class="acceso-enlaces">¿Ya tienes cuenta? <button class="enlace" data-ir="entrar">Entrar</button></p>
    </section>`;
  } else if (est.paso === "verificar") {
    html = `<section class="acceso-tarjeta tarjeta">
      <h1>Revisa tu <span class="serif">correo</span></h1>
      <p class="tenue">Enviamos un código de 6 dígitos a <b>${esc(est.email)}</b>. Si no llega en un minuto, mira en correo no deseado.</p>
      ${error}
      <form id="f-verificar" class="paso">
        <label class="campo"><span>Código</span><input type="text" name="codigo" inputmode="numeric" maxlength="6" required autocomplete="one-time-code" class="codigo"></label>
        ${boton("Activar cuenta")}
      </form>
      <p class="acceso-enlaces"><button class="enlace" id="reenviar">Reenviar el código</button> · <button class="enlace" data-ir="entrar">Volver</button></p>
    </section>`;
  } else if (est.paso === "recuperar" || est.paso === "restablecer") {
    const segundo = est.paso === "restablecer";
    html = `<section class="acceso-tarjeta tarjeta">
      <h1>Nueva <span class="serif">contraseña</span></h1>
      <p class="tenue">${segundo ? `Escribe el código que enviamos a <b>${esc(est.email)}</b> y tu nueva contraseña.` : "Te enviamos un código a tu correo para crear una contraseña nueva."}</p>
      ${error}
      <form id="${segundo ? "f-restablecer" : "f-recuperar"}" class="paso">
        ${segundo ? `<label class="campo"><span>Código</span><input type="text" name="codigo" inputmode="numeric" maxlength="6" required class="codigo"></label>
          <label class="campo"><span>Nueva contraseña (8 o más)</span><input type="password" name="clave" minlength="8" required autocomplete="new-password"></label>`
        : `<label class="campo"><span>Correo</span><input type="email" name="email" required value="${esc(est.email)}"></label>`}
        ${boton(segundo ? "Guardar y entrar" : "Enviar código")}
      </form>
      <p class="acceso-enlaces"><button class="enlace" data-ir="entrar">Volver</button></p>
    </section>`;
  } else if (est.paso === "pagar") {
    const e = yo.empresa, s = yo.suscripcion;
    const motivo = s.estado === "bloqueada" ? "La cuenta está suspendida. Escríbenos para revisarla."
      : e.pagado_hasta ? `La suscripción venció el ${fecha(e.pagado_hasta + s.gracia_dias * 86400)}.`
      : e.prueba_hasta ? `La prueba gratis terminó el ${fecha(e.prueba_hasta)}.`
      : "Esta empresa ya usó su prueba gratis. Elige un plan para empezar.";
    const puedePagar = ["dueno", "admin", "super"].includes(yo.usuario.rol) && s.estado !== "bloqueada";
    html = `<section class="acceso-tarjeta acceso-ancha tarjeta">
      <h1>Activa <span class="serif">${esc(e.nombre)}</span></h1>
      <p>${esc(motivo)} Mientras tanto no se puede montar, diagramar ni generar PDF. Tus máquinas, papeles y datos se conservan.</p>
      ${error}
      ${puedePagar ? `
        <div class="campo"><span>Plan</span>${chips("periodo", [["mensual", "Mensual"], ["anual", "Anual (2 meses gratis)"]], est.periodo)}${tarjetasPlanes(planes, est.plan, est.periodo)}</div>
        <div class="acciones-resultado">
          <button class="boton boton-naranja" id="pagar-checkout" type="button" ${est.ocupado ? "disabled" : ""}>Pagar ${pesos(precioDe(planes, est.plan, est.periodo, e.descuento))} con PSE, Nequi o tarjeta</button>
        </div>
        <p class="tenue" style="font-size:13px">El pago lo procesa Wompi (Bancolombia). Al aprobarse, la cuenta se activa sola. Para que se renueve automáticamente, guarda una tarjeta en Mi cuenta.</p>`
        : `<p class="tenue">Pide al administrador de tu empresa que renueve la suscripción.</p>`}
      <p class="acceso-enlaces"><button class="enlace" id="salir">Cerrar sesión (${esc(yo.usuario.email)})</button></p>
    </section>`;
  } else if (est.paso === "confirmando") {
    html = `<section class="acceso-tarjeta tarjeta"><div class="componiendo"><span class="orbe orbe-respira" aria-hidden="true"></span><p>${esc(est.mensaje || "Confirmando el pago con Wompi…")}</p></div>${error}
      ${est.reintentar ? `<p class="acceso-enlaces"><button class="enlace" id="reconfirmar">Volver a consultar</button> · <a class="enlace" href="/acceso.html">Ir a mi cuenta</a></p>` : ""}</section>`;
  }
  m.innerHTML = `<div class="acceso-fondo"><span class="orbe" aria-hidden="true"></span></div>${html}`;
  conectar();
}

function precioDe(planes, id, periodo, descuento = 0) {
  const p = planes.lista.find((x) => x.id === id) || planes.lista[0];
  return Math.round((periodo === "anual" ? p.anual : p.mensual) * (100 - descuento) / 100);
}

function ir(paso) { est.paso = paso; est.error = ""; pintar(); $("input:not([type=checkbox])", $("#acceso"))?.focus(); }

async function enviar(form, fn) {
  form.addEventListener("submit", async (e) => {
    e.preventDefault();
    if (est.ocupado) return;
    const datos = Object.fromEntries(new FormData(form));
    est.ocupado = true; est.error = ""; pintar();
    try { await fn(datos); } catch (err) { est.error = err.message; }
    est.ocupado = false;
    pintar();
  });
}

function entrarALaApp() { location.href = "/"; }

function conectar() {
  $$("[data-ir]").forEach((b) => b.addEventListener("click", () => ir(b.dataset.ir)));
  $$("[data-chips]").forEach((g) => g.addEventListener("click", (e) => {
    const b = e.target.closest(".chip");
    if (!b) return;
    guardarFormulario();
    est[g.dataset.chips] = b.dataset.valor;
    pintar();
    restaurarFormulario();
  }));
  $$("[data-plan]").forEach((b) => b.addEventListener("click", () => { guardarFormulario(); est.plan = b.dataset.plan; pintar(); restaurarFormulario(); }));
  const f = (id) => $(`#${id}`);
  if (f("f-entrar")) enviar(f("f-entrar"), async (d) => {
    est.email = d.email;
    const r = await api("cuenta.php?accion=entrar", d);
    if (r.verificar) { est.paso = "verificar"; return; }
    await despues();
  });
  if (f("f-registro")) enviar(f("f-registro"), async (d) => {
    est.email = d.email;
    const r = await api("cuenta.php?accion=registrar", { ...d, tipo: est.tipo, plan: est.plan, periodo: est.periodo, empresa: d.empresa || d.nombre });
    est.prueba = r.prueba;
    est.paso = "verificar";
  });
  if (f("f-verificar")) enviar(f("f-verificar"), async (d) => {
    const r = await api("cuenta.php?accion=verificar", { email: est.email, codigo: d.codigo });
    avisar(r.prueba ? "Cuenta activada: empieza tu prueba gratis" : "Cuenta activada");
    await despues();
  });
  if (f("f-recuperar")) enviar(f("f-recuperar"), async (d) => {
    est.email = d.email;
    await api("cuenta.php?accion=recuperar", d);
    est.paso = "restablecer";
  });
  if (f("f-restablecer")) enviar(f("f-restablecer"), async (d) => {
    await api("cuenta.php?accion=restablecer", { ...d, email: est.email });
    await despues();
  });
  $("#reenviar")?.addEventListener("click", async () => { await api("cuenta.php?accion=reenviar", { email: est.email }).catch(() => {}); avisar("Código reenviado"); });
  $("#salir")?.addEventListener("click", async () => { await api("cuenta.php?accion=salir", {}); est.yo = null; ir("entrar"); });
  $("#pagar-checkout")?.addEventListener("click", async () => {
    est.ocupado = true; pintar();
    try {
      const r = await api("pagos.php?accion=checkout", { plan: est.plan, periodo: est.periodo });
      if (r.aplicado) return entrarALaApp();
      location.href = r.url;
    } catch (err) { est.error = err.message; est.ocupado = false; pintar(); }
  });
  $("#reconfirmar")?.addEventListener("click", () => confirmarPago());
}

// Los datos escritos sobreviven al repintar (al cambiar tipo, plan o periodo).
let borrador = {};
function guardarFormulario() { $$("#acceso input").forEach((i) => { if (i.name) borrador[i.name] = i.type === "checkbox" ? i.checked : i.value; }); }
function restaurarFormulario() { $$("#acceso input").forEach((i) => { if (i.name in borrador) { if (i.type === "checkbox") i.checked = borrador[i.name]; else i.value = borrador[i.name]; } }); }

/** Tras entrar o verificar: a la app si la cuenta está al día; si no, a pagar. */
async function despues() {
  est.yo = await api("cuenta.php?accion=yo");
  if (est.yo.sesion && est.yo.suscripcion.puede_usar) return entrarALaApp();
  est.plan = est.yo.empresa?.plan || est.plan;
  est.periodo = est.yo.empresa?.periodo || est.periodo;
  est.paso = est.yo.sesion ? "pagar" : "entrar";
}

async function confirmarPago() {
  const q = new URLSearchParams(location.search);
  const id = q.get("id");
  est.paso = "confirmando"; est.reintentar = false; est.error = ""; est.mensaje = "Confirmando el pago con Wompi…"; pintar();
  for (let i = 0; i < 8; i++) {
    try {
      const r = await api(`pagos.php?accion=confirmar&id=${encodeURIComponent(id || "")}`);
      if (r.aplicado) { est.mensaje = "¡Pago aprobado! Entrando a Macula…"; pintar(); setTimeout(entrarALaApp, 1200); return; }
      if (["DECLINED", "ERROR", "VOIDED"].includes(r.estado)) { est.error = "El pago fue rechazado. Puedes intentarlo de nuevo con otro medio."; break; }
      est.mensaje = "El pago está en proceso (PSE puede tardar unos minutos)…";
    } catch (err) { est.error = err.message; }
    pintar();
    await new Promise((r) => setTimeout(r, 4000));
  }
  est.reintentar = true;
  pintar();
}

async function iniciar() {
  try { est.yo = await api("cuenta.php?accion=yo"); } catch (e) { est.error = `No hay conexión con el servidor (${e.message}).`; }
  const q = new URLSearchParams(location.search);
  if (q.get("pago") && est.yo?.sesion) return confirmarPago();
  if (est.yo?.sesion) {
    if (est.yo.suscripcion.puede_usar && location.hash !== "#pagar") return entrarALaApp();
    est.plan = est.yo.empresa.plan;
    est.periodo = est.yo.empresa.periodo;
    est.paso = "pagar";
  } else if (location.hash === "#registro") est.paso = "registro";
  pintar();
}
iniciar();
