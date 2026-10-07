// Montajes · interfaz web. El motor (Rust → WebAssembly) hace todo el cálculo
// y escribe los PDF; aquí solo se piden datos, se dibuja la vista previa y se
// guardan los catálogos en el navegador.

import iniciarMotor, * as motor from "./motor/montajes_web.js";
import { aMaquina, buscarMaquinas } from "./maquinas-catalogo.js";

const $ = (s, el = document) => el.querySelector(s);
const $$ = (s, el = document) => [...el.querySelectorAll(s)];
const esc = (t) => String(t ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
const mm = (v, d = 1) => (Math.round(v * 10 ** d) / 10 ** d).toLocaleString("es-CO", { maximumFractionDigits: d });
const num = (v, def = 0) => { const n = parseFloat(String(v).replace(",", ".")); return Number.isFinite(n) ? n : def; };
const ahora = () => Math.floor(Date.now() / 1000);

// ───────────── Almacenamiento ─────────────
const almacen = {
  leer(clave, defecto) { try { const v = localStorage.getItem("montajes:" + clave); return v ? JSON.parse(v) : defecto; } catch { return defecto; } },
  guardar(clave, valor) { try { localStorage.setItem("montajes:" + clave, JSON.stringify(valor)); } catch { avisar("No se pudo guardar en este navegador"); } },
};

// Perfiles ICC en IndexedDB (pueden pesar más de lo que admite localStorage).
const iccDB = {
  abrir() {
    return new Promise((ok, mal) => {
      const r = indexedDB.open("montajes", 1);
      r.onupgradeneeded = () => r.result.createObjectStore("icc");
      r.onsuccess = () => ok(r.result);
      r.onerror = () => mal(r.error);
    });
  },
  async poner(id, bytes) { const db = await this.abrir(); return new Promise((ok, mal) => { const t = db.transaction("icc", "readwrite"); t.objectStore("icc").put(bytes, id); t.oncomplete = ok; t.onerror = () => mal(t.error); }); },
  async leer(id) {
    try { const db = await this.abrir(); return await new Promise((ok) => { const r = db.transaction("icc").objectStore("icc").get(id); r.onsuccess = () => ok(r.result || null); r.onerror = () => ok(null); }); }
    catch { return null; }
  },
  async borrar(id) { try { const db = await this.abrir(); db.transaction("icc", "readwrite").objectStore("icc").delete(id); } catch { /* sin almacenamiento */ } },
};

const estado = {
  maquinas: almacen.leer("maquinas", []),
  papeles: almacen.leer("papeles", []),
  preferencias: almacen.leer("preferencias", {}),
  piezas: { archivo: null, archivos: [], info: null, plan: null, error: null, cara: 0, op: { modo: "repetir", cantidades: [], rebase: 3, calle: 0, orientacion: "auto", dorso: false, volteo: "lateral", marcas: true, tira: true } },
  libro: { archivo: null, info: null, plan: null, error: null, cara: 0, op: { encuadernacion: "lomo", firma: "auto", aprovechamiento: "auto", rebase: 3, fresado: 3, refile: 3, rtl: false, marcas: true, tira: true, creep: true } },
  portada: { archivo: null, info: null, calculo: null, error: null, op: { ancho: 148, alto: 210, paginas: 240, lomo: "", tipo: "rustica", solapa: 0, carton: 2.5, escuadra: 3, vuelta: 15, bisagra: 8, rebase: 3, rtl: false, orden: ["tapa", "contratapa", "lomo", "solapa_tapa", "solapa_contratapa"] } },
};

// Correcciones automáticas: preferencia del taller, común a piezas y libros.
const CORRECCIONES = [
  ["sobreimprimir_negro", "Sobreimprimir el negro 100 %", "Evita filetes blancos si el registro se mueve."],
  ["quitar_sobreimpresion_blanco", "Quitar sobreimpresión de blancos", "Si no, los objetos blancos desaparecen al imprimir."],
  ["linea_minima", "Engrosar líneas finas a 0,25 pt", "Las más finas pueden no verse."],
  ["rebase_espejo", "Rebase en espejo si falta", "Refleja el borde de la página sobre el rebase."],
];
function correcciones() {
  const c = { sobreimprimir_negro: true, quitar_sobreimpresion_blanco: true, linea_minima: true, rebase_espejo: false, ...(estado.preferencias.correcciones || {}) };
  return { ...c, linea_minima: c.linea_minima ? 0.25 : null };
}
function bloqueCorrecciones(prefijo) {
  const c = correcciones();
  return `<details class="avanzado"><summary>Correcciones automáticas</summary><div class="paso">
    ${CORRECCIONES.map(([k, t, d]) => `<label class="interruptor"><span>${t}<br><small class="tenue">${d}</small></span><input type="checkbox" data-correccion="${k}" id="${prefijo}-${k}" ${c[k] ? "checked" : ""}></label>`).join("")}
  </div></details>`;
}
function conectarCorrecciones(raiz, alCambiar) {
  $$("[data-correccion]", raiz).forEach((el) => el.addEventListener("change", (e) => {
    e.stopPropagation();
    const actual = { ...(estado.preferencias.correcciones || {}) };
    actual[el.dataset.correccion] = el.checked;
    preferir("correcciones", actual);
    alCambiar();
  }));
}

// ───────────── Catálogos: guardado local y nube del taller ─────────────
// Cada elemento lleva «modificado» (ms); los borrados se recuerdan para que
// también desaparezcan en los demás equipos al sincronizar.
estado.borrados = almacen.leer("borrados", { maquinas: {}, papeles: {} });
estado.nube = { estado: "apagada", cuando: 0 };
const huellas = new Map();
const sinMarca = (x) => JSON.stringify({ ...x, modificado: undefined });
for (const x of [...estado.maquinas, ...estado.papeles]) huellas.set(x, sinMarca(x));

function guardarCatalogos(sincronizar = true) {
  const ahoraMs = Date.now();
  for (const x of [...estado.maquinas, ...estado.papeles]) {
    const h = sinMarca(x);
    if (huellas.get(x) !== h) {
      x.modificado = ahoraMs;
      huellas.set(x, h);
    }
  }
  almacen.guardar("maquinas", estado.maquinas);
  almacen.guardar("papeles", estado.papeles);
  almacen.guardar("borrados", estado.borrados);
  if (sincronizar) programarSincronizacion();
}

function borrarDelCatalogo(tipo, elemento) {
  estado[tipo] = estado[tipo].filter((x) => x !== elemento);
  estado.borrados[tipo][elemento.id] = Date.now();
  guardarCatalogos();
}

/** Une dos listas por id: gana la versión modificada más tarde; respeta borrados. */
function unirListas(local, remota, borrados) {
  const porId = new Map();
  for (const x of [...remota, ...local]) {
    const previo = porId.get(x.id);
    if (!previo || (x.modificado || 0) > (previo.modificado || 0)) porId.set(x.id, x);
  }
  return [...porId.values()].filter((x) => !(borrados[x.id] > (x.modificado || 0)));
}

const codigoTaller = () => almacen.leer("codigoTaller", "");
function nuevoCodigoTaller() {
  const letras = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
  const bytes = crypto.getRandomValues(new Uint8Array(20));
  const s = [...bytes].map((b) => letras[b % letras.length]).join("");
  return s.match(/.{4}/g).join("-");
}

let temporizadorNube;
function programarSincronizacion() {
  if (!codigoTaller()) return;
  clearTimeout(temporizadorNube);
  temporizadorNube = setTimeout(sincronizar, 700);
}

async function sincronizar() {
  const codigo = codigoTaller();
  if (!codigo) return;
  estado.nube = { ...estado.nube, estado: "sincronizando" };
  pintarNube();
  try {
    const r = await fetch("api/catalogo.php", { headers: { "X-Taller": codigo }, cache: "no-store" });
    if (!r.ok) throw new Error(`servidor ${r.status}`);
    const remoto = await r.json();
    const borrados = { maquinas: { ...(remoto.borrados?.maquinas || {}), ...estado.borrados.maquinas }, papeles: { ...(remoto.borrados?.papeles || {}), ...estado.borrados.papeles } };
    estado.borrados = borrados;
    estado.maquinas = unirListas(estado.maquinas, remoto.maquinas || [], borrados.maquinas);
    estado.papeles = unirListas(estado.papeles, remoto.papeles || [], borrados.papeles);
    for (const x of [...estado.maquinas, ...estado.papeles]) huellas.set(x, sinMarca(x));
    guardarCatalogos(false);
    const igual = JSON.stringify(remoto.maquinas || []) === JSON.stringify(estado.maquinas)
      && JSON.stringify(remoto.papeles || []) === JSON.stringify(estado.papeles)
      && JSON.stringify(remoto.borrados || {}) === JSON.stringify(borrados);
    if (igual) { estado.nube = { estado: "ok", cuando: Date.now() }; pintarNube(); return; }
    const g = await fetch("api/catalogo.php", {
      method: "POST",
      headers: { "X-Taller": codigo, "Content-Type": "application/json" },
      body: JSON.stringify({ maquinas: estado.maquinas, papeles: estado.papeles, borrados }),
    });
    if (!g.ok) throw new Error(`servidor ${g.status}`);
    estado.nube = { estado: "ok", cuando: Date.now() };
  } catch (e) {
    console.warn("Sincronización:", e);
    estado.nube = { estado: "error", cuando: estado.nube.cuando };
  }
  pintarNube();
  // Si se está viendo un catálogo, se refresca con lo que llegó de otros equipos.
  if ((location.hash || "#inicio") === "#catalogos" && !$("#ct-dialogo")?.open) {
    const contenido = $("#ct-contenido");
    if (contenido) (estado.preferencias.pestanaCatalogo || "maquinas") === "maquinas" ? listaMaquinas(contenido, $("#vista")) : listaPapeles(contenido, $("#vista"));
  }
}

function textoNube() {
  const n = estado.nube;
  if (!codigoTaller()) return "";
  if (n.estado === "sincronizando") return "Sincronizando…";
  if (n.estado === "error") return "Sin conexión con la nube: se guardó en este equipo y se sincroniza al volver.";
  if (n.estado === "ok") return `Guardado en la nube del taller · ${new Date(n.cuando).toLocaleTimeString("es-CO", { hour: "2-digit", minute: "2-digit" })}`;
  return "Nube del taller conectada";
}
function pintarNube() {
  const el = $("#nube-estado");
  if (el) {
    el.textContent = textoNube();
    el.dataset.estado = estado.nube.estado;
  }
}

function bloqueNube() {
  const codigo = codigoTaller();
  if (codigo) {
    return `<section class="tarjeta nube">
      <div class="nube-cabeza"><span class="orbe" aria-hidden="true"></span><div><h3>Nube del taller</h3><p id="nube-estado" class="tenue" data-estado="${estado.nube.estado}">${esc(textoNube())}</p></div></div>
      <div class="nube-codigo"><span class="tenue">Código del taller</span><code>${esc(codigo)}</code>
        <button class="boton boton-claro boton-chico" type="button" id="nube-copiar">Copiar</button>
        <button class="boton boton-claro boton-chico" type="button" id="nube-salir">Desconectar este equipo</button></div>
      <small class="tenue">Escribe este código en cada computador o celular del taller para ver las mismas máquinas y papeles. Guárdalo en un lugar seguro: quien lo tenga puede ver y editar los catálogos.</small>
    </section>`;
  }
  return `<section class="tarjeta nube nube-apagada">
    <div class="nube-cabeza"><span class="orbe" aria-hidden="true"></span><div><h3>Guarda tus catálogos en la nube del taller</h3><p class="tenue">Ahora solo están en este navegador: si cambias de equipo o se borran los datos del navegador, se pierden. Con la nube quedan en tu servidor y en todos los equipos del taller.</p></div></div>
    <div class="acciones-resultado">
      <button class="boton boton-naranja" type="button" id="nube-crear">Crear código del taller</button>
      <form id="nube-unir" class="nube-unir"><input type="text" name="codigo" placeholder="Ya tengo un código: XXXX-XXXX-…" autocomplete="off" aria-label="Código del taller"><button class="boton boton-claro">Conectar</button></form>
    </div>
  </section>`;
}

function conectarNube(main) {
  $("#nube-crear")?.addEventListener("click", async () => {
    almacen.guardar("codigoTaller", nuevoCodigoTaller());
    await sincronizar();
    avisar("Catálogos guardados en la nube del taller");
    vistaCatalogos(main);
  });
  $("#nube-unir")?.addEventListener("submit", async (e) => {
    e.preventDefault();
    const codigo = String(new FormData(e.target).get("codigo") || "").trim().toUpperCase();
    if (!/^[A-Z0-9-]{16,64}$/.test(codigo)) { avisar("Ese código no es válido"); return; }
    almacen.guardar("codigoTaller", codigo);
    await sincronizar();
    avisar(estado.nube.estado === "ok" ? "Equipo conectado a la nube del taller" : "No se pudo conectar; se reintentará");
    vistaCatalogos(main);
  });
  $("#nube-copiar")?.addEventListener("click", async () => {
    try { await navigator.clipboard.writeText(codigoTaller()); avisar("Código copiado"); } catch { avisar(codigoTaller()); }
  });
  $("#nube-salir")?.addEventListener("click", () => {
    if (!confirm("¿Desconectar este equipo de la nube? Los catálogos quedan en el servidor y en este navegador.")) return;
    almacen.guardar("codigoTaller", "");
    estado.nube = { estado: "apagada", cuando: 0 };
    vistaCatalogos(main);
  });
}
function preferir(clave, valor) { estado.preferencias[clave] = valor; almacen.guardar("preferencias", estado.preferencias); }

let temporizadorAviso;
function avisar(texto) {
  const a = $("#aviso");
  a.textContent = texto;
  a.classList.add("visible");
  clearTimeout(temporizadorAviso);
  temporizadorAviso = setTimeout(() => a.classList.remove("visible"), 3200);
}

function descargar(bytes, nombre) {
  const url = URL.createObjectURL(new Blob([bytes], { type: "application/pdf" }));
  const a = Object.assign(document.createElement("a"), { href: url, download: nombre });
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 10_000);
}
const base = (nombre) => (nombre || "montaje").replace(/\.pdf$/i, "");
// Deja pintar la interfaz antes de un cálculo pesado.
const respirar = () => new Promise((r) => requestAnimationFrame(() => setTimeout(r, 0)));

// ───────────── Formas (íconos de producto, como en dormi) ─────────────
const formas = {
  estrella: (c) => `<svg class="forma" viewBox="0 0 56 56" aria-hidden="true"><path fill="${c}" d="M28 2l6.5 14.3L50 12l-4.3 15.5L56 28l-10.3 0.5L50 44l-15.5-4.3L28 54l-6.5-14.3L6 44l4.3-15.5L0 28l10.3-.5L6 12l15.5 4.3z"/></svg>`,
  flor: (c) => `<svg class="forma" viewBox="0 0 56 56" aria-hidden="true"><path fill="${c}" d="M28 4c5 0 8 4 8 9 4-3 9-3 12 1s1 9-3 11c4 2 6 7 3 11s-8 4-12 1c0 5-3 9-8 9s-8-4-8-9c-4 3-9 3-12-1s-1-9 3-11C7 23 5 18 8 14s8-4 12-1c0-5 3-9 8-9z"/></svg>`,
  circulo: (c) => `<svg class="forma" viewBox="0 0 56 56" aria-hidden="true"><circle cx="28" cy="28" r="26" fill="${c}"/></svg>`,
  cuadro: (c) => `<svg class="forma" viewBox="0 0 56 56" aria-hidden="true"><rect x="3" y="3" width="50" height="50" rx="16" fill="${c}"/></svg>`,
  destello: (c) => `<svg class="forma" viewBox="0 0 56 56" aria-hidden="true"><path fill="${c}" d="M28 0c2 14 8 24 28 28-20 4-26 14-28 28-2-14-8-24-28-28C20 24 26 14 28 0z"/></svg>`,
};

// ───────────── Navegación ─────────────
const vistas = { inicio: vistaInicio, piezas: vistaPiezas, libro: vistaLibro, portada: vistaPortada, catalogos: vistaCatalogos };
function navegar() {
  const nombre = location.hash.slice(1) || "inicio";
  const vista = vistas[nombre] || vistaInicio;
  $$(".nav a").forEach((a) => a.classList.toggle("activa", a.dataset.vista === nombre));
  const main = $("#vista");
  main.innerHTML = "";
  vista(main);
  main.focus({ preventScroll: true });
  window.scrollTo({ top: 0 });
}

// ───────────── Inicio ─────────────
function vistaInicio(main) {
  const hora = new Date().getHours();
  const saludo = hora < 12 ? "Buenos días" : hora < 19 ? "Buenas tardes" : "Buenas noches";
  const productos = [
    ["piezas", formas.estrella("var(--naranja)"), "Volantes y tarjetas", "Cuántas caben por pliego, tiro y retiro, marcas y tira de color."],
    ["libro", formas.flor("var(--azul)"), "Revistas a caballete", "Firmas anidadas con creep calculado según el papel."],
    ["libro", formas.cuadro("var(--lavanda)"), "Libros al lomo o cosidos", "Firmas alzadas, fresado y marcas de alzado en escalera."],
    ["portada", formas.circulo("var(--lima)"), "Portadas", "Lomo automático, solapas, tapa dura y plantilla para el diseñador."],
  ];
  main.innerHTML = `
    <section class="hero">
      <span class="orbe" aria-hidden="true"></span>
      <p class="tenue">${saludo}</p>
      <h1>¿Qué vas a<br><span class="serif">imprimir</span> hoy?</h1>
      <p class="lead">Sube el PDF, elige la máquina y descarga el <span class="resaltado">PDF listo para imprimir</span>, con marcas, rebase y cálculos de la industria.</p>
      <div class="acciones">
        <a class="boton" href="#piezas">Montar piezas</a>
        <a class="boton boton-claro" href="#libro">Montar un libro</a>
      </div>
    </section>
    <section class="rejilla productos">
      ${productos.map(([v, forma, t, d]) => `
        <a class="tarjeta producto" href="#${v}">${forma}<h3>${t}</h3><p>${d}</p><span class="ir">Empezar →</span></a>`).join("")}
    </section>
    <section class="tarjeta estado-catalogo">
      <div class="cifras">
        <div class="cifra"><b class="num">${estado.maquinas.length}</b><span>máquinas</span></div>
        <div class="cifra"><b class="num">${estado.papeles.length}</b><span>papeles</span></div>
      </div>
      <div>
        <p style="max-width:440px">${estado.maquinas.length ? codigoTaller() ? "Tus máquinas y papeles están en la nube del taller y se comparten entre tus equipos." : "Tus máquinas y papeles están solo en este navegador. Actívalos en la nube del taller desde Catálogos para no perderlos." : "Empieza creando las máquinas del taller: tamaño de pliego, pinza y márgenes. Se guardan para siempre."}</p>
        <a class="boton ${estado.maquinas.length ? "boton-claro" : "boton-naranja"}" style="margin-top:14px" href="#catalogos">${estado.maquinas.length ? "Ver catálogos" : "Crear mi primera máquina"}</a>
      </div>
    </section>`;
}

// ───────────── Componentes compartidos ─────────────
function selectorMaquina(id) {
  if (!estado.maquinas.length) {
    return `<div class="error-caja">Aún no hay máquinas. <a href="#catalogos">Crea una en Catálogos</a> para continuar.</div>`;
  }
  const elegida = estado.preferencias.maquina;
  return `<label class="campo"><span>Máquina</span>
    <select id="${id}">${estado.maquinas.map((m) => `<option value="${esc(m.id)}" ${m.id === elegida ? "selected" : ""}>${esc(m.nombre)} · ${mm(m.pliego_max.ancho, 0)}×${mm(m.pliego_max.alto, 0)}</option>`).join("")}</select></label>`;
}
const maquinaElegida = (id) => estado.maquinas.find((m) => m.id === ($(`#${id}`)?.value ?? estado.preferencias.maquina)) || estado.maquinas[0];

function zonaArchivo(id, texto) {
  return `<label class="soltar" id="${id}-zona">
      <span class="orbe" aria-hidden="true"></span>
      <b>${texto}</b><span class="tenue">o arrástralo aquí</span>
      <input type="file" id="${id}" accept="application/pdf,.pdf">
    </label>`;
}

function conectarArchivo(id, alCargar, varios = false) {
  const zona = $(`#${id}-zona`);
  const input = $(`#${id}`);
  if (!zona) return;
  if (varios) input.multiple = true;
  const leer = async (lista) => {
    const archivos = [...lista].filter((a) => /pdf$/i.test(a.type) || /\.pdf$/i.test(a.name));
    if (!archivos.length) return;
    const leidos = await Promise.all(archivos.map(async (a) => ({ nombre: a.name, tamano: a.size, bytes: new Uint8Array(await a.arrayBuffer()) })));
    if (varios) alCargar(leidos); else alCargar(leidos[0]);
  };
  input.addEventListener("change", () => leer(input.files));
  zona.addEventListener("dragover", (e) => { e.preventDefault(); zona.classList.add("encima"); });
  zona.addEventListener("dragleave", () => zona.classList.remove("encima"));
  zona.addEventListener("drop", (e) => { e.preventDefault(); zona.classList.remove("encima"); leer(e.dataTransfer.files); });
}

function tarjetaArchivo(archivo, info, idQuitar) {
  const f = info?.formato;
  const rebase = info ? Math.min(...info.paginas.map((p) => p.rebase)) : 0;
  return `<div class="archivo"><span class="orbe" aria-hidden="true"></span>
    <div><b>${esc(archivo.nombre)}</b><span>${info ? `${info.paginas.length} pág. · ${f ? `${mm(f.ancho)}×${mm(f.alto)} mm` : "tamaños mixtos"} · rebase ${mm(rebase)} mm` : "Analizando…"}</span></div>
    <button class="boton boton-claro boton-chico" id="${idQuitar}" type="button">Cambiar</button></div>`;
}

function analizarArchivo(trabajo, archivo) {
  trabajo.archivo = archivo;
  trabajo.mini = {};
  trabajo.pdfjs = null;
  trabajo.error = null;
  trabajo.cara = 0;
  try {
    trabajo.info = JSON.parse(motor.analizar(archivo.bytes));
    if (!trabajo.info.formato) trabajo.error = "Las páginas del PDF tienen tamaños distintos. Revisa que todas tengan el mismo formato final (TrimBox).";
    revisarArchivo(trabajo);
  } catch (e) {
    trabajo.info = null;
    trabajo.error = `No se pudo leer el PDF: ${e.message || e}`;
  }
}

// Preflight: revisión del PDF con la versión PDF/X de la máquina elegida.
function revisarArchivo(trabajo, maquina) {
  trabajo.preflight = null;
  if (!trabajo.archivo) return;
  const m = maquina || estado.maquinas.find((x) => x.id === estado.preferencias.maquina) || estado.maquinas[0];
  try {
    trabajo.preflight = JSON.parse(motor.revisar_pdf(trabajo.archivo.bytes, JSON.stringify({ rebase: trabajo.op.rebase, pdfx: m?.salida?.pdfx || "PDF/X-4" })));
  } catch (e) {
    trabajo.preflight = { hallazgos: [], errores: 0, advertencias: 0, falla: String(e.message || e) };
  }
}

function bloquePreflight(trabajo) {
  const p = trabajo.preflight;
  if (!p) return "";
  if (p.falla) return `<div class="error-caja">No se pudo revisar el PDF: ${esc(p.falla)}</div>`;
  const listo = p.errores === 0;
  const resumen = listo && p.advertencias === 0
    ? `<span class="sello sello-ok">✓ Listo para imprimir</span>`
    : `<span class="sello ${listo ? "sello-aviso" : "sello-error"}">${p.errores ? `${p.errores} ${p.errores === 1 ? "error" : "errores"}` : ""}${p.errores && p.advertencias ? " · " : ""}${p.advertencias ? `${p.advertencias} ${p.advertencias === 1 ? "aviso" : "avisos"}` : ""}</span>`;
  const corr = correcciones();
  const items = p.hallazgos.map((h, i) => {
    const arreglo = h.corregible ? (corr[h.corregible] ? `<em class="se-corrige">se corrige al generar</em>` : `<em class="puede-corregirse">tiene corrección automática</em>`) : "";
    return `<li class="revision-${h.nivel}"><span>${esc(h.mensaje)}</span><small>${h.paginas.length === trabajo.info?.paginas.length && h.paginas.length > 1 ? "todas las páginas" : `pág. ${esc(p.rangos[i])}`} ${arreglo}</small></li>`;
  }).join("");
  return `<div class="revision">
    <div class="revision-cabeza"><b>Revisión del PDF</b>${resumen}</div>
    ${items ? `<details ${p.errores ? "open" : ""}><summary>${p.hallazgos.length} ${p.hallazgos.length === 1 ? "punto" : "puntos"} revisados</summary><ul class="revision-lista">${items}</ul></details>` : ""}
  </div>`;
}

function chips(nombre, opciones, valor) {
  return `<div class="chips" role="group" data-chips="${nombre}">${opciones.map(([v, t]) => `<button type="button" class="chip" data-valor="${esc(v)}" aria-pressed="${String(v) === String(valor)}">${t}</button>`).join("")}</div>`;
}
function conectarChips(raiz, alCambiar) {
  $$("[data-chips]", raiz).forEach((g) => g.addEventListener("click", (e) => {
    const b = e.target.closest(".chip");
    if (!b) return;
    $$(".chip", g).forEach((c) => c.setAttribute("aria-pressed", String(c === b)));
    alCambiar(g.dataset.chips, b.dataset.valor);
  }));
}

function interruptor(id, texto, valor) {
  return `<label class="interruptor"><span>${texto}</span><input type="checkbox" id="${id}" ${valor ? "checked" : ""}></label>`;
}

// Solo reemplaza el contenido si cambió: así un clic en un botón no se pierde
// cuando el campo que se estaba editando pierde el foco y recalcula.
function pintar(caja, html) {
  if (caja._html === html) return false;
  caja.innerHTML = html;
  caja._html = html;
  return true;
}

function listaAvisos(avisos) {
  const unicos = [...new Set(avisos || [])];
  return unicos.length ? `<ul class="avisos">${unicos.map((a) => `<li>${esc(a)}</li>`).join("")}</ul>` : "";
}

// ───────────── Miniaturas reales (pdf.js) ─────────────
// pdf.js solo dibuja las miniaturas de la vista previa; el PDF de salida lo
// escribe siempre el motor, sin rasterizar.
const PDFJS = "https://cdnjs.cloudflare.com/ajax/libs/pdf.js/4.10.38";
let pdfjsLib = null;
async function cargarPdfjs() {
  if (!pdfjsLib) {
    pdfjsLib = await import(`${PDFJS}/pdf.min.mjs`);
    pdfjsLib.GlobalWorkerOptions.workerSrc = `${PDFJS}/pdf.worker.min.mjs`;
  }
  return pdfjsLib;
}

async function pedirMiniaturas(trabajo, indices, redibujar) {
  if (!trabajo.archivo) return;
  trabajo.mini ??= {};
  const faltan = [...new Set(indices)].filter((i) => !(i in trabajo.mini));
  if (!faltan.length) return;
  for (const i of faltan) trabajo.mini[i] = null; // pedida
  const archivo = trabajo.archivo;
  try {
    const lib = await cargarPdfjs();
    trabajo.pdfjs ??= lib.getDocument({ data: archivo.bytes.slice() }).promise;
    const doc = await trabajo.pdfjs;
    for (const i of faltan) {
      if (trabajo.archivo !== archivo) return;
      const pagina = await doc.getPage(i + 1);
      const base = pagina.getViewport({ scale: 1, rotation: 0 });
      const vp = pagina.getViewport({ scale: Math.min(2.5, 700 / Math.max(base.width, base.height)), rotation: 0 });
      const lienzo = Object.assign(document.createElement("canvas"), { width: Math.ceil(vp.width), height: Math.ceil(vp.height) });
      const ctx = lienzo.getContext("2d");
      ctx.fillStyle = "#fff";
      ctx.fillRect(0, 0, lienzo.width, lienzo.height);
      await pagina.render({ canvasContext: ctx, viewport: vp }).promise;
      trabajo.mini[i] = lienzo.toDataURL("image/jpeg", 0.82);
    }
  } catch (e) {
    console.warn("Miniaturas no disponibles:", e);
    for (const i of faltan) trabajo.mini[i] = false;
  }
  if (trabajo.archivo === archivo) redibujar();
}

/** a × b: primero a, luego b (convención PDF). */
const multiplicar = (a, b) => [
  a[0] * b[0] + a[1] * b[2], a[0] * b[1] + a[1] * b[3],
  a[2] * b[0] + a[3] * b[2], a[2] * b[1] + a[3] * b[3],
  a[4] * b[0] + a[5] * b[2] + b[4], a[4] * b[1] + a[5] * b[3] + b[5],
];
/** La misma matriz que usa el motor (pdf::matriz_colocacion), en mm. */
function matrizColocacion([x0, y0, x1, y1], giro, tx, ty) {
  switch (giro % 360) {
    case 90: return [0, -1, 1, 0, tx - y0, ty + x1];
    case 180: return [-1, 0, 0, -1, tx + x1, ty + y1];
    case 270: return [0, 1, -1, 0, tx + y1, ty - x0];
    default: return [1, 0, 0, 1, tx - x0, ty - y0];
  }
}

// ───────────── Vista previa del pliego ─────────────
const COLORES = ["#4c9ef3", "#e4ea5b", "#ff6b2c", "#8b8cf0"];

function svgCara(cara, maquina, opciones = {}) {
  const W = cara.pliego.ancho, H = cara.pliego.alto;
  const y = (v, h = 0) => H - v - h; // PDF (abajo-izquierda) → SVG (arriba-izquierda)
  const r = (rc, extra = "") => `<rect x="${rc.x}" y="${y(rc.y, rc.alto)}" width="${rc.ancho}" height="${rc.alto}" ${extra}/>`;
  const partes = [];
  partes.push(`<rect x="0" y="0" width="${W}" height="${H}" fill="var(--superficie)" stroke="var(--linea)" stroke-width="${W / 400}"/>`);
  if (maquina && !cara.cajas) {
    const a = { x: maquina.lateral, y: maquina.pinza, ancho: W - 2 * maquina.lateral, alto: H - maquina.pinza - maquina.cola };
    partes.push(r(a, `fill="none" stroke="var(--gris)" stroke-width="${W / 700}" stroke-dasharray="${W / 120} ${W / 160}"`));
    partes.push(`<rect x="0" y="${H - maquina.pinza}" width="${W}" height="${maquina.pinza}" fill="var(--naranja)" opacity=".12"/>`);
    partes.push(`<text x="${W / 2}" y="${H - maquina.pinza / 2}" font-size="${Math.max(Math.min(maquina.pinza * 0.7, W / 45), 3)}" text-anchor="middle" dominant-baseline="middle" fill="var(--naranja)" font-weight="700" letter-spacing=".1em">PINZA</text>`);
  }
  if (cara.cajas) partes.push(r(cara.cajas.sangrado, `fill="none" stroke="#d93b2b" stroke-width="${W / 900}"`));
  const { info, mini } = opciones;
  cara.ubicaciones.forEach((u, k) => {
    const color = COLORES[u.pagina % COLORES.length];
    const imagen = mini?.[u.pagina];
    const pag = info?.paginas[u.pagina];
    if (imagen && pag) {
      // Página real: CropBox dibujada por pdf.js, llevada al pliego con la
      // matriz del motor y recortada al rebase permitido.
      const [vx0, vy0, vx1, vy1] = pag.vista;
      const m = matrizColocacion(pag.corte, (pag.giro + u.giro) % 360, u.corte.x, u.corte.y);
      const t = multiplicar(multiplicar([1, 0, 0, -1, vx0, vy1], m), [1, 0, 0, -1, 0, H]);
      const id = `rc-${opciones.clave || "c"}-${k}`;
      partes.push(`<clipPath id="${id}">${r(u.recorte)}</clipPath><g clip-path="url(#${id})"><image href="${imagen}" x="0" y="0" width="${vx1 - vx0}" height="${vy1 - vy0}" preserveAspectRatio="none" transform="matrix(${t.map((v) => +v.toFixed(4)).join(" ")})"/></g>`);
      partes.push(r(u.corte, `fill="none" stroke="var(--tinta)" stroke-width="${W / 1100}" stroke-opacity=".5"`));
      if (opciones.numeros !== false && opciones.insignias) {
        const tam = Math.min(u.corte.ancho, u.corte.alto) * 0.16;
        const cx = u.corte.x + tam * 0.8, cy = y(u.corte.y + u.corte.alto) + tam * 0.8;
        partes.push(`<circle cx="${cx}" cy="${cy}" r="${tam * 0.62}" fill="var(--oscuro)"/><text x="${cx}" y="${cy}" font-size="${tam * 0.62}" font-weight="700" text-anchor="middle" dominant-baseline="central" fill="#fff">${u.pagina + 1}</text>`);
      }
      return;
    }
    partes.push(r(u.recorte, `fill="${color}" opacity=".28"`));
    partes.push(r(u.corte, `fill="${color}" fill-opacity=".55" stroke="var(--tinta)" stroke-width="${W / 900}"`));
    if (opciones.numeros !== false) {
      const cx = u.corte.x + u.corte.ancho / 2, cy = y(u.corte.y + u.corte.alto / 2);
      const t = Math.min(u.corte.ancho, u.corte.alto) * 0.42;
      // Número de página girado como va impreso; la barra marca la cabeza.
      partes.push(`<g transform="rotate(${u.giro} ${cx} ${cy})"><text x="${cx}" y="${cy}" font-size="${t}" font-weight="800" text-anchor="middle" dominant-baseline="central" fill="var(--tinta)">${u.pagina + 1}</text>
        <rect x="${cx - t * 0.5}" y="${cy - t * 0.95}" width="${t}" height="${t * 0.09}" rx="${t * 0.04}" fill="var(--tinta)"/></g>`);
    }
  });
  const m = cara.marcas;
  const trazo = W / 1000;
  for (const l of m.corte) partes.push(`<line x1="${l.x1}" y1="${y(l.y1)}" x2="${l.x2}" y2="${y(l.y2)}" stroke="var(--tinta)" stroke-width="${trazo * 1.2}"/>`);
  for (const l of m.pliegues || []) partes.push(`<line x1="${l.x1}" y1="${y(l.y1)}" x2="${l.x2}" y2="${y(l.y2)}" stroke="#c03ac0" stroke-width="${trazo * 1.5}" stroke-dasharray="${W / 300}"/>`);
  for (const g of m.registro) partes.push(`<g stroke="var(--tinta)" stroke-width="${trazo}" fill="none"><circle cx="${g.x}" cy="${y(g.y)}" r="${g.radio * 0.6}"/><line x1="${g.x - g.radio}" y1="${y(g.y)}" x2="${g.x + g.radio}" y2="${y(g.y)}"/><line x1="${g.x}" y1="${y(g.y) - g.radio}" x2="${g.x}" y2="${y(g.y) + g.radio}"/></g>`);
  for (const p of m.tira_color) {
    const [c, mg, a, k] = p.cmyk;
    const rgb = [(1 - c) * (1 - k), (1 - mg) * (1 - k), (1 - a) * (1 - k)].map((v) => Math.round(v * 255));
    partes.push(r(p.rect, `fill="rgb(${rgb})"`));
  }
  for (const a of m.alzado || []) partes.push(r(a, `fill="var(--tinta)"`));
  return `<svg viewBox="${-W * 0.01} ${-H * 0.01} ${W * 1.02} ${H * 1.02}" role="img" aria-label="${esc(cara.nombre)}: pliego de ${mm(W, 0)} por ${mm(H, 0)} mm">${partes.join("")}</svg>`;
}

function tarjetaVistaPrevia(trabajo, maquina, titulo, insignias = false) {
  const caras = trabajo.plan?.caras || trabajo.plan?.plan?.caras;
  if (!caras?.length) {
    return `<section class="tarjeta vista-previa"><div class="lienzo"><div class="vacio"><span class="orbe orbe-respira" aria-hidden="true"></span><p>${titulo}</p></div></div></section>`;
  }
  const i = Math.min(trabajo.cara, caras.length - 1);
  const muchas = caras.length > 14;
  return `<section class="tarjeta vista-previa">
    <div class="vista-previa-cabeza">
      <h3>${esc(caras[i].nombre)} <span class="tenue num">· ${i + 1} de ${caras.length}</span></h3>
      <div class="paginador">
        <button class="boton boton-claro boton-chico" data-cara="${i - 1}" ${i === 0 ? "disabled" : ""} aria-label="Pliego anterior">←</button>
        ${muchas ? "" : caras.map((_, k) => `<button class="chip" data-cara="${k}" aria-pressed="${k === i}">${k + 1}</button>`).join("")}
        <button class="boton boton-claro boton-chico" data-cara="${i + 1}" ${i === caras.length - 1 ? "disabled" : ""} aria-label="Pliego siguiente">→</button>
      </div>
    </div>
    <div class="lienzo">${svgCara(caras[i], maquina, { info: trabajo.info, mini: trabajo.mini, insignias, clave: i })}</div>
    <div class="leyenda"><span><i style="border-color:var(--tinta)"></i>Corte</span><span><i style="border-color:#c03ac0;border-top-style:dashed"></i>Pliegue</span><span><i style="border-color:var(--gris);border-top-style:dashed"></i>Área imprimible</span><span><i style="border-color:var(--naranja)"></i>Pinza</span></div>
  </section>`;
}

function conectarPaginador(raiz, trabajo, redibujar) {
  $$("[data-cara]", raiz).forEach((b) => b.addEventListener("click", () => { trabajo.cara = Number(b.dataset.cara); redibujar(); }));
}

// ───────────── Piezas sueltas (n-up) ─────────────
function vistaPiezas(main) {
  const t = estado.piezas;
  const o = t.op;
  main.innerHTML = `
    <div class="encabezado"><div><h1>Volantes y <span class="serif">tarjetas</span></h1><p>Repite una pieza en el pliego con el mejor aprovechamiento, o combina varios diseños y clientes en el mismo pliego según la cantidad de cada uno.</p></div></div>
    <div class="trabajo">
      <div class="panel">
        <section class="tarjeta paso ${t.info ? "listo" : ""}">
          <div class="paso-titulo"><span class="paso-num">1</span><h3>Archivo</h3></div>
          ${chips("modo", [["repetir", "Un diseño por pliego"], ["combinar", "Combinar varios"]], o.modo)}
          ${o.modo === "combinar" ? bloqueArchivosCombinado(t) : t.archivo ? tarjetaArchivo(t.archivo, t.info, "pz-cambiar") : zonaArchivo("pz-archivo", "Sube el PDF de la pieza")}
          ${bloquePreflight(t)}
        </section>
        ${o.modo === "combinar" && t.info?.formato ? bloqueCantidades(t) : ""}
        <section class="tarjeta paso">
          <div class="paso-titulo"><span class="paso-num">2</span><h3>Montaje</h3></div>
          ${selectorMaquina("pz-maquina")}
          <div class="fila">
            <label class="campo"><span>Rebase (mm)</span><input type="number" step="0.5" min="0" id="pz-rebase" value="${o.rebase}"></label>
            <label class="campo"><span>Calle (mm)</span><input type="number" step="0.5" min="0" id="pz-calle" value="${o.calle}"><small>0 = corte compartido</small></label>
          </div>
          <div class="campo"><span>Orientación</span>${chips("orientacion", [["auto", "Automática"], ["normal", "Normal"], ["girada", "Girada 90°"]], o.orientacion)}</div>
          ${interruptor("pz-dorso", "Frente y dorso (páginas en pares)", o.dorso)}
          ${o.dorso ? `<div class="campo"><span>Volteo del pliego</span>${chips("volteo", [["lateral", "Tira y retira (lateral)"], ["cabeza", "De cabeza (tumble)"]], o.volteo)}</div>` : ""}
          <details class="avanzado"><summary>Marcas y pliego</summary>
            <div class="paso">
              ${interruptor("pz-marcas", "Marcas de corte y registro", o.marcas)}
              ${interruptor("pz-tira", "Tira de control de color", o.tira)}
              <div class="fila">
                <label class="campo"><span>Pliego ancho</span><input type="number" id="pz-pliego-ancho" placeholder="máx." value="${o.pliegoAncho ?? ""}"></label>
                <label class="campo"><span>Pliego alto</span><input type="number" id="pz-pliego-alto" placeholder="máx." value="${o.pliegoAlto ?? ""}"></label>
              </div>
            </div>
          </details>
          ${bloqueCorrecciones("pz")}
        </section>
      </div>
      <div class="resultado" id="pz-resultado"></div>
    </div>`;

  if (o.modo === "combinar") {
    conectarArchivo("pz-archivo", (lista) => { agregarArchivosCombinado(t, lista); vistaPiezas(main); }, true);
    $$("[data-quitar]", main).forEach((b) => b.addEventListener("click", () => { t.archivos.splice(Number(b.dataset.quitar), 1); agregarArchivosCombinado(t, []); vistaPiezas(main); }));
    $$("[data-cantidad]", main).forEach((el) => el.addEventListener("change", (e) => {
      e.stopPropagation();
      o.cantidades[Number(el.dataset.cantidad)] = Math.max(1, Math.round(num(el.value, 1)));
      calcularPiezas();
    }));
  } else {
    conectarArchivo("pz-archivo", (a) => { analizarArchivo(t, a); vistaPiezas(main); });
  }
  $("#pz-cambiar")?.addEventListener("click", () => { t.archivo = null; t.info = null; t.plan = null; t.error = null; vistaPiezas(main); });
  const leerOpciones = () => {
    o.rebase = num($("#pz-rebase").value, 3);
    o.calle = num($("#pz-calle").value, 0);
    o.dorso = $("#pz-dorso").checked;
    o.marcas = $("#pz-marcas").checked;
    o.tira = $("#pz-tira").checked;
    o.pliegoAncho = $("#pz-pliego-ancho").value || null;
    o.pliegoAlto = $("#pz-pliego-alto").value || null;
    if ($("#pz-maquina")) preferir("maquina", $("#pz-maquina").value);
  };
  conectarCorrecciones(main, () => vistaPiezas(main));
  $$("input, select", main).forEach((el) => el.addEventListener("change", () => {
    if (el.type === "file" || el.dataset.correccion || el.dataset.cantidad) return;
    const antes = { dorso: o.dorso, rebase: o.rebase, maquina: estado.preferencias.maquina };
    leerOpciones();
    if (antes.rebase !== o.rebase || antes.maquina !== estado.preferencias.maquina) { revisarArchivo(t); vistaPiezas(main); return; }
    if (antes.dorso !== o.dorso) vistaPiezas(main); else calcularPiezas();
  }));
  conectarChips(main, (n, v) => {
    o[n] = v;
    if (n === "modo") {
      // Cada modo trabaja con sus propios archivos.
      t.archivo = null; t.info = null; t.plan = null; t.error = null; t.preflight = null; t.archivos = [];
      vistaPiezas(main);
    } else calcularPiezas();
  });
  calcularPiezas();
}

// ── Combinado: varios PDF (uno por cliente) y cantidades por diseño ──
function bloqueArchivosCombinado(t) {
  const lista = (t.archivos || []).map((a, i) => `<div class="archivo"><span class="orbe" aria-hidden="true"></span>
    <div><b>${esc(a.nombre)}</b><span>${a.paginas ?? "?"} pág.</span></div>
    <button class="boton boton-claro boton-chico" type="button" data-quitar="${i}" aria-label="Quitar ${esc(a.nombre)}">Quitar</button></div>`).join("");
  return `${lista}${zonaArchivo("pz-archivo", t.archivos?.length ? "Agregar otro PDF" : "Sube los PDF de los diseños (uno o varios)")}`;
}

function agregarArchivosCombinado(t, nuevos) {
  t.archivos = [...(t.archivos || []), ...nuevos];
  for (const a of t.archivos) {
    if (a.paginas == null) {
      try { a.paginas = JSON.parse(motor.analizar(a.bytes)).paginas.length; } catch { a.paginas = 0; }
    }
  }
  t.archivos = t.archivos.filter((a) => a.paginas > 0);
  if (!t.archivos.length) { t.archivo = null; t.info = null; t.plan = null; return; }
  try {
    const bytes = t.archivos.length === 1 ? t.archivos[0].bytes : motor.unir_pdfs(t.archivos.map((a) => a.bytes));
    analizarArchivo(t, { nombre: t.archivos.length === 1 ? t.archivos[0].nombre : `combinado-${t.archivos.length}-archivos.pdf`, bytes });
  } catch (e) {
    t.error = `No se pudieron juntar los PDF: ${e.message || e}`;
  }
}

/** Diseños del combinado: cada página, o cada par frente/dorso. */
function disenosCombinado(t) {
  const etiquetas = [];
  for (const a of t.archivos || []) for (let p = 1; p <= a.paginas; p++) etiquetas.push(`${a.nombre.replace(/\.pdf$/i, "")} · pág. ${p}`);
  const paso = t.op.dorso ? 2 : 1;
  const disenos = [];
  for (let i = 0; i + paso - 1 < etiquetas.length; i += paso) {
    const n = disenos.length;
    disenos.push({ etiqueta: etiquetas[i] + (paso === 2 ? " + dorso" : ""), frente: i, dorso: paso === 2 ? i + 1 : null, cantidad: t.op.cantidades[n] ?? 1000 });
  }
  return disenos;
}

function bloqueCantidades(t) {
  const disenos = disenosCombinado(t);
  return `<section class="tarjeta paso">
    <div class="paso-titulo"><span class="paso-num">✦</span><h3>Cantidades</h3></div>
    <p class="tenue" style="font-size:14px">Ejemplares de cada diseño. El pliego se reparte para imprimirlos todos con el menor número de pliegos.</p>
    <div class="cantidades">${disenos.map((x, i) => `<label class="cantidad"><span>${esc(x.etiqueta)}</span><input type="number" min="1" step="50" data-cantidad="${i}" value="${x.cantidad}"></label>`).join("")}</div>
  </section>`;
}

function peticionPiezas(maquina, info, orientacion) {
  const o = estado.piezas.op;
  const pliego = o.pliegoAncho && o.pliegoAlto ? { ancho: num(o.pliegoAncho), alto: num(o.pliegoAlto) } : null;
  return {
    maquina, formato: info.formato, paginas: info.paginas.length, pliego,
    rebase: o.rebase, calle: o.calle, orientacion: orientacion || o.orientacion,
    dorso: o.dorso, volteo: o.volteo, marcas: o.marcas, tira_color: o.tira,
    titulo: base(estado.piezas.archivo?.nombre), fecha: ahora(), correcciones: correcciones(),
  };
}

function peticionCombinado(maquina, info) {
  return { ...peticionPiezas(maquina, info), disenos: disenosCombinado(estado.piezas).map(({ frente, dorso, cantidad }) => ({ frente, dorso, cantidad })) };
}

function calcularPiezas() {
  const t = estado.piezas;
  const caja = $("#pz-resultado");
  if (!caja) return;
  const maquina = maquinaElegida("pz-maquina");
  t.plan = null;
  let explicacion = "";
  let errorPlan = "";
  const combinado = t.op.modo === "combinar";
  if (combinado && t.info?.formato && maquina && !t.error) {
    try {
      t.plan = JSON.parse(motor.planear_combinado(JSON.stringify(peticionCombinado(maquina, t.info))));
      const disenos = disenosCombinado(t);
      explicacion = disenos.map((x, i) => `${esc(x.etiqueta)}: <span class="resaltado">${t.plan.plan.posiciones[i]} posiciones</span> → ${t.plan.plan.impresos[i].toLocaleString("es-CO")} (sobran ${(t.plan.plan.impresos[i] - x.cantidad).toLocaleString("es-CO")})`).join("<br>");
    } catch (e) { errorPlan = String(e.message || e); }
  } else if (t.info?.formato && maquina && !t.error) {
    try {
      t.plan = JSON.parse(motor.planear_nup(JSON.stringify(peticionPiezas(maquina, t.info))));
      const d = t.plan.distribucion;
      if (t.op.orientacion === "auto" && d.girada) {
        try {
          const normal = JSON.parse(motor.planear_nup(JSON.stringify(peticionPiezas(maquina, t.info, "normal"))));
          explicacion = `Giré la pieza <span class="resaltado">90°</span> porque así caben <b>${d.columnas * d.filas}</b> en vez de ${normal.distribucion.columnas * normal.distribucion.filas}.`;
        } catch { explicacion = `Giré la pieza <span class="resaltado">90°</span>: sin girar no cabe ninguna.`; }
      } else {
        explicacion = `Van <span class="resaltado">${d.columnas} columnas × ${d.filas} filas</span>${t.op.calle > 0 ? `, con ${mm(t.op.calle)} mm de calle` : ", con corte compartido"}.`;
      }
    } catch (e) { errorPlan = String(e.message || e); }
  }
  const d = t.plan?.distribucion;
  const metricas = combinado && t.plan
    ? `<div class="metrica"><b>${t.plan.plan.pliegos.toLocaleString("es-CO")}</b><span>pliegos a imprimir</span></div>
        <div class="metrica"><b>${d.columnas * d.filas}</b><span>posiciones por pliego</span></div>
        <div class="metrica"><b>${t.plan.plan.posiciones.length}</b><span>diseños combinados</span></div>`
    : d ? `<div class="metrica"><b>${d.columnas * d.filas}</b><span>piezas por pliego</span></div>
        <div class="metrica"><b>${mm(d.aprovechamiento, 0)}%</b><span>aprovechamiento</span></div>
        <div class="metrica"><b>${t.plan.caras.length}</b><span>${t.plan.caras.length === 1 ? "pliego" : "pliegos"} en el PDF</span></div>` : "";
  const html = `
    ${t.error || errorPlan ? `<div class="error-caja">${esc(t.error || errorPlan)}</div>` : ""}
    ${d ? `<section class="tarjeta-oscura">
      <div class="metricas">
        ${metricas}
      </div>
      <p class="explicacion" style="margin-top:20px">${explicacion}</p>
      <div class="acciones-resultado" style="margin-top:20px"><button class="boton boton-blanco" id="pz-generar" type="button">Descargar PDF listo para imprimir</button></div>
    </section>` : ""}
    ${listaAvisos(t.plan?.avisos)}
    ${tarjetaVistaPrevia(t, maquina, t.archivo ? "Ajusta las opciones para ver el pliego" : "Sube un PDF para ver el montaje")}`;
  const carasPz = t.plan?.caras ?? t.plan?.plan?.caras;
  if (carasPz?.length) pedirMiniaturas(t, carasPz[Math.min(t.cara, carasPz.length - 1)].ubicaciones.map((u) => u.pagina), calcularPiezas);
  if (!pintar(caja, html)) return;
  conectarPaginador(caja, t, calcularPiezas);
  $("#pz-generar")?.addEventListener("click", async (e) => {
    const b = e.currentTarget;
    b.disabled = true; b.textContent = "Generando…";
    await respirar();
    try {
      const icc = (await iccDB.leer(maquina.id)) || new Uint8Array();
      const r = combinado
        ? motor.generar_combinado(t.archivo.bytes, JSON.stringify(peticionCombinado(maquina, t.info)), icc)
        : motor.generar_nup(t.archivo.bytes, JSON.stringify(peticionPiezas(maquina, t.info)), icc);
      descargar(r.pdf, `${base(t.archivo.nombre)}-${combinado ? "combinado" : "montaje"}.pdf`);
      const inf = JSON.parse(r.informe);
      const corregidos = inf.avisos.filter((a) => a.startsWith("corregido")).length;
      avisar(`${inf.pdfx ? "PDF listo (con perfil de salida)" : "PDF listo"}${corregidos ? ` · ${corregidos} correcciones aplicadas` : ""}`);
    } catch (err) { avisar(`Error: ${err.message || err}`); }
    b.disabled = false; b.textContent = "Descargar PDF listo para imprimir";
  });
}

// ───────────── Libros y revistas ─────────────
function vistaLibro(main) {
  const t = estado.libro;
  const o = t.op;
  const papeles = estado.papeles;
  const papelElegido = estado.preferencias.papelTripa;
  main.innerHTML = `
    <div class="encabezado"><div><h1>Libros y <span class="serif">revistas</span></h1><p>Sube el interior página por página. Se reparte en firmas, se pliega y se calcula el creep o el fresado.</p></div></div>
    <div class="trabajo">
      <div class="panel">
        <section class="tarjeta paso ${t.info ? "listo" : ""}">
          <div class="paso-titulo"><span class="paso-num">1</span><h3>Tripa (interior)</h3></div>
          ${t.archivo ? tarjetaArchivo(t.archivo, t.info, "lb-cambiar") : zonaArchivo("lb-archivo", "Sube el PDF del interior")}
          ${bloquePreflight(t)}
        </section>
        <section class="tarjeta paso">
          <div class="paso-titulo"><span class="paso-num">2</span><h3>Encuadernación</h3></div>
          ${chips("encuadernacion", [["caballete", "Caballete"], ["lomo", "Al lomo (PUR)"], ["cosido", "Cosido"]], o.encuadernacion)}
          ${selectorMaquina("lb-maquina")}
          <label class="campo"><span>Papel de la tripa</span>
            <select id="lb-papel"><option value="">Sin papel (no calcula creep ni lomo)</option>${papeles.map((p) => `<option value="${esc(p.id)}" ${p.id === papelElegido ? "selected" : ""}>${esc(p.nombre)} · ${p.calibre_um} µm</option>`).join("")}</select>
            ${papeles.length ? "" : `<small><a href="#catalogos">Agrega papeles</a> para calcular el lomo y el creep.</small>`}
          </label>
          <div class="campo"><span>Páginas por firma</span>${chips("firma", [["auto", "Auto"], ["4", "4"], ["8", "8"], ["16", "16"], ["32", "32"], ["64", "64"]], o.firma)}</div>
          <div class="campo"><span>Firmas por pliego</span>${chips("aprovechamiento", [["auto", "Auto"], ["una", "Una"], ["repetir", "Repetir"], ["tira_retira", "Tira y retira"]], o.aprovechamiento)}<small>Auto monta en tira y retira (una sola plancha para las dos caras) cuando la firma cabe dos veces lado a lado.</small></div>
          <details class="avanzado"><summary>Márgenes, lectura y marcas</summary>
            <div class="paso">
              <div class="fila-3">
                <label class="campo"><span>Rebase</span><input type="number" step="0.5" min="0" id="lb-rebase" value="${o.rebase}"></label>
                <label class="campo"><span>Refile</span><input type="number" step="0.5" min="0" id="lb-refile" value="${o.refile}"></label>
                <label class="campo"><span>Fresado</span><input type="number" step="0.5" min="0" id="lb-fresado" value="${o.fresado}" ${o.encuadernacion !== "lomo" ? "disabled" : ""}></label>
              </div>
              ${interruptor("lb-creep", "Compensar creep (caballete)", o.creep)}
              ${interruptor("lb-rtl", "Lectura de derecha a izquierda", o.rtl)}
              ${interruptor("lb-marcas", "Marcas de corte, plegado y registro", o.marcas)}
              ${interruptor("lb-tira", "Tira de control de color", o.tira)}
            </div>
          </details>
          ${bloqueCorrecciones("lb")}
        </section>
      </div>
      <div class="resultado" id="lb-resultado"></div>
    </div>`;
  conectarArchivo("lb-archivo", (a) => { analizarArchivo(t, a); vistaLibro(main); });
  $("#lb-cambiar")?.addEventListener("click", () => { t.archivo = null; t.info = null; t.plan = null; t.error = null; vistaLibro(main); });
  conectarCorrecciones(main, () => vistaLibro(main));
  $$("input, select", main).forEach((el) => el.addEventListener("change", () => {
    if (el.type === "file" || el.dataset.correccion) return;
    const antes = { rebase: o.rebase, maquina: estado.preferencias.maquina };
    o.rebase = num($("#lb-rebase").value, 3);
    o.refile = num($("#lb-refile").value, 3);
    o.fresado = num($("#lb-fresado").value, 3);
    o.creep = $("#lb-creep").checked;
    o.rtl = $("#lb-rtl").checked;
    o.marcas = $("#lb-marcas").checked;
    o.tira = $("#lb-tira").checked;
    if ($("#lb-maquina")) preferir("maquina", $("#lb-maquina").value);
    preferir("papelTripa", $("#lb-papel").value);
    if (antes.rebase !== o.rebase || antes.maquina !== estado.preferencias.maquina) { revisarArchivo(t); vistaLibro(main); return; }
    calcularLibro();
  }));
  conectarChips(main, (n, v) => { o[n] = v; t.cara = 0; if (n === "encuadernacion") vistaLibro(main); else calcularLibro(); });
  calcularLibro();
}

function peticionLibro(maquina, info) {
  const o = estado.libro.op;
  const papel = estado.papeles.find((p) => p.id === $("#lb-papel")?.value);
  return {
    maquina, formato: info.formato, paginas: info.paginas.length, encuadernacion: o.encuadernacion,
    firma: o.firma === "auto" ? null : Number(o.firma), rebase: o.rebase, fresado: o.fresado, refile: o.refile,
    calibre_um: papel && (o.creep || o.encuadernacion !== "caballete") ? papel.calibre_um : null,
    derecha_a_izquierda: o.rtl, marcas: o.marcas, tira_color: o.tira, aprovechamiento: o.aprovechamiento || "auto",
    titulo: base(estado.libro.archivo?.nombre), fecha: ahora(), correcciones: correcciones(),
  };
}

function calcularLibro() {
  const t = estado.libro;
  const caja = $("#lb-resultado");
  if (!caja) return;
  const maquina = maquinaElegida("lb-maquina");
  t.plan = null;
  if (t.info?.formato && maquina && !t.error) {
    try { t.plan = JSON.parse(motor.planear_libro(JSON.stringify(peticionLibro(maquina, t.info)))); }
    catch (e) { t.plan = null; caja.dataset.error = String(e.message || e); }
  }
  const error = t.error || (!t.plan && caja.dataset.error) || "";
  delete caja.dataset.error;
  const p = t.plan?.plan;
  let composicion = "", explicacion = "";
  if (p) {
    const grupos = [];
    for (const f of p.firmas) {
      const g = grupos.at(-1);
      if (g && g.n === f.paginas && g.girada === f.girada) g.c++; else grupos.push({ n: f.paginas, girada: f.girada, c: 1 });
    }
    composicion = grupos.map((g) => `${g.c} × ${g.n} pp`).join(" + ");
    const girada = p.firmas.some((f) => f.girada);
    const o = t.op;
    explicacion = `${p.paginas_libro} páginas en <span class="resaltado">${composicion}</span>${girada ? ", con la firma girada 90° para que quepa en el pliego" : ""}. `;
    explicacion += o.encuadernacion === "caballete" ? "Las firmas van anidadas una dentro de otra." : "Las firmas se alzan una tras otra" + (o.encuadernacion === "lomo" ? `, con ${mm(o.fresado)} mm de fresado en el lomo.` : ".");
    if (p.blancas) explicacion += ` Se agregan <b>${p.blancas}</b> páginas en blanco al final.`;
    const multiples = p.firmas.filter((f) => f.copias > 1);
    if (multiples.length) {
      const tr = multiples.some((f) => f.tira_retira);
      explicacion += ` ${multiples.length === p.firmas.length ? "Cada pliego" : `En ${multiples.length} firmas, cada pliego`} da <span class="resaltado">${Math.max(...multiples.map((f) => f.copias))} firmas${tr ? " en tira y retira" : ""}</span>: ${p.juegos_planchas} juegos de planchas y ${mm(p.pliegos_por_ejemplar, 2)} pliegos por ejemplar.`;
    }
  }
  const tercera = !p ? "" : t.op.encuadernacion === "caballete"
    ? `<div class="metrica"><b>${mm(p.creep_max, 2)}</b><span>mm de creep (hoja central)</span></div>`
    : `<div class="metrica"><b>${t.plan.lomo != null ? mm(t.plan.lomo) : "—"}</b><span>mm de lomo del bloque</span></div>`;
  const html = `
    ${error ? `<div class="error-caja">${esc(error)}</div>` : ""}
    ${p ? `<section class="tarjeta-oscura">
      <div class="metricas">
        <div class="metrica"><b>${p.firmas.length}</b><span>${p.firmas.length === 1 ? "firma" : "firmas"}</span></div>
        <div class="metrica"><b>${p.caras.length}</b><span>pliegos (tiro y retiro)</span></div>
        ${tercera}
      </div>
      <p class="explicacion" style="margin-top:20px">${explicacion}</p>
      <div class="acciones-resultado" style="margin-top:20px">
        <button class="boton boton-blanco" id="lb-generar" type="button">Descargar pliegos</button>
        <a class="boton boton-claro" href="#portada" id="lb-a-portada">Hacer la portada →</a>
      </div>
    </section>` : ""}
    ${listaAvisos(t.plan?.avisos)}
    ${tarjetaVistaPrevia(t, maquina, t.archivo ? "Ajusta las opciones para ver las firmas" : "Sube el interior para ver las firmas", true)}`;
  const carasLb = t.plan?.plan?.caras;
  if (carasLb?.length) pedirMiniaturas(t, carasLb[Math.min(t.cara, carasLb.length - 1)].ubicaciones.map((u) => u.pagina), calcularLibro);
  if (!pintar(caja, html)) return;
  conectarPaginador(caja, t, calcularLibro);
  $("#lb-a-portada")?.addEventListener("click", () => {
    const po = estado.portada.op;
    po.ancho = t.info.formato.ancho; po.alto = t.info.formato.alto; po.paginas = p.paginas_libro;
    po.rtl = t.op.rtl; po.lomo = "";
    preferir("papelPortadaTripa", $("#lb-papel")?.value || "");
  });
  $("#lb-generar")?.addEventListener("click", async (e) => {
    const b = e.currentTarget;
    b.disabled = true; b.textContent = "Generando…";
    await respirar();
    try {
      const icc = (await iccDB.leer(maquina.id)) || new Uint8Array();
      const r = motor.generar_libro(t.archivo.bytes, JSON.stringify(peticionLibro(maquina, t.info)), icc);
      descargar(r.pdf, `${base(t.archivo.nombre)}-pliegos.pdf`);
      const corregidos = JSON.parse(r.informe).avisos.filter((a) => a.startsWith("corregido")).length;
      avisar(`Pliegos listos${corregidos ? ` · ${corregidos} correcciones aplicadas` : ""}`);
    } catch (err) { avisar(`Error: ${err.message || err}`); }
    b.disabled = false; b.textContent = "Descargar pliegos";
  });
}

// ───────────── Portada ─────────────
function vistaPortada(main) {
  const t = estado.portada;
  const o = t.op;
  const papeles = estado.papeles;
  const tripa = estado.preferencias.papelPortadaTripa ?? estado.preferencias.papelTripa;
  const cubierta = estado.preferencias.papelCubierta;
  const opcionesPapel = (sel, vacio) => `<option value="">${vacio}</option>${papeles.map((p) => `<option value="${esc(p.id)}" ${p.id === sel ? "selected" : ""}>${esc(p.nombre)} · ${p.calibre_um} µm</option>`).join("")}`;
  main.innerHTML = `
    <div class="encabezado"><div><h1>Portada con <span class="serif">lomo</span> exacto</h1><p>Calcula tapa, lomo, contratapa y solapas a partir de las páginas y el papel. Descarga la plantilla para el diseñador o arma la portada con sus páginas.</p></div></div>
    <div class="trabajo">
      <div class="panel">
        <section class="tarjeta paso">
          <div class="paso-titulo"><span class="paso-num">1</span><h3>El libro</h3></div>
          <div class="fila">
            <label class="campo"><span>Ancho (mm)</span><input type="number" id="po-ancho" value="${o.ancho}"></label>
            <label class="campo"><span>Alto (mm)</span><input type="number" id="po-alto" value="${o.alto}"></label>
          </div>
          <label class="campo"><span>Páginas de la tripa</span><input type="number" id="po-paginas" value="${o.paginas}"></label>
          <label class="campo"><span>Papel de la tripa</span><select id="po-papel">${opcionesPapel(tripa, "Elegir papel…")}</select></label>
          <label class="campo"><span>Lomo manual (mm)</span><input type="number" step="0.1" id="po-lomo" value="${o.lomo}" placeholder="se calcula solo"><small>Déjalo vacío para calcularlo con el papel.</small></label>
        </section>
        <section class="tarjeta paso">
          <div class="paso-titulo"><span class="paso-num">2</span><h3>Tipo de portada</h3></div>
          ${chips("tipo", [["rustica", "Rústica"], ["tapa_dura", "Tapa dura"]], o.tipo)}
          ${o.tipo === "rustica" ? `
            <label class="campo"><span>Papel de la portada</span><select id="po-cubierta">${opcionesPapel(cubierta, "Sin sumar al lomo")}</select></label>
            <label class="campo"><span>Solapas (mm, 0 = sin solapas)</span><input type="number" id="po-solapa" value="${o.solapa}"></label>` : `
            <div class="fila">
              <label class="campo"><span>Cartón (mm)</span><input type="number" step="0.1" id="po-carton" value="${o.carton}"></label>
              <label class="campo"><span>Escuadra (mm)</span><input type="number" step="0.5" id="po-escuadra" value="${o.escuadra}"></label>
            </div>
            <div class="fila">
              <label class="campo"><span>Vuelta (mm)</span><input type="number" id="po-vuelta" value="${o.vuelta}"></label>
              <label class="campo"><span>Bisagra (mm)</span><input type="number" id="po-bisagra" value="${o.bisagra}"></label>
            </div>`}
          <div class="fila">
            <label class="campo"><span>Rebase (mm)</span><input type="number" step="0.5" id="po-rebase" value="${o.rebase}"></label>
            <div class="campo" style="align-content:end">${interruptor("po-rtl", "Derecha a izquierda", o.rtl)}</div>
          </div>
        </section>
      </div>
      <div class="resultado" id="po-resultado"></div>
    </div>`;
  $$("input, select", main).forEach((el) => el.addEventListener("change", () => {
    if (el.type === "file") return;
    o.ancho = num($("#po-ancho").value, 148); o.alto = num($("#po-alto").value, 210);
    o.paginas = Math.max(0, Math.round(num($("#po-paginas").value, 0)));
    o.lomo = $("#po-lomo").value;
    o.rebase = num($("#po-rebase").value, 3);
    o.rtl = $("#po-rtl").checked;
    if (o.tipo === "rustica") { o.solapa = num($("#po-solapa").value, 0); preferir("papelCubierta", $("#po-cubierta").value); }
    else { o.carton = num($("#po-carton").value, 2.5); o.escuadra = num($("#po-escuadra").value, 3); o.vuelta = num($("#po-vuelta").value, 15); o.bisagra = num($("#po-bisagra").value, 8); }
    preferir("papelPortadaTripa", $("#po-papel").value);
    calcularPortada();
  }));
  conectarChips(main, (n, v) => { o[n] = v; vistaPortada(main); });
  calcularPortada();
}

function peticionPortada() {
  const o = estado.portada.op;
  const tripa = estado.papeles.find((p) => p.id === $("#po-papel")?.value);
  const cubierta = estado.papeles.find((p) => p.id === $("#po-cubierta")?.value);
  return {
    formato: { ancho: o.ancho, alto: o.alto },
    lomo: o.lomo === "" ? null : num(o.lomo),
    paginas: o.paginas, calibre_um: tripa?.calibre_um ?? null, calibre_portada_um: cubierta?.calibre_um ?? null,
    tipo: o.tipo === "rustica" ? { rustica: { solapa: o.solapa } } : { tapa_dura: { carton: o.carton, escuadra: o.escuadra, vuelta: o.vuelta, bisagra: o.bisagra } },
    rebase: o.rebase, derecha_a_izquierda: o.rtl, marcas: true, orden: o.orden,
    titulo: tripa ? `${o.paginas} pp en ${tripa.nombre}` : "Montajes", fecha: ahora(),
  };
}

const NOMBRES_PANEL = { solapa_contratapa: "Solapa", contratapa: "Contratapa", bisagra: "Bisagra", lomo: "Lomo", tapa: "Tapa", solapa_tapa: "Solapa", vuelta: "Vuelta" };
function svgPortada(c) {
  const W = c.tamano.ancho, H = c.tamano.alto, r = c.rebase;
  const y = (v, h) => H - v - h;
  const colores = { tapa: "#4c9ef3", contratapa: "#e4ea5b", lomo: "#ff6b2c", solapa_tapa: "#8b8cf0", solapa_contratapa: "#8b8cf0", bisagra: "#c9c8c2", vuelta: "#e3e2dc" };
  const partes = [`<rect x="${-r}" y="${-r}" width="${W + 2 * r}" height="${H + 2 * r}" fill="none" stroke="#d93b2b" stroke-width="${W / 900}"/>`];
  for (const p of c.paneles) {
    const pr = p.rect;
    partes.push(`<rect x="${pr.x}" y="${y(pr.y, pr.alto)}" width="${pr.ancho}" height="${pr.alto}" fill="${colores[p.tipo]}" fill-opacity="${p.tipo === "vuelta" ? 1 : 0.55}"/>`);
  }
  for (const p of c.paneles) {
    if (p.tipo === "vuelta" || p.tipo === "bisagra") continue;
    const pr = p.rect, cx = pr.x + pr.ancho / 2, cy = y(pr.y, pr.alto) + pr.alto / 2;
    const fs = Math.min(W / 30, 14);
    const vertical = pr.ancho < fs * 4;
    partes.push(`<g transform="${vertical ? `rotate(-90 ${cx} ${cy})` : ""}"><text x="${cx}" y="${cy - fs * 0.3}" font-size="${fs}" font-weight="700" text-anchor="middle" fill="#111">${NOMBRES_PANEL[p.tipo]}</text><text x="${cx}" y="${cy + fs * 0.9}" font-size="${fs * 0.75}" text-anchor="middle" fill="#111">${mm(pr.ancho, 2)} mm</text></g>`);
  }
  for (const x of c.pliegues) partes.push(`<line x1="${x}" y1="${-r}" x2="${x}" y2="${H + r}" stroke="#c03ac0" stroke-width="${W / 700}" stroke-dasharray="${W / 150}"/>`);
  for (const v of c.pliegues_horizontales || []) partes.push(`<line x1="${-r}" y1="${H - v}" x2="${W + r}" y2="${H - v}" stroke="#c03ac0" stroke-width="${W / 700}" stroke-dasharray="${W / 150}"/>`);
  partes.push(`<rect x="0" y="0" width="${W}" height="${H}" fill="none" stroke="#111" stroke-width="${W / 700}"/>`);
  const m = r + W * 0.01;
  return `<svg viewBox="${-m} ${-m} ${W + 2 * m} ${H + 2 * m}" role="img" aria-label="Portada extendida de ${mm(W)} por ${mm(H)} mm">${partes.join("")}</svg>`;
}

function calcularPortada() {
  const t = estado.portada;
  const caja = $("#po-resultado");
  if (!caja) return;
  let c = null, error = "";
  try { c = JSON.parse(motor.calcular_portada_json(JSON.stringify(peticionPortada()))); }
  catch (e) { error = String(e.message || e); }
  t.calculo = c;
  const paneles = c ? c.paneles.filter((p) => p.tipo !== "vuelta" || p.rect.alto >= c.tamano.alto - 0.001) : [];
  const html = `
    ${error ? `<div class="error-caja">${esc(error.includes("calibre") || error.includes("páginas") ? "Elige el papel de la tripa (o escribe el lomo a mano) para calcular la portada." : error)}</div>` : ""}
    ${c ? `<section class="tarjeta-oscura">
      <div class="metricas">
        <div class="metrica"><b>${mm(c.lomo, 2)}</b><span>mm de lomo</span></div>
        <div class="metrica"><b>${mm(c.tamano.ancho, 1)}</b><span>mm de ancho total</span></div>
        <div class="metrica"><b>${mm(c.tamano.alto, 1)}</b><span>mm de alto + ${mm(c.rebase)} de rebase</span></div>
      </div>
      <p class="explicacion" style="margin-top:20px">${paneles.map((p) => `${NOMBRES_PANEL[p.tipo]} <span class="resaltado">${mm(p.rect.ancho, 2)}</span>`).join(" · ")}</p>
      <div class="acciones-resultado" style="margin-top:20px">
        <button class="boton boton-blanco" id="po-plantilla" type="button">Descargar plantilla</button>
        <label class="boton boton-claro" style="cursor:pointer">Armar con mi PDF<input type="file" id="po-armar" accept="application/pdf,.pdf" hidden></label>
      </div>
      <p class="tenue" style="margin-top:14px;font-size:14px">Para armar: un PDF de una sola página con la portada completa, o páginas sueltas en este orden: tapa, contratapa, lomo, solapa de tapa y solapa de contratapa.</p>
    </section>
    <section class="tarjeta vista-previa"><div class="lienzo tira-portada">${svgPortada(c)}</div>
      <div class="leyenda"><span><i style="border-color:#111"></i>Corte</span><span><i style="border-color:#d93b2b"></i>Rebase</span><span><i style="border-color:#c03ac0;border-top-style:dashed"></i>Pliegue</span></div></section>` : ""}`;
  if (!pintar(caja, html)) return;
  $("#po-plantilla")?.addEventListener("click", () => {
    try { const r = motor.plantilla_portada(JSON.stringify(peticionPortada())); descargar(r.pdf, `plantilla-portada-${mm(c.lomo, 1).replace(",", "_")}mm.pdf`); avisar("Plantilla descargada"); }
    catch (e) { avisar(`Error: ${e.message || e}`); }
  });
  $("#po-armar")?.addEventListener("change", async (e) => {
    const archivo = e.target.files[0];
    if (!archivo) return;
    await respirar();
    try {
      const bytes = new Uint8Array(await archivo.arrayBuffer());
      const r = motor.armar_portada(bytes, JSON.stringify(peticionPortada()), new Uint8Array());
      descargar(r.pdf, `${base(archivo.name)}-portada.pdf`);
      const inf = JSON.parse(r.informe);
      avisar(inf.completa ? "Portada verificada: la medida es correcta" : "Portada armada");
    } catch (err) { avisar(`${err.message || err}`); }
    e.target.value = "";
  });
}

// ───────────── Catálogos ─────────────
const CONDICIONES = ["FOGRA39", "FOGRA51", "FOGRA52", "GRACoL2013", "SWOP2013C3", "PSOuncoated_v3", "Otra"];
const EJEMPLOS = [
  { id: "sm74", nombre: "Heidelberg SM74", tipo: "offset", pliego_max: { ancho: 740, alto: 530 }, pinza: 10, cola: 6, lateral: 5, colores: 4, duplex: false, condicion: "FOGRA39" },
  { id: "gto52", nombre: "Heidelberg GTO 52", tipo: "offset", pliego_max: { ancho: 520, alto: 360 }, pinza: 10, cola: 5, lateral: 5, colores: 1, duplex: false, condicion: "FOGRA39" },
  { id: "digital", nombre: "Digital 33×48", tipo: "digital", pliego_max: { ancho: 480, alto: 330 }, pinza: 4, cola: 4, lateral: 4, colores: 4, duplex: true, condicion: "FOGRA39" },
];

function nuevaMaquina(d) {
  return {
    id: d.id, nombre: d.nombre, tipo: d.tipo, pliego_max: d.pliego_max, pliego_min: d.pliego_min ?? null,
    pinza: d.pinza, cola: d.cola, lateral: d.lateral, plancha: null, colores: d.colores, duplex: d.duplex,
    salida: { pdfx: d.pdfx || "PDF/X-4", perfil_icc: null, condicion: d.condicion || null, jdf: !!d.jdf }, notas: d.notas || "",
  };
}
const slug = (t) => t.normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "").slice(0, 32) || "item";

function vistaCatalogos(main) {
  const pestana = estado.preferencias.pestanaCatalogo || "maquinas";
  main.innerHTML = `
    <div class="encabezado">
      <div><h1>Catálogos</h1><p>Máquinas y papeles del taller. Con la nube del taller se comparten entre todos los equipos; también puedes exportar una copia.</p></div>
      <div class="acciones-resultado">
        <button class="boton boton-claro boton-chico" id="ct-exportar" type="button">Exportar copia</button>
        <label class="boton boton-claro boton-chico" style="cursor:pointer">Importar<input type="file" id="ct-importar" accept="application/json,.json" hidden></label>
      </div>
    </div>
    ${bloqueNube()}
    <div class="pestanas">${chips("pestana", [["maquinas", `Máquinas · ${estado.maquinas.length}`], ["papeles", `Papeles · ${estado.papeles.length}`]], pestana)}</div>
    <div id="ct-contenido"></div>
    <dialog id="ct-dialogo"></dialog>`;
  conectarChips(main, (_, v) => { preferir("pestanaCatalogo", v); vistaCatalogos(main); });
  conectarNube(main);
  const contenido = $("#ct-contenido");
  if (pestana === "maquinas") listaMaquinas(contenido, main); else listaPapeles(contenido, main);

  $("#ct-exportar").addEventListener("click", () => {
    const datos = JSON.stringify({ formato: "montajes-catalogo", version: 1, maquinas: estado.maquinas, papeles: estado.papeles }, null, 2);
    const url = URL.createObjectURL(new Blob([datos], { type: "application/json" }));
    Object.assign(document.createElement("a"), { href: url, download: "montajes-catalogo.json" }).click();
    setTimeout(() => URL.revokeObjectURL(url), 5000);
  });
  $("#ct-importar").addEventListener("change", async (e) => {
    try {
      const d = JSON.parse(await e.target.files[0].text());
      let n = 0;
      for (const m of d.maquinas || []) if (m.id && !estado.maquinas.some((x) => x.id === m.id)) { estado.maquinas.push(m); n++; }
      for (const p of d.papeles || []) if (p.id && !estado.papeles.some((x) => x.id === p.id)) { estado.papeles.push(p); n++; }
      guardarCatalogos();
      avisar(`${n} elementos importados`);
      vistaCatalogos(main);
    } catch { avisar("Ese archivo no es una copia de catálogo válida"); }
  });
}

function listaMaquinas(caja, main) {
  const ms = estado.maquinas;
  caja.innerHTML = `
    <section class="tarjeta buscador-maquinas">
      <div class="paso-titulo"><span class="orbe buscador-orbe" aria-hidden="true"></span><div><h3>Buscar una máquina</h3><p class="tenue" style="font-size:14px">Escribe la marca o el modelo y se cargan todas sus características.</p></div></div>
      <input type="search" id="mq-buscar" placeholder="Ej.: SM 74, Komori 40, Indigo 7900, Versant…" autocomplete="off" value="${esc(estado.buscarMaquina || "")}">
      <div id="mq-resultados"></div>
    </section>
    <div class="acciones-resultado" style="margin:16px 0">
      <button class="boton boton-naranja" id="mq-nueva" type="button">Nueva máquina</button>
      ${ms.length ? "" : `<button class="boton boton-claro" id="mq-ejemplos" type="button">Cargar ejemplos</button>`}
    </div>
    ${ms.length ? `<div class="lista">${ms.map((m, i) => `
      <article class="tarjeta item">
        <div class="item-cabeza">${[formas.estrella, formas.flor, formas.cuadro, formas.circulo, formas.destello][i % 5](COLORES[i % 4])}<div><h3>${esc(m.nombre)}</h3><span class="etiqueta">${m.tipo === "offset" ? "Offset" : m.tipo === "digital" ? "Digital" : "Gran formato"}</span></div></div>
        <dl>
          <dt>Pliego máx.</dt><dd>${mm(m.pliego_max.ancho)} × ${mm(m.pliego_max.alto)} mm</dd>
          <dt>Pinza / cola</dt><dd>${mm(m.pinza)} / ${mm(m.cola)} mm</dd>
          <dt>Laterales</dt><dd>${mm(m.lateral)} mm</dd>
          <dt>Colores</dt><dd>${m.colores}${m.duplex ? " · dúplex" : ""}</dd>
          <dt>Salida</dt><dd>${esc(m.salida?.pdfx || "PDF/X-4")}${m.salida?.condicion ? ` · ${esc(m.salida.condicion)}` : ""} <span data-icc="${esc(m.id)}"></span></dd>
        </dl>
        <div class="acciones"><button class="boton boton-claro boton-chico" data-editar="${esc(m.id)}" type="button">Editar</button><button class="boton boton-claro boton-chico" data-borrar="${esc(m.id)}" type="button">Borrar</button></div>
      </article>`).join("")}</div>` : `<section class="tarjeta"><div class="vacio" style="padding:40px 0"><span class="orbe" aria-hidden="true"></span><h2>Sin máquinas todavía</h2><p>Crea cada máquina con su pliego máximo, pinza y márgenes. Los montajes se calculan con esos datos.</p></div></section>`}`;
  for (const m of ms) iccDB.leer(m.id).then((b) => { const s = $(`[data-icc="${CSS.escape(m.id)}"]`); if (s) s.textContent = b ? "· ICC ✓" : "· sin ICC"; });
  $("#mq-nueva").addEventListener("click", () => dialogoMaquina(null, main));
  const buscar = $("#mq-buscar");
  buscar.addEventListener("input", () => { estado.buscarMaquina = buscar.value; pintarResultados($("#mq-resultados"), buscar.value, main); });
  pintarResultados($("#mq-resultados"), buscar.value, main);
  $("#mq-ejemplos")?.addEventListener("click", () => { estado.maquinas.push(...EJEMPLOS.map(nuevaMaquina)); guardarCatalogos(); avisar("Ejemplos cargados: ajústalos a tus máquinas"); vistaCatalogos(main); });
  $$("[data-editar]", caja).forEach((b) => b.addEventListener("click", () => dialogoMaquina(estado.maquinas.find((m) => m.id === b.dataset.editar), main)));
  $$("[data-borrar]", caja).forEach((b) => b.addEventListener("click", () => {
    const m = estado.maquinas.find((x) => x.id === b.dataset.borrar);
    if (!confirm(`¿Borrar la máquina «${m.nombre}»?`)) return;
    borrarDelCatalogo("maquinas", m);
    iccDB.borrar(m.id);
    guardarCatalogos();
    vistaCatalogos(main);
  }));
}

// ───────────── Buscador de máquinas ─────────────
const CLAVE_IA = "claveAnthropic";

function pintarResultados(caja, consulta, main) {
  const texto = consulta.trim();
  if (!texto) { caja.innerHTML = ""; return; }
  const encontrados = buscarMaquinas(texto);
  const clave = almacen.leer(CLAVE_IA, "");
  caja.innerHTML = `
    ${encontrados.length ? `<ul class="resultados-maquinas">${encontrados.map((f, i) => `
      <li><div><b>${esc(f.modelo)}</b><span class="tenue">${f.tipo === "offset" ? "Offset" : "Digital"} · pliego ${f.pliego[0]}×${f.pliego[1]} mm · área ${f.impresion[0]}×${f.impresion[1]} · pinza ${f.pinza} mm${f.verificado ? " · ficha verificada" : ""}</span></div>
      <button class="boton boton-chico" type="button" data-ficha="${i}">Agregar</button></li>`).join("")}</ul>` : `<p class="tenue" style="margin-top:12px">No está en el catálogo de referencia.</p>`}
    <div class="busqueda-ia">
      <button class="boton boton-claro boton-chico" type="button" id="mq-ia">Buscar «${esc(texto)}» en internet</button>
      <span class="tenue" style="font-size:13px">${clave ? "Con IA: lee las fichas del fabricante y llena el formulario." : "Necesita una clave de API de Anthropic (una sola vez)."}</span>
    </div>
    <div id="mq-ia-estado"></div>`;
  $$("[data-ficha]", caja).forEach((b) => b.addEventListener("click", () => dialogoMaquina(null, main, aMaquina(encontrados[Number(b.dataset.ficha)]))));
  $("#mq-ia", caja).addEventListener("click", () => buscarEnInternet(texto, $("#mq-ia-estado", caja), main));
}

function formularioClave(caja, alGuardar) {
  caja.innerHTML = `<form class="clave-ia" id="clave-form">
    <label class="campo"><span>Clave de API de Anthropic</span><input type="password" name="clave" placeholder="sk-ant-…" required autocomplete="off"></label>
    <small class="tenue">Se guarda solo en este navegador y se usa para buscar fichas técnicas con Claude y su búsqueda web. Cada búsqueda tiene un costo pequeño en tu cuenta de Anthropic. Créala en console.anthropic.com → API Keys.</small>
    <div class="acciones-resultado"><button class="boton boton-chico">Guardar y buscar</button></div>
  </form>`;
  $("#clave-form", caja).addEventListener("submit", (e) => {
    e.preventDefault();
    const clave = new FormData(e.target).get("clave").trim();
    if (!clave) return;
    almacen.guardar(CLAVE_IA, clave);
    alGuardar(clave);
  });
}

async function buscarEnInternet(consulta, caja, main) {
  const clave = almacen.leer(CLAVE_IA, "");
  if (!clave) { formularioClave(caja, () => buscarEnInternet(consulta, caja, main)); return; }
  caja.innerHTML = `<div class="buscando"><span class="orbe orbe-respira" aria-hidden="true"></span><div><b>Buscando la ficha de «${esc(consulta)}»…</b><span class="tenue">Revisando fichas del fabricante y distribuidores. Puede tardar hasta un minuto.</span></div></div>`;
  try {
    const { buscarFichaEnInternet, fichaAMaquina } = await import("./busqueda-ia.js");
    const { ficha, fuentes } = await buscarFichaEnInternet(consulta, clave);
    // Solo enlaces http(s) válidos: las URLs vienen de la búsqueda.
    const enlaces = fuentes.flatMap((u) => {
      try { const url = new URL(u); return /^https?:$/.test(url.protocol) ? [{ href: url.href, dominio: url.hostname.replace(/^www\./, "") }] : []; }
      catch { return []; }
    });
    caja.innerHTML = `<div class="ia-encontrado"><b>Encontrada: ${esc(ficha.modelo)}</b>
      <span class="tenue">Pliego ${ficha.pliego_max_ancho_mm}×${ficha.pliego_max_alto_mm} mm · ${ficha.colores} colores${ficha.notas ? ` · ${esc(ficha.notas)}` : ""}</span>
      ${enlaces.length ? `<span class="fuentes">Fuentes: ${enlaces.slice(0, 4).map(({ href, dominio }) => `<a href="${esc(href)}" target="_blank" rel="noopener noreferrer">${esc(dominio)}</a>`).join(" · ")}</span>` : ""}</div>`;
    dialogoMaquina(null, main, fichaAMaquina(ficha, fuentes));
  } catch (e) {
    const sinClave = /clave de API no es válida/.test(e.message);
    caja.innerHTML = `<div class="error-caja">${esc(e.message)}${sinClave ? ` <button class="boton boton-claro boton-chico" type="button" id="clave-cambiar">Cambiar clave</button>` : ""}</div>`;
    $("#clave-cambiar", caja)?.addEventListener("click", () => { almacen.guardar(CLAVE_IA, ""); buscarEnInternet(consulta, caja, main); });
  }
}

function dialogoMaquina(m, main, datosIniciales = null) {
  const d = $("#ct-dialogo");
  const v = m || nuevaMaquina({ id: "", ...(datosIniciales || { nombre: "", tipo: "offset", pliego_max: { ancho: 700, alto: 500 }, pinza: 10, cola: 5, lateral: 5, colores: 4, duplex: false, condicion: "FOGRA39" }) });
  d.innerHTML = `<form method="dialog" id="mq-form">
    <h2>${m ? "Editar máquina" : datosIniciales ? "Revisa y guarda" : "Nueva máquina"}</h2>
    ${datosIniciales ? `<p class="tenue" style="font-size:14px">Datos cargados de la ficha técnica. Revísalos con tu máquina (la pinza y los márgenes reales pueden variar) y guarda.</p>` : ""}
    <label class="campo"><span>Nombre</span><input type="text" name="nombre" required value="${esc(v.nombre)}" placeholder="Heidelberg SM74"></label>
    <div class="campo"><span>Tipo</span>${chips("tipo", [["offset", "Offset"], ["digital", "Digital"], ["gran_formato", "Gran formato"]], v.tipo)}</div>
    <div class="fila">
      <label class="campo"><span>Pliego máx. ancho (mm)</span><input type="number" step="0.1" name="ancho" required value="${v.pliego_max.ancho}"></label>
      <label class="campo"><span>Pliego máx. alto (mm)</span><input type="number" step="0.1" name="alto" required value="${v.pliego_max.alto}"></label>
    </div>
    <small class="tenue">El ancho es el lado de la pinza.</small>
    <div class="fila-3">
      <label class="campo"><span>Pinza</span><input type="number" step="0.5" min="0" name="pinza" value="${v.pinza}"></label>
      <label class="campo"><span>Cola</span><input type="number" step="0.5" min="0" name="cola" value="${v.cola}"></label>
      <label class="campo"><span>Laterales</span><input type="number" step="0.5" min="0" name="lateral" value="${v.lateral}"></label>
    </div>
    <div class="fila">
      <label class="campo"><span>Colores / cuerpos</span><input type="number" min="1" max="12" name="colores" value="${v.colores}"></label>
      <label class="campo"><span>Formato de salida</span><select name="pdfx"><option ${v.salida.pdfx === "PDF/X-4" ? "selected" : ""}>PDF/X-4</option><option ${v.salida.pdfx === "PDF/X-1a" ? "selected" : ""}>PDF/X-1a</option></select></label>
    </div>
    ${interruptor("mq-duplex", "Imprime las dos caras en una pasada", v.duplex)}
    <label class="campo"><span>Condición de impresión</span><select name="condicion">${CONDICIONES.map((c) => `<option ${c === v.salida.condicion ? "selected" : ""}>${c}</option>`).join("")}</select></label>
    <label class="campo"><span>Perfil ICC de salida (opcional)</span><input type="file" name="icc" accept=".icc,.icm"><small>Con el perfil, el PDF sale identificado como PDF/X con su OutputIntent.</small></label>
    ${interruptor("mq-jdf", "Su RIP/CTP recibe JDF", v.salida.jdf)}
    <label class="campo"><span>Notas</span><input type="text" name="notas" value="${esc(v.notas)}"></label>
    <div class="acciones"><button class="boton boton-claro" value="cancelar" formnovalidate>Cancelar</button><button class="boton" value="guardar">Guardar</button></div>
  </form>`;
  let tipo = v.tipo;
  conectarChips(d, (_, valor) => { tipo = valor; });
  d.showModal();
  $("#mq-form").addEventListener("submit", async (e) => {
    if (e.submitter?.value !== "guardar") return;
    const f = new FormData(e.target);
    const datos = {
      id: m ? m.id : slug(f.get("nombre")), nombre: String(f.get("nombre")).trim(), tipo,
      pliego_max: { ancho: num(f.get("ancho")), alto: num(f.get("alto")) },
      pinza: num(f.get("pinza")), cola: num(f.get("cola")), lateral: num(f.get("lateral")),
      colores: Math.round(num(f.get("colores"), 4)), duplex: $("#mq-duplex").checked,
      pdfx: f.get("pdfx"), condicion: f.get("condicion") === "Otra" ? null : f.get("condicion"), jdf: $("#mq-jdf").checked, notas: f.get("notas"),
      pliego_min: v.pliego_min,
    };
    if (!m) while (estado.maquinas.some((x) => x.id === datos.id)) datos.id += "-2";
    if (datos.pinza + datos.cola >= datos.pliego_max.alto || 2 * datos.lateral >= datos.pliego_max.ancho) { e.preventDefault(); avisar("Los márgenes ocupan todo el pliego"); return; }
    const nueva = nuevaMaquina(datos);
    if (m) Object.assign(m, nueva); else estado.maquinas.push(nueva);
    const icc = f.get("icc");
    if (icc && icc.size) await iccDB.poner(datos.id, new Uint8Array(await icc.arrayBuffer()));
    guardarCatalogos();
    preferir("maquina", datos.id);
    avisar("Máquina guardada");
    vistaCatalogos(main);
  });
}

function listaPapeles(caja, main) {
  const ps = estado.papeles;
  caja.innerHTML = `
    <div class="acciones-resultado" style="margin-bottom:16px">
      <button class="boton boton-naranja" id="pp-nuevo" type="button">Nuevo papel</button>
      <button class="boton boton-claro" id="pp-referencia" type="button">Cargar biblioteca de referencia</button>
    </div>
    ${ps.length ? `<div class="lista">${ps.map((p, i) => `
      <article class="tarjeta item">
        <div class="item-cabeza">${formas.circulo(COLORES[i % 4])}<div><h3>${esc(p.nombre)}</h3>${p.estucado ? `<span class="etiqueta">Estucado</span>` : ""}</div></div>
        <dl><dt>Gramaje</dt><dd>${mm(p.gramaje)} g/m²</dd><dt>Calibre</dt><dd>${mm(p.calibre_um)} µm</dd>${p.fibra ? `<dt>Fibra</dt><dd>${p.fibra}</dd>` : ""}</dl>
        <div class="acciones"><button class="boton boton-claro boton-chico" data-editar="${esc(p.id)}" type="button">Editar</button><button class="boton boton-claro boton-chico" data-borrar="${esc(p.id)}" type="button">Borrar</button></div>
      </article>`).join("")}</div>` : `<section class="tarjeta"><div class="vacio" style="padding:40px 0"><span class="orbe" aria-hidden="true"></span><h2>Sin papeles todavía</h2><p>Carga la biblioteca de referencia y ajusta los calibres con la ficha de tu proveedor.</p></div></section>`}`;
  $("#pp-nuevo").addEventListener("click", () => dialogoPapel(null, main));
  $("#pp-referencia").addEventListener("click", () => {
    const ref = JSON.parse(motor.papeles_referencia());
    let n = 0;
    for (const p of ref) if (!estado.papeles.some((x) => x.id === p.id)) { estado.papeles.push(p); n++; }
    guardarCatalogos();
    avisar(`${n} papeles agregados. Verifica los calibres con tu proveedor`);
    vistaCatalogos(main);
  });
  $$("[data-editar]", caja).forEach((b) => b.addEventListener("click", () => dialogoPapel(estado.papeles.find((p) => p.id === b.dataset.editar), main)));
  $$("[data-borrar]", caja).forEach((b) => b.addEventListener("click", () => {
    const p = estado.papeles.find((x) => x.id === b.dataset.borrar);
    if (!confirm(`¿Borrar el papel «${p.nombre}»?`)) return;
    borrarDelCatalogo("papeles", p);
    guardarCatalogos();
    vistaCatalogos(main);
  }));
}

function dialogoPapel(p, main) {
  const d = $("#ct-dialogo");
  const v = p || { id: "", nombre: "", gramaje: 75, calibre_um: 100, estucado: false, fibra: null, pliegos: [{ ancho: 700, alto: 1000 }], notas: "" };
  d.innerHTML = `<form method="dialog" id="pp-form">
    <h2>${p ? "Editar papel" : "Nuevo papel"}</h2>
    <label class="campo"><span>Nombre</span><input type="text" name="nombre" required value="${esc(v.nombre)}" placeholder="Bond 75 g"></label>
    <div class="fila">
      <label class="campo"><span>Gramaje (g/m²)</span><input type="number" step="0.1" min="1" name="gramaje" required value="${v.gramaje}"></label>
      <label class="campo"><span>Calibre (µm)</span><input type="number" step="1" min="1" name="calibre" required value="${v.calibre_um}"><small>Micras: 0,1 mm = 100 µm</small></label>
    </div>
    ${interruptor("pp-estucado", "Estucado (brillante, mate o satinado)", v.estucado)}
    <div class="campo"><span>Fibra</span>${chips("fibra", [["", "Sin indicar"], ["larga", "Larga"], ["corta", "Corta"]], v.fibra || "")}</div>
    <label class="campo"><span>Notas</span><input type="text" name="notas" value="${esc(v.notas)}"></label>
    <div class="acciones"><button class="boton boton-claro" value="cancelar" formnovalidate>Cancelar</button><button class="boton" value="guardar">Guardar</button></div>
  </form>`;
  let fibra = v.fibra || "";
  conectarChips(d, (_, valor) => { fibra = valor; });
  d.showModal();
  $("#pp-form").addEventListener("submit", (e) => {
    if (e.submitter?.value !== "guardar") return;
    const f = new FormData(e.target);
    const datos = { ...v, nombre: String(f.get("nombre")).trim(), gramaje: num(f.get("gramaje")), calibre_um: num(f.get("calibre")), estucado: $("#pp-estucado").checked, fibra: fibra || null, notas: f.get("notas") };
    if (!p) { datos.id = slug(datos.nombre); while (estado.papeles.some((x) => x.id === datos.id)) datos.id += "-2"; estado.papeles.push(datos); }
    else Object.assign(p, datos);
    guardarCatalogos();
    avisar("Papel guardado");
    vistaCatalogos(main);
  });
}

// ───────────── Arranque ─────────────
async function arrancar() {
  try {
    await iniciarMotor();
    $("#version").textContent = `Motor ${motor.version()}`;
  } catch (e) {
    $("#vista").innerHTML = `<div class="error-caja">No se pudo cargar el motor de imposición en este navegador (${esc(e.message || e)}). Usa una versión reciente de Chrome, Edge, Firefox o Safari.</div>`;
    return;
  }
  window.addEventListener("hashchange", navegar);
  navegar();
  // Pide al navegador no borrar estos datos cuando le falte espacio.
  navigator.storage?.persist?.().catch(() => {});
  if (codigoTaller()) {
    sincronizar();
    document.addEventListener("visibilitychange", () => { if (document.visibilityState === "visible") sincronizar(); });
  }
}
arrancar();
