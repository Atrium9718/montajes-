// Macula · interfaz web. El motor (Rust → WebAssembly) hace todo el cálculo
// y escribe los PDF; aquí solo se piden datos, se dibuja la vista previa y se
// guardan los catálogos en el navegador.

import iniciarMotor, * as motor from "./motor/montajes_web.js";
import { aMaquina, buscarMaquinas } from "./maquinas-catalogo.js";
import { vistaDiagramacion } from "./diagramacion.js";
import { api, vistaAdmin, vistaCuenta } from "./cuenta.js";

const $ = (s, el = document) => el.querySelector(s);
const $$ = (s, el = document) => [...el.querySelectorAll(s)];
const esc = (t) => String(t ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
const mm = (v, d = 1) => (Math.round(v * 10 ** d) / 10 ** d).toLocaleString("es-CO", { maximumFractionDigits: d });
/** Rebase mínimo de imprenta: nunca menos de 3 mm. */
const REBASE_MINIMO = 3;
const rebaseMinimo = (v) => {
  const r = Math.max(REBASE_MINIMO, Number(v) || REBASE_MINIMO);
  if (Number(v) < REBASE_MINIMO) setTimeout(() => avisar(`El rebase mínimo es de ${REBASE_MINIMO} mm`), 0);
  return r;
};
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
      const r = indexedDB.open("montajes", 2);
      r.onupgradeneeded = () => {
        for (const almacen of ["icc", "fuentes"]) if (!r.result.objectStoreNames.contains(almacen)) r.result.createObjectStore(almacen);
      };
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

// Fuentes tipográficas propias para la diagramación (mismo IndexedDB).
const fuentesDB = {
  async poner(nombre, valor) { const db = await iccDB.abrir(); return new Promise((ok, mal) => { const t = db.transaction("fuentes", "readwrite"); t.objectStore("fuentes").put(valor, nombre); t.oncomplete = ok; t.onerror = () => mal(t.error); }); },
  async borrar(nombre) { try { const db = await iccDB.abrir(); db.transaction("fuentes", "readwrite").objectStore("fuentes").delete(nombre); } catch { /* sin almacenamiento */ } },
  async todas() {
    try {
      const db = await iccDB.abrir();
      return await new Promise((ok) => {
        const almacen = db.transaction("fuentes").objectStore("fuentes");
        const claves = almacen.getAllKeys(), valores = almacen.getAll();
        valores.onsuccess = () => ok(claves.result.map((k, i) => [k, valores.result[i]]));
        valores.onerror = () => ok([]);
      });
    } catch { return []; }
  },
};

const estado = {
  maquinas: almacen.leer("maquinas", []),
  papeles: almacen.leer("papeles", []),
  preferencias: almacen.leer("preferencias", {}),
  piezas: { archivo: null, archivos: [], info: null, plan: null, error: null, cara: 0, op: { modo: "repetir", cantidades: [], rebase: 3, calle: 6, orientacion: "auto", dorso: false, volteo: "lateral", marcas: true, tira: true } },
  libro: { archivo: null, info: null, plan: null, error: null, cara: 0, op: { encuadernacion: "lomo", firma: "auto", aprovechamiento: "auto", rebase: 3, fresado: 3, refile: 3, rtl: false, marcas: true, tira: true, creep: true } },
  portada: { archivo: null, info: null, calculo: null, error: null, op: { ancho: 148, alto: 210, paginas: 240, lomo: "", tipo: "rustica", solapa: 0, carton: 2.5, escuadra: 3, vuelta: 15, bisagra: 8, rebase: 3, rtl: false, orden: ["tapa", "contratapa", "lomo", "solapa_tapa", "solapa_contratapa"] } },
};

// Correcciones automáticas: preferencia del taller, común a piezas y libros.
const CORRECCIONES = [
  ["sobreimprimir_negro", "Sobreimprimir el negro 100 %", "Evita filetes blancos si el registro se mueve."],
  ["quitar_sobreimpresion_blanco", "Quitar sobreimpresión de blancos", "Si no, los objetos blancos desaparecen al imprimir."],
  ["linea_minima", "Engrosar líneas finas a 0,25 pt", "Las más finas pueden no verse."],
];
// Cómo se hace el rebase de cada borde. «auto» lo decide mirando la página.
const MODOS_REBASE = [
  ["extendido", "Fondo extendido (recomendado)", "El rebase sale solo de la capa de fondo del diseño (la foto, el degradado, las franjas de color): si la foto es más grande que la página se ve su continuación real, y lo que no alcance se completa reflejando ese fondo. Los textos, QR y logos nunca pasan al rebase."],
  ["auto", "Automático por borde", "En cada borde: usa el rebase del PDF si está limpio; si no, lo hace en espejo; y si junto al corte hay textos o QR, con el color del fondo."],
  ["original", "El que trae el PDF", "Tal cual viene; donde falta, en espejo."],
  ["espejo", "Espejo", "Refleja los 3 mm de la orilla: conserva degradados, fotos y texturas."],
  ["estirar", "Estirar la orilla", "Alarga la última orilla de la página."],
  ["color", "Color del fondo", "Un color plano: el más limpio cuando hay elementos pegados al corte."],
];
// La versión anterior guardaba «auto» como recomendado: ahora el recomendado es «extendido».
const modoRebase = () => (MODOS_REBASE.some(([v]) => v === estado.preferencias.rebaseModo) && !(estado.preferencias.rebaseModo === "auto" && !estado.preferencias.rebaseModoElegido) ? estado.preferencias.rebaseModo : "extendido");
function correcciones() {
  const c = { sobreimprimir_negro: true, quitar_sobreimpresion_blanco: true, linea_minima: true, ...(estado.preferencias.correcciones || {}) };
  // Para páginas sin modo por borde (no se pudieron mirar): estirar lo que falte.
  return { ...c, linea_minima: c.linea_minima ? 0.25 : null, rebase_fondo: false, rebase_estirado: true, rebase_espejo: false, rebase_extendido: modoRebase() === "extendido" };
}
function bloqueCorrecciones(prefijo) {
  const c = correcciones();
  const modo = modoRebase();
  return `<details class="avanzado"><summary>Correcciones automáticas</summary><div class="paso">
    <label class="campo"><span>Rebase</span><select id="${prefijo}-rebase-modo" data-rebase-modo>${MODOS_REBASE.map(([v, t]) => `<option value="${v}" ${v === modo ? "selected" : ""}>${t}</option>`).join("")}</select>
      <small>${MODOS_REBASE.find(([v]) => v === modo)[2]}</small></label>
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
  $$("[data-rebase-modo]", raiz).forEach((el) => el.addEventListener("change", (e) => {
    e.stopPropagation();
    preferir("rebaseModoElegido", true);
    preferir("rebaseModo", el.value);
    el.nextElementSibling.textContent = MODOS_REBASE.find(([v]) => v === el.value)[2];
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

// Código del taller de antes de las cuentas: solo se envía para migrar su catálogo.
const codigoTaller = () => almacen.leer("codigoTaller", "");
const cabecerasNube = (extra = {}) => ({ "X-Macula": "1", ...(codigoTaller() ? { "X-Taller": codigoTaller() } : {}), ...extra });

let temporizadorNube;
function programarSincronizacion() {
  clearTimeout(temporizadorNube);
  temporizadorNube = setTimeout(sincronizar, 700);
}

async function sincronizar() {
  estado.nube = { ...estado.nube, estado: "sincronizando" };
  pintarNube();
  try {
    const r = await fetch("api/catalogo.php", { headers: cabecerasNube(), cache: "no-store" });
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
      headers: cabecerasNube({ "Content-Type": "application/json" }),
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
  if (n.estado === "sincronizando") return "Sincronizando…";
  if (n.estado === "error") return "Sin conexión con la nube: se guardó en este equipo y se sincroniza al volver.";
  if (n.estado === "ok") return `Guardado en la nube de la empresa · ${new Date(n.cuando).toLocaleTimeString("es-CO", { hour: "2-digit", minute: "2-digit" })}`;
  return "Nube de la empresa conectada";
}
function pintarNube() {
  const el = $("#nube-estado");
  if (el) {
    el.textContent = textoNube();
    el.dataset.estado = estado.nube.estado;
  }
}

function bloqueNube() {
  return `<section class="tarjeta nube">
    <div class="nube-cabeza"><span class="orbe" aria-hidden="true"></span><div><h3>Nube de ${esc(estado.cuenta?.empresa?.nombre || "la empresa")}</h3><p id="nube-estado" class="tenue" data-estado="${estado.nube.estado}">${esc(textoNube())}</p></div></div>
    <small class="tenue">Las máquinas y papeles se guardan en la cuenta de la empresa y los ven todos sus usuarios, en cualquier equipo.</small>
  </section>`;
}

function conectarNube() {}
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
const vistas = { cuenta: (main) => vistaCuenta(main, ayudasCuenta()), admin: (main) => vistaAdmin(main, ayudasCuenta()), inicio: vistaInicio, piezas: vistaPiezas, diagramar: (main) => vistaDiagramacion(main, ayudasDiagramacion()), libro: vistaLibro, portada: vistaPortada, catalogos: vistaCatalogos };

function ayudasCuenta() {
  return { esc, chips, avisar, estado, refrescarCuenta };
}

// Lo que la diagramación usa de la app (sin acoplarla al resto de vistas).
function ayudasDiagramacion() {
  return {
    $, $$, esc, mm, num, chips, conectarChips, interruptor, avisar, descargar, respirar, cargarPdfjs, motor, estado, preferir, fuentesDB,
    enviarALibro(archivo) {
      const t = estado.libro;
      analizarArchivo(t, archivo, true);
      t.roles = t.info ? rolesIniciales(t.info) : [];
      t.plan = null;
      t.op.cuadernillos = "";
      // La firma la elige el montaje según la máquina: el cuadre en
      // cuadernillos de la diagramación ya deja la cuenta exacta.
      t.op.firma = "auto";
      location.hash = "#libro";
    },
    enviarAPortada(ancho, alto, paginas) {
      Object.assign(estado.portada.op, { ancho, alto, paginas, lomo: "" });
      location.hash = "#portada";
    },
  };
}
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
    ["diagramar", formas.destello("var(--naranja)"), "Diagramación de libros", "Del manuscrito en Word al interior compuesto y cuadrado en cuadernillos."],
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
        <p style="max-width:440px">${estado.maquinas.length ? "Tus máquinas y papeles están en la nube de la empresa y los ven todos sus usuarios." : "Empieza creando las máquinas del taller: tamaño de pliego, pinza y márgenes. Se guardan para siempre."}</p>
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
/** Máquina elegida, con la pinza que se haya puesto para este trabajo (vale solo para esa máquina). */
function maquinaElegida(id) {
  const m = estado.maquinas.find((x) => x.id === ($(`#${id}`)?.value ?? estado.preferencias.maquina)) || estado.maquinas[0];
  const o = id === "pz-maquina" ? estado.piezas.op : id === "lb-maquina" ? estado.libro.op : null;
  return m && o?.pinza != null && o.pinzaDe === m.id ? { ...m, pinza: o.pinza } : m;
}

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

function analizarArchivo(trabajo, archivo, formatosMixtos = false) {
  trabajo.archivo = archivo;
  trabajo.mini = {};
  trabajo.fondos = {};
  trabajo.bordes = {};
  trabajo.miniFondo = {};
  trabajo.capaFondo = null;
  trabajo.pdfjs = null;
  trabajo.error = null;
  trabajo.cara = 0;
  try {
    trabajo.info = JSON.parse(motor.analizar(archivo.bytes));
    if (!trabajo.info.formato && !formatosMixtos) trabajo.error = "Las páginas del PDF tienen tamaños distintos. Revisa que todas tengan el mismo formato final (TrimBox).";
    revisarArchivo(trabajo);
  } catch (e) {
    trabajo.info = null;
    trabajo.error = `No se pudo leer el PDF: ${mensaje(e)}`;
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
    trabajo.preflight = { hallazgos: [], errores: 0, advertencias: 0, falla: mensaje(e) };
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

/** Texto de un error para mostrar. Si el motor (WebAssembly) se detuvo, queda
 * inservible hasta recargar: se dice así en vez del mensaje técnico. */
function mensaje(e) {
  if (e instanceof WebAssembly.RuntimeError || /memory access out of bounds|unreachable/i.test(String(e?.message))) {
    return "el motor se quedó sin memoria con este PDF. Recarga la página y vuelve a intentarlo; si se repite, envíanos el archivo.";
  }
  return String(e?.message || e);
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
// escribe siempre el motor, sin rasterizar. Va la versión «legacy»: la normal
// no abre en iPhone/iPad con iOS anterior a 17.4 (y ahí también se mide el
// color del fondo para el rebase).
const PDFJS = "https://cdn.jsdelivr.net/npm/pdfjs-dist@4.10.38/legacy/build";
let pdfjsLib = null;
async function cargarPdfjs() {
  if (!pdfjsLib) {
    pdfjsLib = await import(`${PDFJS}/pdf.min.mjs`);
    pdfjsLib.GlobalWorkerOptions.workerSrc = `${PDFJS}/pdf.worker.min.mjs`;
  }
  return pdfjsLib;
}

/**
 * Capa de fondo de la página `i` (la que usa el motor para el rebase
 * extendido), dibujada con transparencia para la vista previa. Queda en
 * trabajo.miniFondo[i] = { imagen, vista } (vista en mm, coordenadas de la página).
 */
async function dibujarFondo(trabajo, i, lib) {
  trabajo.miniFondo ??= {};
  if (i in trabajo.miniFondo) return;
  trabajo.miniFondo[i] = null;
  try {
    if (!trabajo.capaFondo) {
      const r = motor.capa_fondo(trabajo.archivo.bytes);
      trabajo.capaFondo = { hay: JSON.parse(r.informe), doc: lib.getDocument({ data: r.pdf }).promise };
    }
    if (!trabajo.capaFondo.hay[i]) return;
    const doc = await trabajo.capaFondo.doc;
    const pagina = await doc.getPage(i + 1);
    const base = pagina.getViewport({ scale: 1, rotation: 0 });
    const vp = pagina.getViewport({ scale: Math.min(2.5, 800 / Math.max(base.width, base.height)), rotation: 0 });
    const lienzo = Object.assign(document.createElement("canvas"), { width: Math.floor(vp.width), height: Math.floor(vp.height) });
    await pagina.render({ canvasContext: lienzo.getContext("2d"), viewport: vp, background: "rgba(0,0,0,0)" }).promise;
    const mmpt = 25.4 / 72;
    trabajo.miniFondo[i] = { imagen: lienzo.toDataURL("image/png"), vista: pagina.view.map((v) => v * mmpt) };
  } catch (e) {
    console.warn("No se pudo dibujar la capa de fondo:", e);
  }
}

async function pedirMiniaturas(trabajo, indices, redibujar) {
  if (!trabajo.archivo) return;
  trabajo.mini ??= {};
  const faltan = [...new Set(indices)].filter((i) => !(i in trabajo.mini));
  // La capa de fondo se pide también si se cambia a «extendido» con las miniaturas ya hechas.
  const faltanFondo = modoRebase() === "extendido" ? [...new Set(indices)].filter((i) => !(i in (trabajo.miniFondo ?? {}))) : [];
  if (!faltan.length && !faltanFondo.length) return;
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
      const lienzo = Object.assign(document.createElement("canvas"), { width: Math.floor(vp.width), height: Math.floor(vp.height) });
      const ctx = lienzo.getContext("2d", { willReadFrequently: true });
      ctx.fillStyle = "#fff";
      ctx.fillRect(0, 0, lienzo.width, lienzo.height);
      await pagina.render({ canvasContext: ctx, viewport: vp }).promise;
      trabajo.mini[i] = lienzo.toDataURL("image/jpeg", 0.82);
      // El mismo dibujo da el color del fondo para el rebase de la vista previa.
      trabajo.fondos ??= {};
      trabajo.bordes ??= {};
      if (!(i in trabajo.fondos)) try {
        trabajo.fondos[i] = fondosDeLienzo(ctx, lienzo.width, trabajo.info.paginas[i]);
        trabajo.bordes[i] = bordesDeLienzo(lienzo, trabajo.info.paginas[i]);
      } catch (e) { console.warn("No se pudo mirar el borde:", e); }
    }
    for (const i of faltanFondo) {
      if (trabajo.archivo !== archivo) return;
      await dibujarFondo(trabajo, i, lib);
    }
  } catch (e) {
    console.warn("Miniaturas no disponibles:", e);
    for (const i of faltan) trabajo.mini[i] = false;
  }
  if (trabajo.archivo === archivo) redibujar();
}

/** Color del fondo junto a cada borde de corte, leído de la página ya dibujada. */
function fondosDeLienzo(ctx, ancho, info) {
  const [vx0, , , vy1] = info.vista;
  const pxmm = ancho / (info.vista[2] - info.vista[0]);
  const X = (xmm) => Math.round((xmm - vx0) * pxmm), Y = (ymm) => Math.round((vy1 - ymm) * pxmm);
  const [cx0, cy0, cx1, cy1] = info.corte;
  const a = 0.5, b = 2.5;
  const franjas = [
    [X(cx0 + a), Y(cy1), X(cx0 + b), Y(cy0)], // izquierda
    [X(cx0), Y(cy0 + b), X(cx1), Y(cy0 + a)], // abajo
    [X(cx1 - b), Y(cy1), X(cx1 - a), Y(cy0)], // derecha
    [X(cx0), Y(cy1 - a), X(cx1), Y(cy1 - b)], // arriba
  ];
  return franjas.map(([x0, y0, x1, y1]) => {
    const w = Math.max(1, x1 - x0), h = Math.max(1, y1 - y0);
    const datos = ctx.getImageData(Math.max(0, x0), Math.max(0, y0), w, h).data;
    const cubetas = new Map();
    for (let k = 0; k < datos.length; k += 4) {
      const clave = (datos[k] >> 4) * 256 + (datos[k + 1] >> 4) * 16 + (datos[k + 2] >> 4);
      const c = cubetas.get(clave) || [0, 0, 0, 0];
      c[0] += datos[k]; c[1] += datos[k + 1]; c[2] += datos[k + 2]; c[3]++;
      cubetas.set(clave, c);
    }
    const [r, g, bb, n] = [...cubetas.values()].sort((p, q) => q[3] - p[3])[0] || [255, 255, 255, 1];
    return [r / n / 255, g / n / 255, bb / n / 255];
  });
}

/**
 * Rebase automático de cada borde (izquierda, abajo, derecha, arriba):
 * - «original» si el PDF trae rebase y es continuación de la orilla (no hay
 *   textos, QR ni logos que se salgan del corte);
 * - «espejo» si los 3 mm junto al corte son fondo (liso, degradado, foto o
 *   textura): el reflejo los continúa sin costuras;
 * - «color» si junto al corte hay elementos que el espejo repetiría.
 * Se mira a 2 px/mm, que suaviza texturas finas y deja ver los elementos.
 */
function bordesDeLienzo(lienzo, info) {
  const PX = 2, B = 3, LIMITE = 70;
  const [vx0, vy0, vx1, vy1] = info.vista;
  const w = Math.max(1, Math.round((vx1 - vx0) * PX)), h = Math.max(1, Math.round((vy1 - vy0) * PX));
  const chico = Object.assign(document.createElement("canvas"), { width: w, height: h });
  const ctx = chico.getContext("2d", { willReadFrequently: true });
  ctx.imageSmoothingQuality = "high";
  ctx.drawImage(lienzo, 0, 0, w, h);
  const datos = ctx.getImageData(0, 0, w, h).data;
  const pixel = (xmm, ymm) => {
    const X = Math.floor((xmm - vx0) * PX), Y = Math.floor((vy1 - ymm) * PX);
    if (X < 0 || Y < 0 || X >= w || Y >= h) return null;
    const k = (Y * w + X) * 4;
    return [datos[k], datos[k + 1], datos[k + 2]];
  };
  const dif = (a, b) => Math.max(Math.abs(a[0] - b[0]), Math.abs(a[1] - b[1]), Math.abs(a[2] - b[2]));
  const [cx0, cy0, cx1, cy1] = info.corte;
  const paso = 1 / PX;
  // Para cada lado: punto del borde a lo largo t y hacia afuera d (mm).
  const lados = [
    [(t, d) => [cx0 - d, t], cy0, cy1], // izquierda
    [(t, d) => [t, cy0 - d], cx0, cx1], // abajo
    [(t, d) => [cx1 + d, t], cy0, cy1], // derecha
    [(t, d) => [t, cy1 + d], cx0, cx1], // arriba
  ];
  const trae = info.rebase >= 2.5;
  return lados.map(([punto, t0, t1]) => {
    let fuera = 0, nf = 0, dentro = 0, nd = 0;
    for (let t = t0 + paso / 2; t < t1; t += paso) {
      const orilla = pixel(...punto(t, -paso / 2));
      if (!orilla) continue;
      for (let d = paso / 2; d < B; d += paso) {
        const a = pixel(...punto(t, -d));
        if (a) { nd++; if (dif(a, orilla) > LIMITE) dentro++; }
        const b = trae ? pixel(...punto(t, d)) : null;
        if (b) { nf++; if (dif(b, orilla) > LIMITE) fuera++; }
      }
    }
    if (trae && nf && fuera / nf < 0.04) return "original";
    return nd && dentro / nd > 0.04 ? "color" : "espejo";
  });
}

/**
 * Color del fondo junto a cada borde de corte de las páginas (izquierda,
 * abajo, derecha, arriba), en RGB 0–1: el color más frecuente en una franja
 * de 0,5 a 2,5 mm hacia adentro del corte. Sirve para el rebase «solo fondo».
 */
async function medirFondos(trabajo, indices) {
  trabajo.fondos ??= {};
  trabajo.bordes ??= {};
  const faltan = [...new Set(indices)].filter((i) => !(i in trabajo.fondos));
  if (faltan.length) {
    const lib = await cargarPdfjs();
    trabajo.pdfjs ??= lib.getDocument({ data: trabajo.archivo.bytes.slice() }).promise;
    const doc = await trabajo.pdfjs;
    for (const i of faltan) try {
      const info = trabajo.info.paginas[i];
      const pagina = await doc.getPage(i + 1);
      const base = pagina.getViewport({ scale: 1, rotation: 0 });
      const escala = Math.min(4, 900 / Math.max(base.width, base.height));
      const vp = pagina.getViewport({ scale: escala, rotation: 0 });
      const lienzo = Object.assign(document.createElement("canvas"), { width: Math.floor(vp.width), height: Math.floor(vp.height) });
      const ctx = lienzo.getContext("2d", { willReadFrequently: true });
      ctx.fillStyle = "#fff";
      ctx.fillRect(0, 0, lienzo.width, lienzo.height);
      await pagina.render({ canvasContext: ctx, viewport: vp }).promise;
      trabajo.fondos[i] = fondosDeLienzo(ctx, lienzo.width, info);
      trabajo.bordes[i] = bordesDeLienzo(lienzo, info);
    } catch (e) {
      // Sin color medido el motor estira la orilla: tampoco usa lo de fuera del corte.
      console.warn(`No se pudo medir el fondo de la página ${i + 1}:`, e);
      trabajo.fondos[i] = null;
    }
  }
  const lista = [];
  for (const i of indices) lista[i] = trabajo.fondos[i];
  return Array.from({ length: trabajo.info.paginas.length }, (_, i) => lista[i] ?? null);
}

/** Modo de rebase de cada borde de una página (izquierda, abajo, derecha, arriba). */
function modosPagina(trabajo, i) {
  const modo = modoRebase();
  // Con «extendido», las páginas sin capa de fondo usan el automático por borde.
  if (modo !== "auto" && modo !== "extendido") return [modo, modo, modo, modo];
  return trabajo.bordes?.[i] ?? null;
}

/** Correcciones para generar: colores del fondo y modo de rebase de cada borde. */
async function correccionesParaGenerar(trabajo, indices) {
  const c = { ...correcciones(), ...tintasDe(trabajo.op) };
  if (trabajo.archivo && trabajo.info) {
    try { c.fondos = await medirFondos(trabajo, indices); } catch (e) { console.warn("No se pudo medir el fondo:", e); c.fondos = []; }
    const usadas = new Set(indices);
    c.modos = trabajo.info.paginas.map((_, i) => (usadas.has(i) ? modosPagina(trabajo, i) : null));
  }
  return c;
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

/**
 * Rebase por bordes de una ubicación, como pdf::colocar_por_lados: franjas
 * reflejadas o estiradas debajo (zona + transformación en el pliego, mm),
 * el recorte de la página y franjas de color encima. Lados del pliego:
 * izquierda, abajo, derecha, arriba.
 */
function rebasePorLados(u, pag, modosPag, coloresPag) {
  const SOLAPE = 0.2, PASE = 0.25, ORILLA = 0.3;
  const c = u.corte, r = u.recorte;
  const der = (z) => z.x + z.ancho, arr = (z) => z.y + z.alto;
  const pide = [c.x - r.x, c.y - r.y, der(r) - der(c), arr(r) - arr(c)];
  const s = u.escala ?? 1;
  const tiene = pag.rebase * s;
  const pasos = (((pag.giro + u.giro) % 360) / 90) % 4;
  const ciclo = [0, 3, 2, 1];
  const ladoPag = (l) => ciclo[(ciclo.indexOf(l) + 4 - pasos) % 4];
  const hace = pide.map((v) => v > 0.01);
  const color = [0, 1, 2, 3].map((l) => coloresPag?.[ladoPag(l)] ?? null);
  const modo = [0, 1, 2, 3].map((l) => {
    const x = modosPag[ladoPag(l)];
    if (x === "original" && pide[l] > tiene + 0.05) return "espejo";
    if (x === "color" && !color[l]) return "estirar";
    return x;
  });
  const original = modo.map((x, l) => hace[l] && x === "original");
  const reflejo = modo.map((x, l) => hace[l] && (x === "espejo" || x === "estirar"));
  const plano = modo.map((x, l) => hace[l] && x === "color");
  const transformar = (l) => {
    const [borde, signo, horizontal] = [[c.x, 1, true], [c.y, 1, false], [der(c), -1, true], [arr(c), -1, false]][l];
    let k = -1, t = 2 * borde;
    if (modo[l] === "estirar") {
      const fijo = borde + signo * ORILLA;
      k = (ORILLA + pide[l]) / ORILLA;
      t = fijo * (1 - k);
    }
    return horizontal ? [k, 0, 0, 1, t, 0] : [1, 0, 0, k, 0, t];
  };
  const R = (x, y, ancho, alto) => ({ x, y, ancho, alto });
  const [pi, pb, pd, pa] = pide;
  const vy = original[1] ? r.y : c.y, vy2 = original[3] ? arr(r) : arr(c);
  const hx = original[0] ? r.x : c.x, hx2 = original[2] ? der(r) : der(c);
  const bajo = [];
  const franjas = [[0, R(r.x, vy, pi + SOLAPE, vy2 - vy)], [2, R(der(c) - SOLAPE, vy, pd + SOLAPE, vy2 - vy)], [1, R(hx, r.y, hx2 - hx, pb + SOLAPE)], [3, R(hx, arr(c) - SOLAPE, hx2 - hx, pa + SOLAPE)]];
  for (const [l, zona] of franjas) if (reflejo[l]) bajo.push({ zona, T: transformar(l) });
  for (const [v, h, zona] of [[0, 1, R(r.x, r.y, pi, pb)], [2, 1, R(der(c), r.y, pd, pb)], [0, 3, R(r.x, arr(c), pi, pa)], [2, 3, R(der(c), arr(c), pd, pa)]]) {
    if (reflejo[v] && reflejo[h]) bajo.push({ zona, T: multiplicar(transformar(v), transformar(h)) });
  }
  const pase = pide.map((v, l) => (original[l] ? v : hace[l] ? Math.min(v, PASE) : 0));
  const pagina = R(c.x - pase[0], c.y - pase[1], c.ancho + pase[0] + pase[2], c.alto + pase[1] + pase[3]);
  const encima = [];
  for (const [l, zona] of [[0, R(r.x, r.y, pi, r.alto)], [2, R(der(c), r.y, pd, r.alto)], [1, R(r.x, r.y, r.ancho, pb)], [3, R(r.x, arr(c), r.ancho, pa)]]) {
    if (!plano[l] || zona.ancho <= 1e-6 || zona.alto <= 1e-6) continue;
    let z = zona;
    if (l === 1 || l === 3) {
      const x = plano[0] ? c.x : r.x, x2 = plano[2] ? der(c) : der(r);
      z = R(x, zona.y, x2 - x, zona.alto);
    }
    encima.push({ zona: z, rgb: color[l].map((v) => Math.round(v * 255)) });
  }
  return { bajo, pagina, encima };
}

// ───────────── Vista previa del pliego ─────────────
const COLORES = ["#4c9ef3", "#e4ea5b", "#ff6b2c", "#8b8cf0"];

function svgCara(cara, maquina, opciones = {}) {
  const W = cara.pliego.ancho, H = cara.pliego.alto;
  const y = (v, h = 0) => H - v - h; // PDF (abajo-izquierda) → SVG (arriba-izquierda)
  const r = (rc, extra = "") => `<rect x="${rc.x}" y="${y(rc.y, rc.alto)}" width="${rc.ancho}" height="${rc.alto}" ${extra}/>`;
  const partes = [];
  // «Como queda el PDF»: papel blanco, marcas en negro y sin guías encima.
  const guias = opciones.guias !== false;
  const tinta = guias ? "var(--tinta)" : "#1d1d1f";
  partes.push(`<rect x="0" y="0" width="${W}" height="${H}" fill="${guias ? "var(--superficie)" : "#fff"}" stroke="var(--linea)" stroke-width="${W / 400}"/>`);
  if (maquina && !cara.cajas && guias) {
    const a = { x: maquina.lateral, y: maquina.pinza, ancho: W - 2 * maquina.lateral, alto: H - maquina.pinza - maquina.cola };
    partes.push(r(a, `fill="none" stroke="var(--gris)" stroke-width="${W / 700}" stroke-dasharray="${W / 120} ${W / 160}"`));
  }
  // La pinza se marca siempre (no se imprime): así se ve de qué lado entra el papel.
  if (maquina && !cara.cajas && maquina.pinza > 0) {
    partes.push(`<rect x="0" y="${H - maquina.pinza}" width="${W}" height="${maquina.pinza}" fill="var(--naranja)" opacity=".12"/>`);
    partes.push(`<text x="${W / 2}" y="${H - maquina.pinza / 2}" font-size="${Math.max(Math.min(maquina.pinza * 0.7, W / 45), 3)}" text-anchor="middle" dominant-baseline="middle" fill="var(--naranja)" font-weight="700" letter-spacing=".1em">PINZA</text>`);
  }
  if (cara.cajas && guias) partes.push(r(cara.cajas.sangrado, `fill="none" stroke="#d93b2b" stroke-width="${W / 900}"`));
  const { info, mini } = opciones;
  // Lado de la cara (como lo rotula el motor) y sus tintas: a una tinta, gris.
  const nombreCara = cara.nombre.toLowerCase();
  const esRetiro = !nombreCara.includes("tira y retira") && nombreCara.trim().endsWith("retiro");
  const tintasCara = opciones.tintas ? (esRetiro ? opciones.tintas.tintas_retiro : nombreCara.includes("tira y retira") ? Math.max(opciones.tintas.tintas_tiro, opciones.tintas.tintas_retiro) : opciones.tintas.tintas_tiro) : 4;
  const filtro = tintasCara === 1 ? ' style="filter:grayscale(1)"' : "";
  cara.ubicaciones.forEach((u, k) => {
    const color = COLORES[u.pagina % COLORES.length];
    const imagen = mini?.[u.pagina];
    const pag = info?.paginas[u.pagina];
    if (imagen && pag) {
      // Página real: CropBox dibujada por pdf.js, llevada al pliego con la
      // matriz del motor y recortada al rebase permitido.
      const [vx0, vy0, vx1, vy1] = pag.vista;
      let m = matrizColocacion(pag.corte, (pag.giro + u.giro) % 360, u.corte.x, u.corte.y);
      const s = u.escala ?? 1;
      if (Math.abs(s - 1) > 1e-9) m = multiplicar(m, [s, 0, 0, s, u.corte.x * (1 - s), u.corte.y * (1 - s)]);
      const id = `rc-${opciones.clave || "c"}-${k}`;
      const imagenCon = (mt, zona, n) => {
        const tt = multiplicar(multiplicar([1, 0, 0, -1, vx0, vy1], mt), [1, 0, 0, -1, 0, H]);
        partes.push(`<clipPath id="${id}-${n}">${r(zona)}</clipPath><g clip-path="url(#${id}-${n})"><image href="${imagen}" x="0" y="0" width="${vx1 - vx0}" height="${vy1 - vy0}" preserveAspectRatio="none"${filtro} transform="matrix(${tt.map((v) => +v.toFixed(4)).join(" ")})"/></g>`);
      };
      // El rebase de cada borde, igual que lo arma el motor (pdf::colocar_por_lados).
      const modos = opciones.modos?.(u.pagina);
      const capa = opciones.extendido ? opciones.miniFondo?.[u.pagina] : null;
      const mas = (g, e = 0.15) => ({ x: g.x - e, y: g.y - e, ancho: g.ancho + 2 * e, alto: g.alto + 2 * e });
      const c = u.corte, rr = u.recorte;
      const pide = [c.x - rr.x, c.y - rr.y, rr.x + rr.ancho - c.x - c.ancho, rr.y + rr.alto - c.y - c.alto];
      if (capa && pide.some((v) => v > 0.01)) {
        // Igual que el motor (pdf::colocar_extendido): capa de fondo reflejada en
        // cada franja, la capa tal cual en el anillo de rebase y la página encima.
        const [fx0, fy0, fx1, fy1] = capa.vista;
        const fondoCon = (mt, recorte, n) => {
          const tt = multiplicar(multiplicar([1, 0, 0, -1, fx0, fy1], mt), [1, 0, 0, -1, 0, H]);
          partes.push(`<clipPath id="${id}-f${n}">${recorte}</clipPath><g clip-path="url(#${id}-f${n})"><image href="${capa.imagen}" x="0" y="0" width="${fx1 - fx0}" height="${fy1 - fy0}" preserveAspectRatio="none"${filtro} transform="matrix(${tt.map((v) => +v.toFixed(4)).join(" ")})"/></g>`);
        };
        const hace = pide.map((v) => v > 0.01);
        const [pi, pb, pd, pa] = pide;
        const der = c.x + c.ancho, arr = c.y + c.alto;
        const espejo = [[-1, 0, 0, 1, 2 * c.x, 0], [1, 0, 0, -1, 0, 2 * c.y], [-1, 0, 0, 1, 2 * der, 0], [1, 0, 0, -1, 0, 2 * arr]];
        const R = (x, yy, ancho, alto) => ({ x, y: yy, ancho, alto });
        [[0, R(rr.x, c.y, pi, c.alto)], [1, R(c.x, rr.y, c.ancho, pb)], [2, R(der, c.y, pd, c.alto)], [3, R(c.x, arr, c.ancho, pa)]]
          .forEach(([l, z]) => hace[l] && fondoCon(multiplicar(m, espejo[l]), r(mas(z)), `e${l}`));
        [[0, 1, R(rr.x, rr.y, pi, pb)], [2, 1, R(der, rr.y, pd, pb)], [0, 3, R(rr.x, arr, pi, pa)], [2, 3, R(der, arr, pd, pa)]]
          .forEach(([v, h, z]) => hace[v] && hace[h] && fondoCon(multiplicar(multiplicar(m, espejo[v]), espejo[h]), r(mas(z)), `c${v}${h}`));
        const anillo = (z) => `M${z.x} ${y(z.y, z.alto)}h${z.ancho}v${z.alto}h${-z.ancho}z`;
        fondoCon(m, `<path clip-rule="evenodd" d="${anillo(mas(rr))}${anillo(c)}"/>`, "a");
        const pase = pide.map((v, l) => (hace[l] ? Math.min(v, 0.25) : 0));
        imagenCon(m, mas({ x: c.x - pase[0], y: c.y - pase[1], ancho: c.ancho + pase[0] + pase[2], alto: c.alto + pase[1] + pase[3] }), "p");
      } else if (modos) {
        const plan = rebasePorLados(u, pag, modos, opciones.fondos?.[u.pagina]);
        // 0,15 mm de más en cada zona (mas) para que el navegador no deje filos
        // blancos donde se tocan (entre franjas y entre piezas vecinas).
        plan.bajo.forEach(({ zona, T }, n) => imagenCon(multiplicar(m, T), mas(zona), n));
        imagenCon(m, mas(plan.pagina), "p");
        for (const { zona, rgb } of plan.encima) partes.push(r(mas(zona), `fill="rgb(${rgb})"`));
      } else {
        imagenCon(m, u.recorte, "p");
      }
      if (guias) partes.push(r(u.corte, `fill="none" stroke="var(--tinta)" stroke-width="${W / 1100}" stroke-opacity=".5"`));
      // Con guías: los pliegues del plegable sobre cada pieza (como nup::marcar_pliegues).
      const pl = opciones.plegable;
      if (guias && pl?.pliegues?.length) {
        const c = u.corte;
        for (const [i, p0] of pl.pliegues.entries()) {
          const espejo = pl.dorso && u.pagina % 2 === 1;
          const p = espejo ? pl.ancho - p0 : p0;
          const g = u.giro % 360;
          const [x1, y1, x2, y2] = g === 0 ? [c.x + p * c.ancho / pl.ancho, c.y, c.x + p * c.ancho / pl.ancho, c.y + c.alto]
            : g === 180 ? [c.x + c.ancho - p * c.ancho / pl.ancho, c.y, c.x + c.ancho - p * c.ancho / pl.ancho, c.y + c.alto]
            : g === 90 ? [c.x, c.y + c.alto - p * c.alto / pl.ancho, c.x + c.ancho, c.y + c.alto - p * c.alto / pl.ancho]
            : [c.x, c.y + p * c.alto / pl.ancho, c.x + c.ancho, c.y + p * c.alto / pl.ancho];
          // Se puede arrastrar (ver conectarPaginador): la línea ancha invisible es la que se toma.
          const datos = `data-pliegue="${i}" data-giro="${g}" data-c="${c.x},${c.y},${c.ancho},${c.alto}" data-espejo="${espejo ? 1 : 0}" data-ancho="${pl.ancho}" data-h="${H}"`;
          partes.push(`<g class="pliegue" ${datos} style="cursor:${g % 180 === 0 ? "ew-resize" : "ns-resize"};touch-action:none"><line x1="${x1}" y1="${y(y1)}" x2="${x2}" y2="${y(y2)}" stroke="#c03ac0" stroke-width="${W / 700}" stroke-dasharray="${W / 200} ${W / 300}"/><line x1="${x1}" y1="${y(y1)}" x2="${x2}" y2="${y(y2)}" stroke="transparent" stroke-width="${W / 60}" pointer-events="stroke"/></g>`);
        }
      }
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
  for (const l of m.corte) partes.push(`<line x1="${l.x1}" y1="${y(l.y1)}" x2="${l.x2}" y2="${y(l.y2)}" stroke="${tinta}" stroke-width="${trazo * 1.2}"/>`);
  for (const l of m.pliegues || []) partes.push(`<line x1="${l.x1}" y1="${y(l.y1)}" x2="${l.x2}" y2="${y(l.y2)}" stroke="#c03ac0" stroke-width="${trazo * 1.5}" stroke-dasharray="${W / 300}"/>`);
  for (const g of m.registro) partes.push(`<g stroke="${tinta}" stroke-width="${trazo}" fill="none"><circle cx="${g.x}" cy="${y(g.y)}" r="${g.radio * 0.6}"/><line x1="${g.x - g.radio}" y1="${y(g.y)}" x2="${g.x + g.radio}" y2="${y(g.y)}"/><line x1="${g.x}" y1="${y(g.y) - g.radio}" x2="${g.x}" y2="${y(g.y) + g.radio}"/></g>`);
  for (const p of m.tira_color) {
    const [c, mg, a, k] = p.cmyk;
    let rgb = [(1 - c) * (1 - k), (1 - mg) * (1 - k), (1 - a) * (1 - k)].map((v) => Math.round(v * 255));
    // A una tinta la tira lleva solo negro (como pdf::dibujar_marcas).
    if (tintasCara === 1) rgb = Array(3).fill(Math.round(0.3 * rgb[0] + 0.59 * rgb[1] + 0.11 * rgb[2]));
    partes.push(r(p.rect, `fill="rgb(${rgb})"`));
  }
  for (const a of m.alzado || []) partes.push(r(a, `fill="${tinta}"`));
  // Rótulo de la plancha, como lo escribe el motor (pdf::dibujar_rotulo).
  if (m.rotulo && opciones.titulo !== undefined) {
    const ro = m.rotulo;
    const colores = tintasCara === 1 ? [["NEGRO", "#1d1d1f"]] : [["CYAN", "#00a3e0"], ["MAGENTA", "#e5007e"], ["AMARILLO", "#f2d500"], ["NEGRO", "#1d1d1f"]];
    const tspans = colores.map(([t, c], i) => `${i ? `<tspan fill="${tinta}"> / </tspan>` : ""}<tspan fill="${c}">${t}</tspan>`).join("");
    const lado = opciones.ladoCara?.(cara) || "";
    const yy = y(ro.y);
    const giro = ro.vertical ? ` transform="rotate(-90 ${ro.x} ${yy})"` : "";
    partes.push(`<text x="${ro.x}" y="${yy}" font-size="${ro.alto * 1.25}" font-family="ui-monospace, monospace" textLength="${Math.min(ro.largo, ro.alto * (colores.map(([t]) => t).join(" / ") + "   " + opciones.titulo + "   " + lado).length)}" lengthAdjust="spacingAndGlyphs"${giro}>${tspans}<tspan fill="${tinta}">   ${esc(opciones.titulo.toUpperCase())}   ${esc(lado)}</tspan></text>`);
  }
  const etiqueta = `${esc(cara.nombre)}: pliego de ${mm(W, 0)} por ${mm(H, 0)} mm`;
  // El pliego se ve siempre horizontal (como se acostumbra en el taller): si va
  // vertical en la máquina, se gira 90° y la pinza queda a la izquierda.
  if (H > W) {
    return `<svg viewBox="${-H * 0.01} ${-W * 0.01} ${H * 1.02} ${W * 1.02}" role="img" aria-label="${etiqueta}, pinza en el lado corto"><g transform="matrix(0 1 -1 0 ${H} 0)">${partes.join("")}</g></svg>`;
  }
  return `<svg viewBox="${-W * 0.01} ${-H * 0.01} ${W * 1.02} ${H * 1.02}" role="img" aria-label="${etiqueta}">${partes.join("")}</svg>`;
}

function tarjetaVistaPrevia(trabajo, maquina, titulo, insignias = false) {
  const caras = trabajo.plan?.caras || trabajo.plan?.plan?.caras;
  if (!caras?.length) {
    return `<section class="tarjeta vista-previa"><div class="lienzo"><div class="vacio"><span class="orbe orbe-respira" aria-hidden="true"></span><p>${titulo}</p></div></div></section>`;
  }
  const i = Math.min(trabajo.cara, caras.length - 1);
  const muchas = caras.length > 14;
  const guias = estado.preferencias.vistaGuias === true;
  return `<section class="tarjeta vista-previa">
    <div class="vista-previa-cabeza">
      <h3>${esc(caras[i].nombre)} <span class="tenue num">· ${i + 1} de ${caras.length}</span></h3>
      <div class="paginador">
        <button class="boton boton-claro boton-chico" data-cara="${i - 1}" ${i === 0 ? "disabled" : ""} aria-label="Pliego anterior">←</button>
        ${muchas ? "" : caras.map((_, k) => `<button class="chip" data-cara="${k}" aria-pressed="${k === i}">${k + 1}</button>`).join("")}
        <button class="boton boton-claro boton-chico" data-cara="${i + 1}" ${i === caras.length - 1 ? "disabled" : ""} aria-label="Pliego siguiente">→</button>
      </div>
    </div>
    <div class="lienzo">${svgCara(caras[i], maquina, { info: trabajo.info, mini: trabajo.mini, insignias: insignias && guias, clave: i, guias, modos: (k) => modosPagina(trabajo, k), fondos: trabajo.fondos, extendido: modoRebase() === "extendido", miniFondo: trabajo.miniFondo, tintas: tintasDe(trabajo.op), plegable: trabajo === estado.piezas && trabajo.op.modo === "repetir" && trabajo.info?.formato ? { ...plegableDe(trabajo.op, trabajo.info.formato), ancho: trabajo.info.formato.ancho, dorso: trabajo.op.dorso } : null, titulo: base(trabajo.archivo?.nombre) || "", ladoCara: (c) => ladosDe(caras).get(c) })}</div>
    <div class="leyenda">${chips("vista-guias", [["pdf", "Como queda el PDF"], ["guias", "Con guías"]], guias ? "guias" : "pdf")}
      ${guias ? `<span><i style="border-color:var(--tinta)"></i>Corte</span><span><i style="border-color:#c03ac0;border-top-style:dashed"></i>Pliegue</span><span><i style="border-color:var(--gris);border-top-style:dashed"></i>Área imprimible</span><span><i style="border-color:var(--naranja)"></i>Pinza</span>` : ""}</div>
  </section>`;
}

function conectarPaginador(raiz, trabajo, redibujar) {
  $$("[data-cara]", raiz).forEach((b) => b.addEventListener("click", () => { trabajo.cara = Number(b.dataset.cara); redibujar(); }));
  // Pliegues del plegable: se arrastran sobre la vista previa y quedan como anchos propios.
  $$("[data-pliegue]", raiz).forEach((grupo) => grupo.addEventListener("pointerdown", (e) => {
    e.preventDefault();
    const pl = plegableDe(trabajo.op, trabajo.info?.formato);
    if (!pl) return;
    const i = Number(grupo.dataset.pliegue);
    const giro = Number(grupo.dataset.giro);
    const [cx, cy, cw, ch] = grupo.dataset.c.split(",").map(Number);
    const ancho = Number(grupo.dataset.ancho), H = Number(grupo.dataset.h);
    const espejo = grupo.dataset.espejo === "1";
    const padre = grupo.parentNode;
    const inicio = new DOMPoint(e.clientX, e.clientY).matrixTransform(padre.getScreenCTM().inverse());
    // Distancia del pliegue al borde izquierdo del exterior (mm) para un punto del pliego.
    const distancia = (pt) => {
      const yPdf = H - pt.y;
      let p = giro === 0 ? (pt.x - cx) * ancho / cw : giro === 180 ? (cx + cw - pt.x) * ancho / cw
        : giro === 90 ? (cy + ch - yPdf) * ancho / ch : (yPdf - cy) * ancho / ch;
      if (espejo) p = ancho - p;
      const antes = i > 0 ? pl.pliegues[i - 1] : 0, despues = i < pl.pliegues.length - 1 ? pl.pliegues[i + 1] : ancho;
      return Math.round(Math.min(despues - 5, Math.max(antes + 5, p)) * 10) / 10;
    };
    let ultimo = pl.pliegues[i];
    grupo.setPointerCapture(e.pointerId);
    const mover = (ev) => {
      const pt = new DOMPoint(ev.clientX, ev.clientY).matrixTransform(padre.getScreenCTM().inverse());
      ultimo = distancia(pt);
      const dx = pt.x - inicio.x, dy = pt.y - inicio.y;
      grupo.setAttribute("transform", giro % 180 === 0 ? `translate(${dx} 0)` : `translate(0 ${dy})`);
      grupo.querySelector("line").setAttribute("stroke", "#e0218a");
    };
    const soltar = () => {
      grupo.removeEventListener("pointermove", mover);
      grupo.removeEventListener("pointerup", soltar);
      grupo.removeEventListener("pointercancel", soltar);
      const nuevos = pl.pliegues.map((v, k) => (k === i ? ultimo : v));
      const bordes = [0, ...nuevos, ancho];
      trabajo.op.anchos = bordes.slice(1).map((v, k) => (v - bordes[k]).toFixed(1)).join(" ");
      avisar(`Pliegue ${i + 1} a ${mm(ultimo, 1)} mm del borde izquierdo del exterior`);
      const vista = $("#vista");
      if (trabajo === estado.piezas && vista) vistaPiezas(vista); else redibujar();
    };
    grupo.addEventListener("pointermove", mover);
    grupo.addEventListener("pointerup", soltar);
    grupo.addEventListener("pointercancel", soltar);
  }));
  $$('[data-chips="vista-guias"]', raiz).forEach((g) => g.addEventListener("click", (e) => {
    const b = e.target.closest(".chip");
    if (!b) return;
    e.stopPropagation(); // no es una opción del montaje
    preferir("vistaGuias", b.dataset.valor === "guias");
    redibujar();
  }));
}

// ───────────── Tamaños de papel (pliego de impresión) ─────────────
const FORMATOS_PAPEL = [
  ["carta", "Carta", 216, 279], ["oficio", "Oficio", 216, 330], ["tabloide", "Tabloide", 279, 432],
  ["tabloide_extra", "Tabloide extra", 330, 480], ["sra3", "SRA3", 320, 450], ["cuarto", "1/4 de pliego", 350, 500],
  ["tercio", "1/3 de pliego", 330, 700], ["medio", "1/2 pliego", 500, 700], ["pliego", "Pliego", 700, 1000],
];
const cm = (v) => mm(v / 10, 1);

/** El tamaño girado como el pliego máximo de la máquina (apaisado o vertical). */
function orientarPapel(f, maquina) {
  const apaisada = maquina.pliego_max.ancho >= maquina.pliego_max.alto;
  const [a, b] = [Math.max(f.ancho, f.alto), Math.min(f.ancho, f.alto)];
  return apaisada ? { ...f, ancho: a, alto: b } : { ...f, ancho: b, alto: a };
}
function cabeEnMaquina(f, m) {
  const min = m.pliego_min;
  return f.ancho <= m.pliego_max.ancho + 0.01 && f.alto <= m.pliego_max.alto + 0.01 && (!min || (Math.max(f.ancho, f.alto) >= Math.max(min.ancho, min.alto) - 0.01 && Math.min(f.ancho, f.alto) >= Math.min(min.ancho, min.alto) - 0.01));
}
/** Tamaños que caben en la máquina: el máximo, los comunes, los del papel elegido y los propios. */
function formatosParaMaquina(maquina, papel) {
  if (!maquina) return [];
  const max = maquina.pliego_max;
  const igual = FORMATOS_PAPEL.find(([, , a, b]) => Math.abs(Math.max(a, b) - Math.max(max.ancho, max.alto)) < 1 && Math.abs(Math.min(a, b) - Math.min(max.ancho, max.alto)) < 1);
  const lista = [{ id: "maquina", nombre: igual ? `${igual[1]} (máximo de la máquina)` : "Máximo de la máquina", ancho: max.ancho, alto: max.alto }];
  const extra = [
    ...FORMATOS_PAPEL.map(([id, nombre, a, b]) => ({ id, nombre, ancho: a, alto: b })),
    ...(papel?.pliegos || []).map((t) => ({ id: `${t.ancho}x${t.alto}`, nombre: `Del papel ${papel.nombre}`, ancho: t.ancho, alto: t.alto })),
    ...(estado.preferencias.formatosPapel || []).map((t) => ({ id: `${t.ancho}x${t.alto}`, nombre: t.nombre || "Propio", ancho: t.ancho, alto: t.alto })),
  ];
  for (const f of extra.map((x) => orientarPapel(x, maquina))) {
    if (!cabeEnMaquina(f, maquina)) continue;
    if (lista.some((x) => Math.abs(x.ancho - f.ancho) < 1 && Math.abs(x.alto - f.alto) < 1)) continue;
    lista.push(f);
  }
  return lista;
}
/** Pliego pedido al motor (null = máximo de la máquina). */
function pliegoDe(o, maquina) {
  if (!maquina || !o.papelTam || o.papelTam === "maquina") return null;
  if (o.papelTam === "otro") {
    const a = num(o.pliegoAncho), b = num(o.pliegoAlto);
    return a > 0 && b > 0 ? orientarPapel({ ancho: a, alto: b }, maquina) : null;
  }
  const f = formatosParaMaquina(maquina, null).find((x) => x.id === o.papelTam)
    || (/^\d+(\.\d+)?x\d+(\.\d+)?$/.test(o.papelTam) ? orientarPapel({ ancho: Number(o.papelTam.split("x")[0]), alto: Number(o.papelTam.split("x")[1]) }, maquina) : null);
  return f ? { ancho: f.ancho, alto: f.alto } : null;
}
function selectorPapel(prefijo, o, maquina, papel) {
  if (!maquina) return "";
  const lista = formatosParaMaquina(maquina, papel);
  const valor = o.papelTam || "maquina";
  return `<div class="campo"><span>Tamaño del papel (pliego)</span>
    <select id="${prefijo}-papel-tam">${lista.map((f) => `<option value="${esc(f.id)}" ${f.id === valor ? "selected" : ""}>${esc(f.nombre)} · ${cm(f.ancho)} × ${cm(f.alto)} cm</option>`).join("")}
      <option value="otro" ${valor === "otro" ? "selected" : ""}>Otro tamaño…</option></select>
    ${valor === "otro" ? `<div class="fila-3">
      <label class="campo"><span>Ancho (mm)</span><input type="number" id="${prefijo}-pliego-ancho" value="${o.pliegoAncho ?? ""}"></label>
      <label class="campo"><span>Alto (mm)</span><input type="number" id="${prefijo}-pliego-alto" value="${o.pliegoAlto ?? ""}"></label>
      <button class="boton boton-claro boton-chico" type="button" id="${prefijo}-guardar-tam" style="align-self:end">Guardar tamaño</button></div>` : ""}
    <small>Solo aparecen los tamaños que caben en ${esc(maquina.nombre)}. Abajo se comparan para ver cuál gasta menos papel.</small>
    ${bloquePinza(prefijo, o, maquina)}</div>`;
}
/** ¿El papel entra en la máquina tanto horizontal como vertical? */
/**
 * Pinza: de qué lado del papel muerde (lado largo = papel horizontal en la
 * máquina; lado corto = vertical) y cuánto mide en este trabajo.
 */
/** Tintas del tiro y del retiro: «4x4», «4x1», «1x1»… (a 1 tinta todo pasa a negro). */
function campoTintas(o, conRetiro) {
  const opciones = conRetiro
    ? [["4x4", "4 × 4 · color los dos lados"], ["4x1", "4 × 1 · retiro en negro"], ["1x4", "1 × 4 · tiro en negro"], ["1x1", "1 × 1 · todo en negro"]]
    : [["4x4", "Color (4 tintas)"], ["1x1", "Una tinta (negro)"]];
  const valor = opciones.some(([v]) => v === o.tintas) ? o.tintas : conRetiro ? "4x4" : (o.tintas || "4x4").startsWith("1") ? "1x1" : "4x4";
  return `<div class="campo"><span>Tintas${conRetiro ? " (tiro × retiro)" : ""}</span>${chips("tintas", opciones, valor)}
    <small>Cada plancha sale rotulada con sus tintas (cada nombre en su color), el archivo y si es tiro o retiro. A una tinta, todo el diseño pasa a negro.</small></div>`;
}
/** «TIRO 1», «RETIRO 1»… de cada cara, como pdf::lados_de. */
function ladosDe(caras) {
  const mapa = new Map();
  let n = 0;
  for (const c of caras) {
    const nombre = c.nombre.toLowerCase();
    if (nombre.includes("tira y retira")) mapa.set(c, `TIRA Y RETIRA ${++n}`);
    else if (nombre.trim().endsWith("retiro")) mapa.set(c, `RETIRO ${Math.max(n, 1)}`);
    else mapa.set(c, `TIRO ${++n}`);
  }
  return mapa;
}
/** Tintas del tiro y del retiro de un trabajo. */
function tintasDe(o) {
  const [t, r] = String(o?.tintas || "4x4").split("x").map((v) => (v === "1" ? 1 : 4));
  return { tintas_tiro: t, tintas_retiro: r ?? t };
}

function bloquePinza(prefijo, o, maquina) {
  const papel = pliegoDe(o, maquina) || maquina.pliego_max;
  const ambas = ambasOrientaciones(papel, maquina);
  const unica = comoPapel(cabeEnMaquina({ ancho: Math.max(papel.ancho, papel.alto), alto: Math.min(papel.ancho, papel.alto) }, maquina) ? { ancho: 2, alto: 1 } : { ancho: 1, alto: 2 });
  const propia = maquina.pinza;
  const original = estado.maquinas.find((m) => m.id === maquina.id)?.pinza ?? propia;
  return `<div class="campo"><span>Pinza</span>
      ${ambas ? chips(`${prefijo}-orientacion-papel`, [["auto", "Automático (lo que más rinda)"], ["horizontal", "Horizontal · lado largo"], ["vertical", "Vertical · lado corto"]], o.orientacionPapel || "auto")
        : `<small>En ${esc(maquina.nombre)} este papel solo entra con la pinza en el lado ${unica === "horizontal" ? "largo (horizontal)" : "corto (vertical)"}.</small>`}
    </div>
    <label class="campo"><span>Tamaño de la pinza (mm)</span><input type="number" min="0" max="60" step="0.5" id="${prefijo}-pinza" value="${propia}">
      <small>${Math.abs(propia - original) > 0.01 ? `Cambiada para este trabajo; la de la máquina es ${mm(original)} mm.` : "La de la máquina; cámbiala si este trabajo necesita otra."}</small></label>`;
}
const ambasOrientaciones = (t, m) => cabeEnMaquina({ ancho: t.ancho, alto: t.alto }, m) && cabeEnMaquina({ ancho: t.alto, alto: t.ancho }, m);
const orientacionPapel = (o) => o.orientacionPapel || "auto";
/** «horizontal» o «vertical» según cómo quedó el pliego. */
const comoPapel = (t) => (t.ancho >= t.alto ? "horizontal" : "vertical");
function conectarSelectorPapel(prefijo, o, alCambiar) {
  $$(`[data-chips="${prefijo}-orientacion-papel"]`).forEach((g) => g.addEventListener("click", (e) => {
    const b = e.target.closest(".chip");
    if (!b) return;
    e.stopPropagation();
    $$(".chip", g).forEach((c) => c.setAttribute("aria-pressed", String(c === b)));
    o.orientacionPapel = b.dataset.valor;
    alCambiar(false);
  }));
  $(`#${prefijo}-papel-tam`)?.addEventListener("change", (e) => { e.stopPropagation(); o.papelTam = e.target.value; alCambiar(true); });
  $(`#${prefijo}-pinza`)?.addEventListener("change", (e) => {
    e.stopPropagation();
    const v = num(e.target.value, -1);
    o.pinza = e.target.value === "" || v < 0 ? null : v;
    o.pinzaDe = $(`#${prefijo}-maquina`)?.value ?? estado.preferencias.maquina;
    alCambiar(true);
  });
  for (const lado of ["ancho", "alto"]) {
    $(`#${prefijo}-pliego-${lado}`)?.addEventListener("change", (e) => {
      e.stopPropagation();
      o[lado === "ancho" ? "pliegoAncho" : "pliegoAlto"] = e.target.value || null;
      alCambiar(false);
    });
  }
  $(`#${prefijo}-guardar-tam`)?.addEventListener("click", () => {
    const a = num(o.pliegoAncho), b = num(o.pliegoAlto);
    if (!(a > 0 && b > 0)) { avisar("Escribe ancho y alto"); return; }
    const nombre = prompt("Nombre para este tamaño", `${cm(a)} × ${cm(b)}`) || `${cm(a)} × ${cm(b)}`;
    preferir("formatosPapel", [...(estado.preferencias.formatosPapel || []).filter((x) => !(x.ancho === a && x.alto === b)), { nombre, ancho: a, alto: b }]);
    o.papelTam = `${a}x${b}`;
    avisar("Tamaño guardado");
    alCambiar(true);
  });
}

/** Tabla que compara tamaños de papel: cuál usa menos pliegos y menos área. */
function tarjetaComparar(prefijo, filas, actual, digital) {
  if (filas.length < 2) return "";
  const ordenadas = [...filas].sort((a, b) => (digital ? a.pliegos - b.pliegos || a.area - b.area : a.area - b.area || a.pliegos - b.pliegos));
  const mejor = ordenadas[0];
  return `<section class="tarjeta paso comparar-papel">
    <div class="paso-titulo"><h3>¿En qué papel sale mejor?</h3></div>
    <p class="tenue" style="font-size:14px">${digital ? "En digital se cobra por pliego impreso: gana el que imprime menos pliegos." : "En offset el costo va con el área de papel: gana el que gasta menos papel."} Mejor opción: <b>${esc(mejor.nombre)} (${cm(mejor.ancho)} × ${cm(mejor.alto)} cm)</b>.</p>
    <div class="tabla-desliza"><table class="tabla"><thead><tr><th>Papel</th><th>${filas[0].etiquetaCabe}</th><th>${filas[0].etiquetaPliegos}</th><th>Papel usado</th><th>Aprovecha</th><th></th></tr></thead><tbody>
      ${ordenadas.map((f) => `<tr class="${f.id === actual ? "fila-actual" : ""}">
        <td><b>${esc(f.nombre)}</b><br><span class="tenue">${cm(f.ancho)} × ${cm(f.alto)} cm</span></td>
        <td>${f.cabe}</td><td class="num">${f.pliegosTexto}</td><td class="num">${mm(f.area, 3)} m²</td><td class="num">${mm(f.aprovecha, 0)} %</td>
        <td>${f.id === actual ? `<span class="sello sello-ok">En uso</span>` : `<button class="boton boton-claro boton-chico" type="button" data-usar-papel="${esc(f.id)}">Usar</button>`}${f === mejor && f.id !== actual ? ` <span class="etiqueta">mejor</span>` : ""}</td>
      </tr>`).join("")}
    </tbody></table></div>
  </section>`;
}

// ───────────── Escala del arte ─────────────
const escalaDe = (o) => Math.min(400, Math.max(10, Number(o.escala) || 100)) / 100;
function campoEscala(prefijo, o, formato) {
  const e = escalaDe(o);
  const tam = formato ? ` → queda de <b>${cm(formato.ancho * e)} × ${cm(formato.alto * e)} cm</b>` : "";
  return `<div class="campo"><span>Escala del arte (%)</span>
    <div class="fila-escala"><input type="number" id="${prefijo}-escala" min="10" max="400" step="1" value="${Math.round(e * 100)}">
      ${e !== 1 ? `<button type="button" class="boton boton-claro boton-chico" id="${prefijo}-escala-100">Volver a 100 %</button>` : ""}</div>
    <small>${formato ? `Tamaño original ${cm(formato.ancho)} × ${cm(formato.alto)} cm${tam}.` : "Reduce el arte para que entren más por pliego."}</small>
    <div id="${prefijo}-escala-sugerencia"></div></div>`;
}
function conectarEscala(prefijo, o, alCambiar) {
  $(`#${prefijo}-escala`)?.addEventListener("change", (e) => {
    e.stopPropagation();
    o.escala = Math.min(400, Math.max(10, Math.round(num(e.target.value, 100))));
    alCambiar();
  });
  $(`#${prefijo}-escala-100`)?.addEventListener("click", () => { o.escala = 100; alCambiar(); });
}
/**
 * Busca la menor reducción (de 99 % hacia 70 %) que mejora el montaje.
 * `medir(escala)` devuelve un número a maximizar (piezas, o −pliegos) o null.
 */
function sugerirEscala(prefijo, o, formato, medir, describir, alUsar) {
  const caja = $(`#${prefijo}-escala-sugerencia`);
  if (!caja || !formato) return;
  const actual = escalaDe(o);
  const base = medir(actual);
  let html = "";
  if (base != null) {
    for (let pct = Math.round(actual * 100) - 1; pct >= 70; pct--) {
      const v = medir(pct / 100);
      if (v != null && v > base + 1e-9) {
        html = `<div class="aviso-dobles">Al <b>${pct} %</b> ${describir(v, base)}: el arte queda de <b>${cm(formato.ancho * pct / 100)} × ${cm(formato.alto * pct / 100)} cm</b>.
          <button type="button" class="boton boton-naranja boton-chico" data-usar-escala="${pct}">Usar ${pct} %</button></div>`;
        break;
      }
    }
  }
  if (!pintar(caja, html)) return;
  $("[data-usar-escala]", caja)?.addEventListener("click", (e) => { o.escala = Number(e.currentTarget.dataset.usarEscala); alUsar(); });
}

function conectarComparar(caja, o, redibujar) {
  $$("[data-usar-papel]", caja).forEach((b) => b.addEventListener("click", () => { o.papelTam = b.dataset.usarPapel; redibujar(); }));
}

/** Cuántos pliegos salen con las páginas derechas y con las páginas giradas. */
function pintarOrientaciones(maquina) {
  const caja = $("#lb-orientaciones");
  const t = estado.libro;
  if (!caja) return;
  if (!t.info || !t.plan || !maquina) { caja.textContent = ""; return; }
  const base = peticionLibro(maquina, t.info);
  const prueba = (o) => {
    try {
      const r = JSON.parse(motor.planear_libro(JSON.stringify({ ...base, orientacion_paginas: o })));
      const pp = Math.round(r.plan.paginas_libro / r.plan.pliegos_por_ejemplar);
      return `${mm(r.plan.pliegos_por_ejemplar, 2)} pliegos por libro (${pp} págs. por pliego)`;
    } catch { return "no entran"; }
  };
  caja.textContent = `Derechas: ${prueba("normal")} · Giradas 90°: ${prueba("girada")}.`;
}

function pintarCompararLibro() {
  const caja = $("#lb-comparar");
  if (!caja) return;
  const t = estado.libro, o = t.op;
  const maquina = maquinaElegida("lb-maquina");
  if (!t.info || !t.plan || !maquina) { pintar(caja, ""); return; }
  const base = peticionLibro(maquina, t.info);
  const papel = estado.papeles.find((p) => p.id === $("#lb-papel")?.value);
  const f0 = base.formato;
  const filas = [];
  for (const f of formatosParaMaquina(maquina, papel)) {
    try {
      const r = JSON.parse(motor.planear_libro(JSON.stringify({ ...base, pliego: f.id === "maquina" ? null : { ancho: f.ancho, alto: f.alto } })));
      const pl = r.plan, pliegos = pl.pliegos_por_ejemplar;
      const porPliego = Math.round(pl.paginas_libro / pliegos);
      filas.push({
        ...f, ancho: r.pliego.ancho, alto: r.pliego.alto, nombre: `${f.nombre} · ${comoPapel(r.pliego)}`, pliegos, area: (pliegos * f.ancho * f.alto) / 1e6,
        aprovecha: (100 * pl.paginas_libro * f0.ancho * f0.alto) / (2 * pliegos * f.ancho * f.alto),
        cabe: `${porPliego} págs. (${porPliego / 2} por cara)`, etiquetaCabe: "Páginas por pliego",
        pliegosTexto: mm(pliegos, 2), etiquetaPliegos: "Pliegos por libro",
      });
    } catch { /* no cabe ni una firma */ }
  }
  const actual = filas.find((f) => f.id === (o.papelTam || "maquina"))?.id || (o.papelTam || "maquina");
  if (!pintar(caja, tarjetaComparar("lb", filas, actual, maquina.tipo === "digital"))) return;
  conectarComparar(caja, o, () => { t.cara = 0; vistaLibro($("#vista")); });
}

function pintarCompararPiezas() {
  const caja = $("#pz-comparar");
  if (!caja) return;
  const t = estado.piezas, o = t.op;
  const maquina = maquinaElegida("pz-maquina");
  if (o.modo !== "repetir" || !t.info?.formato || !t.plan || !maquina) { pintar(caja, ""); return; }
  const base = peticionPiezas(maquina, t.info);
  const f0 = t.info.formato;
  const filas = [];
  for (const f of formatosParaMaquina(maquina, null)) {
    try {
      const r = JSON.parse(motor.planear_nup(JSON.stringify({ ...base, pliego: f.id === "maquina" ? null : { ancho: f.ancho, alto: f.alto } })));
      const n = r.distribucion.columnas * r.distribucion.filas;
      const pliegos = Math.ceil(1000 / n);
      filas.push({
        ...f, ancho: r.pliego.ancho, alto: r.pliego.alto, nombre: `${f.nombre} · ${comoPapel(r.pliego)}`, pliegos, area: (pliegos * f.ancho * f.alto) / 1e6, aprovecha: (100 * n * f0.ancho * f0.alto) / (f.ancho * f.alto),
        cabe: `${n} piezas`, etiquetaCabe: "Caben", pliegosTexto: mm(pliegos, 0), etiquetaPliegos: "Pliegos por 1.000",
      });
    } catch { /* no cabe */ }
  }
  const actual = o.papelTam || "maquina";
  if (!pintar(caja, tarjetaComparar("pz", filas, actual, maquina.tipo === "digital"))) return;
  conectarComparar(caja, o, () => vistaPiezas($("#vista")));
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
            <label class="campo"><span>Rebase (mm)</span><input type="number" step="0.5" min="3" id="pz-rebase" value="${o.rebase}"></label>
            <label class="campo"><span>Calle (mm)</span><input type="number" step="0.5" min="0" id="pz-calle" value="${o.calle}"><small>${o.calle >= 2 * o.rebase ? "Doble corte: cada pieza con su rebase" : o.calle > 0 ? "Calle menor que 2 × rebase: el rebase se recorta" : "Corte compartido (sencillo)"}</small></label>
          </div>
          <div class="campo"><span>Corte entre piezas</span>${chips("corte", [["doble", "Doble corte (calle con rebase)"], ["sencillo", "Corte sencillo (compartido)"]], o.calle > 0 ? "doble" : "sencillo")}
            <small>Doble corte: entre pieza y pieza queda una calle de ${mm(2 * o.rebase)} mm con el rebase de cada una; si la cuchilla se corre, no se ve la pieza vecina.</small></div>
          <div class="campo"><span>Orientación</span>${chips("orientacion", [["auto", "Automática"], ["normal", "Normal"], ["girada", "Girada 90°"]], o.orientacion)}</div>
          ${o.modo === "repetir" ? bloquePlegable(t) : ""}
          ${campoEscala("pz", o, t.info?.formato)}
          ${selectorPapel("pz", o, maquinaElegida("pz-maquina"), null)}
          ${interruptor("pz-dorso", "Frente y dorso (páginas en pares)", o.dorso)}
          ${o.dorso ? `<div class="campo"><span>Volteo del pliego</span>${chips("volteo", [["lateral", "Tira y retira (lateral)"], ["cabeza", "De cabeza (tumble)"]], o.volteo)}</div>` : ""}
          ${campoTintas(o, o.dorso)}
          <details class="avanzado"><summary>Marcas y pliego</summary>
            <div class="paso">
              ${interruptor("pz-marcas", "Marcas de corte y registro", o.marcas)}
              ${interruptor("pz-tira", "Tira de control de color", o.tira)}
            </div>
          </details>
          ${bloqueCorrecciones("pz")}
        </section>
      </div>
      <div class="columna-resultado"><div class="resultado" id="pz-resultado"></div><div id="pz-comparar"></div><div id="pz-cotizacion"></div></div>
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
    conectarArchivo("pz-archivo", (a) => { analizarArchivo(t, a); aplicarPlegable(t); if (Number(o.cuerpos) && t.info?.paginas.length === 2) o.dorso = true; vistaPiezas(main); });
    for (const id of ["pz-compensacion", "pz-anchos"]) {
      $(`#${id}`)?.addEventListener("change", (e) => {
        e.stopPropagation();
        if (id === "pz-anchos") o.anchos = e.target.value.trim(); else o.compensacion = num(e.target.value, 2);
        vistaPiezas(main);
      });
    }
  }
  $("#pz-cambiar")?.addEventListener("click", () => { t.archivo = null; t.info = null; t.plan = null; t.error = null; vistaPiezas(main); });
  const leerOpciones = () => {
    o.rebase = rebaseMinimo(num($("#pz-rebase").value, 3));
    o.calle = num($("#pz-calle").value, 0);
    o.dorso = $("#pz-dorso").checked;
    o.marcas = $("#pz-marcas").checked;
    o.tira = $("#pz-tira").checked;
    if ($("#pz-maquina")) preferir("maquina", $("#pz-maquina").value);
  };
  conectarCorrecciones(main, () => vistaPiezas(main));
  conectarSelectorPapel("pz", o, (redibujar) => (redibujar ? vistaPiezas(main) : calcularPiezas()));
  conectarEscala("pz", o, () => vistaPiezas(main));
  $$("input, select", main).forEach((el) => el.addEventListener("change", () => {
    if (el.type === "file" || el.dataset.correccion || el.dataset.cantidad || /papel-tam|pliego-(ancho|alto)|-escala|-pinza$|pz-compensacion|pz-anchos/.test(el.id)) return;
    const antes = { dorso: o.dorso, rebase: o.rebase, maquina: estado.preferencias.maquina };
    leerOpciones();
    if (antes.rebase !== o.rebase || antes.maquina !== estado.preferencias.maquina) { revisarArchivo(t); vistaPiezas(main); return; }
    if (antes.dorso !== o.dorso) vistaPiezas(main); else calcularPiezas();
  }));
  conectarChips(main, (n, v) => {
    if (n === "corte") { o.calle = v === "doble" ? 2 * o.rebase : 0; vistaPiezas(main); return; }
    if (n === "cuerpos" || n === "plegado" || n === "archivoPlegable") {
      o[n] = v;
      if (n === "cuerpos") o.anchos = "";
      aplicarPlegable(t);
      // Un plegable abierto lleva exterior e interior: frente y dorso.
      if (Number(o.cuerpos) && t.info?.paginas.length % 2 === 0) o.dorso = true;
      vistaPiezas(main);
      return;
    }
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
    t.error = `No se pudieron juntar los PDF: ${mensaje(e)}`;
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
  const pliego = pliegoDe(o, maquina);
  return {
    maquina, formato: info.formato, paginas: info.paginas.length, pliego, orientacion_papel: orientacionPapel(o), escala: escalaDe(o),
    rebase: o.rebase, calle: o.calle, orientacion: orientacion || o.orientacion,
    dorso: o.dorso, volteo: o.volteo, marcas: o.marcas, tira_color: o.tira,
    titulo: base(estado.piezas.archivo?.nombre), fecha: ahora(), correcciones: correcciones(),
    pliegues: o.modo === "repetir" ? plegableDe(o, info.formato)?.pliegues || [] : [],
  };
}

// ───────────── Plegables (2, 3 o 4 cuerpos) ─────────────
const PLEGADOS = {
  2: [["diptico", "Díptico (un pliegue al centro)"]],
  3: [["envolvente", "Envolvente (carta)"], ["acordeon", "Acordeón (Z)"], ["ventana", "Ventana"]],
  4: [["acordeon", "Acordeón (zigzag)"], ["envolvente", "Envolvente"], ["ventana", "Ventana doble (puerta)"]],
};
/**
 * Anchos de cada cuerpo del exterior, de izquierda a derecha, para un ancho
 * abierto W. En el envolvente y la ventana los cuerpos que se meten adentro
 * van más angostos (compensación c) para que el plegable cierre sin abombarse.
 */
function anchosCuerpos(n, tipo, W, c) {
  if (tipo === "envolvente" && n > 2) {
    // [w − (n−2)c, …, w − c, w, w]: el más angosto es la solapa que entra primero.
    const w = (W + (c * (n - 2) * (n - 1)) / 2) / n;
    return Array.from({ length: n }, (_, i) => (i < n - 2 ? w - (n - 2 - i) * c : w));
  }
  if (tipo === "ventana" && n === 3) {
    const b = (W + 2 * c) / 2;
    return [b / 2 - c, b, b / 2 - c];
  }
  if (tipo === "ventana" && n === 4) {
    const b = (W + 2 * c) / 4;
    return [b - c, b, b, b - c];
  }
  return Array(n).fill(W / n);
}
/** Plegable de un trabajo: cuerpos, anchos, pliegues (mm desde el borde izquierdo del exterior) y tamaño cerrado. */
function plegableDe(o, formato) {
  const n = Number(o.cuerpos) || 0;
  if (!n || !formato) return null;
  const tipo = PLEGADOS[n].some(([v]) => v === o.plegado) ? o.plegado : PLEGADOS[n][0][0];
  const W = formato.ancho;
  const c = num(o.compensacion, 2);
  let anchos = anchosCuerpos(n, tipo, W, c);
  let propios = false;
  const escritos = String(o.anchos || "").split(/[;|\s]+/).map((v) => num(v, NaN)).filter((v) => v > 0);
  if (escritos.length === n && Math.abs(escritos.reduce((a, b) => a + b, 0) - W) <= 1) { anchos = escritos; propios = true; }
  const pliegues = anchos.slice(0, -1).map((_, i) => anchos.slice(0, i + 1).reduce((a, b) => a + b, 0));
  return { n, tipo, anchos, pliegues, propios, escritosMal: escritos.length > 0 && !propios, cerrado: { ancho: Math.max(...anchos), alto: formato.alto } };
}
function bloquePlegable(t) {
  const o = t.op;
  const n = Number(o.cuerpos) || 0;
  const pl = plegableDe(o, t.info?.formato);
  const r = (v) => mm(v, 1);
  return `<div class="campo"><span>Plegable</span>${chips("cuerpos", [["0", "No"], ["2", "2 cuerpos"], ["3", "3 cuerpos"], ["4", "4 cuerpos"]], String(n))}</div>
    ${n ? `<div class="campo"><span>Tipo de plegado</span>${chips("plegado", PLEGADOS[n], pl?.tipo || PLEGADOS[n][0][0])}</div>
    <div class="campo"><span>El PDF viene</span>${chips("archivoPlegable", [["abierto", "Abierto (exterior e interior)"], ["cuerpos", "Una página por cuerpo"]], o.archivoPlegable || "abierto")}
      <small>${(o.archivoPlegable || "abierto") === "cuerpos" ? `Páginas en orden físico: los ${n} cuerpos del exterior de izquierda a derecha y luego los ${n} del interior de izquierda a derecha. Se unen en una pieza abierta.` : "Página 1 el exterior abierto y página 2 el interior (frente y dorso)."}</small></div>
    <div class="fila">
      ${pl && pl.tipo !== "acordeon" && pl.tipo !== "diptico" ? `<label class="campo"><span>Compensación (mm)</span><input type="number" step="0.5" min="0" max="6" id="pz-compensacion" value="${num(o.compensacion, 2)}"><small>Lo que es más angosto el cuerpo que se mete adentro.</small></label>` : ""}
      <label class="campo"><span>Cuerpos del exterior (mm)</span><input type="text" id="pz-anchos" placeholder="Automático" value="${esc(o.anchos || "")}"><small>De izquierda a derecha, separados por espacios; vacío = automático. También puedes arrastrar los pliegues en la vista previa (Con guías).</small></label>
    </div>
    ${pl ? `<p class="nota">${pl.escritosMal ? `<b>Los anchos escritos no suman ${r(t.info.formato.ancho)} mm:</b> se usan los automáticos. ` : ""}Exterior: ${pl.anchos.map(r).join(" | ")} mm (el interior va al revés). Abierto ${r(t.info.formato.ancho)} × ${r(t.info.formato.alto)} mm · cerrado ${r(pl.cerrado.ancho)} × ${r(pl.cerrado.alto)} mm. Los pliegues se marcan punteados fuera del bloque.</p>` : t.info ? "" : `<p class="nota">Sube el PDF para ver los cuerpos.</p>`}` : ""}`;
}
/** Une los cuerpos si el PDF viene por cuerpos (o vuelve al original). */
function aplicarPlegable(t) {
  const o = t.op;
  if (!t.archivo) return;
  const n = Number(o.cuerpos) || 0;
  const original = t.archivo.original || t.archivo.bytes;
  const quiere = n && o.archivoPlegable === "cuerpos" ? n : 0;
  if ((t.archivo.unido || 0) === quiere) return;
  if (!quiere) { analizarArchivo(t, { nombre: t.archivo.nombre, tamano: t.archivo.tamano, bytes: original }); return; }
  try {
    const bytes = motor.unir_cuerpos(original, n);
    analizarArchivo(t, { nombre: t.archivo.nombre, tamano: t.archivo.tamano, bytes, original, unido: n });
  } catch (e) {
    t.error = `No se pudieron unir los cuerpos: ${mensaje(e)}`;
  }
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
    } catch (e) { errorPlan = mensaje(e); }
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
      if (t.plan?.pliego) explicacion += ` Papel ${comoPapel(t.plan.pliego)} de ${cm(t.plan.pliego.ancho)} × ${cm(t.plan.pliego.alto)} cm.`;
      if (escalaDe(t.op) !== 1 && t.info?.formato) explicacion += ` Arte al <span class="resaltado">${Math.round(escalaDe(t.op) * 100)} %</span>: queda de ${cm(t.info.formato.ancho * escalaDe(t.op))} × ${cm(t.info.formato.alto * escalaDe(t.op))} cm.`;
    } catch (e) { errorPlan = mensaje(e); }
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
  pintarCotizacion("piezas");
  pintarCompararPiezas();
  if (t.info?.formato && t.plan && maquina && t.op.modo === "repetir") {
    const base = peticionPiezas(maquina, t.info);
    sugerirEscala("pz", t.op, t.info.formato, (e) => {
      try { const d = JSON.parse(motor.planear_nup(JSON.stringify({ ...base, escala: e }))).distribucion; return d.columnas * d.filas; } catch { return null; }
    }, (v, b) => `entran <b>${v}</b> por pliego en vez de ${b}`, () => vistaPiezas($("#vista")));
  }
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
        ? motor.generar_combinado(t.archivo.bytes, JSON.stringify({ ...peticionCombinado(maquina, t.info), correcciones: await correccionesParaGenerar(t, t.info.paginas.map((_, i) => i)) }), icc)
        : motor.generar_nup(t.archivo.bytes, JSON.stringify({ ...peticionPiezas(maquina, t.info), correcciones: await correccionesParaGenerar(t, t.info.paginas.map((_, i) => i)) }), icc);
      descargar(r.pdf, `${base(t.archivo.nombre)}-${combinado ? "combinado" : "montaje"}.pdf`);
      const inf = JSON.parse(r.informe);
      const corregidos = inf.avisos.filter((a) => a.startsWith("corregido")).length;
      avisar(`${inf.pdfx ? "PDF listo (con perfil de salida)" : "PDF listo"}${corregidos ? ` · ${corregidos} correcciones aplicadas` : ""}`);
    } catch (err) { avisar(`Error: ${mensaje(err)}`); }
    b.disabled = false; b.textContent = "Descargar PDF listo para imprimir";
  });
}

// ───────────── Páginas del libro: tripa y carátula ─────────────
const ROLES = [
  ["tripa", "Tripa"],
  ["portada", "Portada · tiro"], ["contraportada", "Contraportada · tiro"], ["lomo", "Lomo · tiro"],
  ["solapa_portada", "Solapa de portada · tiro"], ["solapa_contraportada", "Solapa de contraportada · tiro"],
  ["exterior", "Carátula exterior completa · tiro"],
  ["segunda", "2.ª de forros · retiro"], ["tercera", "3.ª de forros · retiro"],
  ["solapa_portada_interior", "Solapa de portada · retiro"], ["solapa_contraportada_interior", "Solapa de contraportada · retiro"],
  ["interior", "Carátula interior completa · retiro"],
  ["guarda_del", "Guarda delantera"], ["guarda_tras", "Guarda trasera"],
  ["excluir", "No usar"],
];
const ROLES_GUARDA = ["guarda_del", "guarda_tras"];
const ROLES_CARATULA = ROLES.map(([r]) => r).filter((r) => r !== "tripa" && r !== "excluir" && !ROLES_GUARDA.includes(r));
const NOMBRE_ROL = Object.fromEntries(ROLES);

/** Propuesta inicial: las páginas de otro tamaño (pliegos extendidos) son la carátula. */
function rolesIniciales(info) {
  const clave = (p) => `${Math.round(p.ancho)}x${Math.round(p.alto)}`;
  const cuenta = {};
  for (const p of info.paginas) cuenta[clave(p)] = (cuenta[clave(p)] || 0) + 1;
  const comun = Object.entries(cuenta).sort((a, b) => b[1] - a[1])[0]?.[0];
  const distintas = info.paginas.map((p, i) => [i, clave(p)]).filter(([, k]) => k !== comun).map(([i]) => i);
  const roles = info.paginas.map(() => "tripa");
  if (distintas[0] != null) roles[distintas[0]] = "exterior";
  if (distintas[1] != null) roles[distintas[1]] = "interior";
  for (const i of distintas.slice(2)) roles[i] = "excluir";
  return roles;
}

const tripaDe = (t) => (t.roles || []).map((r, i) => (r === "tripa" ? i : -1)).filter((i) => i >= 0);
function formatoTripa(t) {
  const i = tripaDe(t)[0];
  const p = t.info?.paginas[i ?? 0];
  return p ? { ancho: p.ancho, alto: p.alto } : t.info?.formato;
}
const FIRMAS_VALIDAS = [4, 8, 16, 32, 64];
/** Cuadernillos escritos tal cual. */
function cuadernillosEscritos(o) {
  return String(o.cuadernillos || "").split(/[^0-9]+/).map(Number).filter((n) => n > 0);
}
/**
 * Cuadernillos para el libro: los escritos y, si no alcanzan para la tripa,
 * se completa repitiendo el último (p. ej. «16» = todo en cuadernillos de 16)
 * y cerrando con el más chico que alcance, para dejar pocas páginas en blanco.
 */
function cuadernillosDe(o, tripa = 0) {
  const lista = cuadernillosEscritos(o);
  if (!lista.length || lista.some((n) => !FIRMAS_VALIDAS.includes(n))) return lista;
  const ultimo = lista[lista.length - 1];
  let falta = tripa - lista.reduce((a, b) => a + b, 0);
  while (falta > ultimo) { lista.push(ultimo); falta -= ultimo; }
  if (falta > 0) lista.push(FIRMAS_VALIDAS.find((n) => n >= falta && n <= ultimo) ?? ultimo);
  return lista;
}
function textoCuadernillos(t) {
  const escritos = cuadernillosEscritos(t.op);
  const tripa = tripaDe(t).length;
  if (!escritos.length) return "Vacío: la app elige la firma más grande que quepa. Escribe un número (p. ej. 16) para hacer todo en cuadernillos de ese tamaño.";
  const malos = escritos.filter((n) => !FIRMAS_VALIDAS.includes(n));
  if (malos.length) return `Cuadernillos de ${malos.join(", ")} páginas no son válidos: use 4, 8, 16, 32 o 64.`;
  const lista = cuadernillosDe(t.op, tripa);
  const suma = lista.reduce((a, b) => a + b, 0);
  const resumen = Object.entries(lista.reduce((m, n) => ({ ...m, [n]: (m[n] || 0) + 1 }), {})).sort((a, b) => b[0] - a[0]).map(([n, k]) => `${k} de ${n}`).join(" + ");
  const completado = lista.length > escritos.length ? "Completado: " : "";
  return `${completado}${resumen} = ${suma} páginas${suma > tripa ? ` (${suma - tripa} en blanco al final)` : ""}.`;
}

/** Páginas dobles: miden el doble de ancho que las sencillas (pliegos de lectura). */
function paginasDobles(info) {
  if (!info) return { dobles: [], todas: false };
  const sencilla = Math.min(...info.paginas.map((p) => p.ancho));
  const dobles = info.paginas.map((p, i) => (Math.abs(p.ancho - 2 * sencilla) <= sencilla * 0.03 ? i : -1)).filter((i) => i >= 0);
  // Todas apaisadas e iguales: puede ser un PDF entero en pliegos de lectura.
  const todas = !dobles.length && info.paginas.length > 1 && info.paginas.every((p) => p.ancho > p.alto * 1.2);
  return { dobles, todas };
}
function rangos(indices) {
  const r = [];
  for (const i of indices) { const u = r.at(-1); if (u && u[1] === i - 1) u[1] = i; else r.push([i, i]); }
  return r.map(([a, b]) => (a === b ? `${a + 1}` : `${a + 1}–${b + 1}`)).join(", ");
}

function bloquePaginasLibro(t) {
  const roles = t.roles || [];
  const { dobles, todas } = paginasDobles(t.info);
  const avisoDobles = dobles.length
    ? `<div class="aviso-dobles"><b>Este PDF trae páginas dobles (pliegos de lectura): ${dobles.length === 1 ? "la página" : "las páginas"} ${rangos(dobles)}.</b> Cada una son dos páginas seguidas en la misma hoja; para compaginar la revista hay que separarlas.
        <button class="boton boton-naranja boton-chico" type="button" id="lb-separar" data-dobles="${dobles.join(",")}">Separar en páginas sencillas</button></div>`
    : todas ? `<div class="aviso-dobles">¿El PDF viene en pliegos de lectura (dos páginas por hoja)? <button class="boton boton-claro boton-chico" type="button" id="lb-separar" data-dobles="${t.info.paginas.map((_, i) => i).join(",")}">Separar todas en páginas sencillas</button></div>` : "";
  const n = roles.length;
  const tripa = tripaDe(t).length;
  const caratula = roles.filter((r) => ROLES_CARATULA.includes(r)).length;
  const guardas = roles.filter((r) => ROLES_GUARDA.includes(r)).length;
  return `<section class="tarjeta paso paginas-libro">
    <div class="paso-titulo"><span class="paso-num">✦</span><h3>¿Qué es cada página?</h3></div>
    ${avisoDobles}
    <p class="tenue" style="font-size:14px"><b>${tripa}</b> de tripa · <b>${caratula}</b> de carátula${guardas ? ` · <b>${guardas}</b> de guardas` : ""}${roles.includes("excluir") ? ` · ${roles.filter((r) => r === "excluir").length} sin usar` : ""}. Marca la portada y la contraportada (tiro), sus interiores (retiro) y las guardas: cada grupo se monta aparte de la tripa.</p>
    <div class="chips">
      <button class="chip" type="button" data-preset="tripa">Todo es tripa</button>
      ${n >= 6 ? `<button class="chip" type="button" data-preset="extremos">Carátula: 1.ª y última</button>` : ""}
      ${n >= 8 ? `<button class="chip" type="button" data-preset="forros">Carátula con interiores: 1, 2, penúltima, última</button>` : ""}
      ${n >= 12 ? `<button class="chip" type="button" data-preset="guardas">Carátula 1 y última + guardas 2-3 y antepenúltima-penúltima</button>` : ""}
    </div>
    <div class="grilla-paginas">${roles.map((r, i) => `<label class="pagina-rol ${r === "tripa" ? "" : `rol-${r === "excluir" ? "excluir" : ROLES_GUARDA.includes(r) ? "guarda" : "caratula"}`}">
      <span class="pagina-mini">${t.mini?.[i] ? `<img src="${t.mini[i]}" alt="">` : ""}<b>${i + 1}</b></span>
      <select data-rol="${i}" aria-label="Página ${i + 1}">${ROLES.map(([v, txt]) => `<option value="${v}" ${v === r ? "selected" : ""}>${txt}</option>`).join("")}</select>
    </label>`).join("")}</div>
  </section>`;
}

function conectarPaginasLibro(main, t) {
  const redibujar = () => { const y = window.scrollY; vistaLibro(main); window.scrollTo(0, y); };
  $("#lb-separar", main)?.addEventListener("click", async (e) => {
    const b = e.currentTarget;
    b.disabled = true; b.textContent = "Separando…";
    await respirar();
    try {
      const dobles = b.dataset.dobles.split(",").map(Number);
      const bytes = motor.separar_dobles(t.archivo.bytes, JSON.stringify(dobles));
      analizarArchivo(t, { nombre: `${base(t.archivo.nombre)}-paginas.pdf`, tamano: bytes.length, bytes }, true);
      t.roles = t.info ? rolesIniciales(t.info) : [];
      avisar(`Listo: ${t.info?.paginas.length} páginas sencillas`);
    } catch (err) { avisar(`${mensaje(err)}`); }
    redibujar();
  });
  $$("[data-rol]", main).forEach((sel) => sel.addEventListener("change", (e) => {
    e.stopPropagation();
    const i = Number(sel.dataset.rol), rol = sel.value;
    // Cada panel de la carátula es una sola página: la anterior queda sin usar.
    if (ROLES_CARATULA.includes(rol)) t.roles.forEach((r, k) => { if (r === rol && k !== i) t.roles[k] = "excluir"; });
    // Cada guarda es una página extendida o dos sueltas: la tercera desplaza a la primera.
    if (ROLES_GUARDA.includes(rol)) {
      const otras = t.roles.map((r, k) => (r === rol && k !== i ? k : -1)).filter((k) => k >= 0);
      if (otras.length >= 2) t.roles[otras[0]] = "excluir";
    }
    t.roles[i] = rol;
    redibujar();
  }));
  $$("[data-preset]", main).forEach((b) => b.addEventListener("click", () => {
    const n = t.roles.length;
    t.roles = t.roles.map(() => "tripa");
    if (b.dataset.preset === "extremos") { t.roles[0] = "portada"; t.roles[n - 1] = "contraportada"; }
    if (b.dataset.preset === "forros") { t.roles[0] = "portada"; t.roles[1] = "segunda"; t.roles[n - 2] = "tercera"; t.roles[n - 1] = "contraportada"; }
    if (b.dataset.preset === "guardas") {
      t.roles[0] = "portada"; t.roles[n - 1] = "contraportada";
      t.roles[1] = t.roles[2] = "guarda_del"; t.roles[n - 3] = t.roles[n - 2] = "guarda_tras";
    }
    redibujar();
  }));
  // Miniaturas de todas las páginas (hasta 300) para reconocerlas.
  if (t.info) {
    const indices = t.info.paginas.slice(0, 300).map((_, i) => i);
    pedirMiniaturas(t, indices, () => {
      $$(".pagina-mini", main).forEach((el, i) => {
        if (t.mini?.[i] && !el.querySelector("img")) el.insertAdjacentHTML("afterbegin", `<img src="${t.mini[i]}" alt="">`);
      });
      calcularLibro();
    });
  }
}

function peticionCaratula(maquina, montar) {
  const t = estado.libro, o = t.op;
  const asignacion = Object.fromEntries(ROLES_CARATULA.map((r) => { const i = t.roles.indexOf(r); return [r, i < 0 ? null : i]; }));
  const tripaPapel = estado.papeles.find((p) => p.id === $("#lb-papel")?.value);
  const cubierta = estado.papeles.find((p) => p.id === o.papelCaratula);
  const lomoManual = o.lomoCaratula === "" || o.lomoCaratula == null ? null : num(o.lomoCaratula);
  return {
    formato: formatoTripa(t),
    lomo: o.encuadernacion === "caballete" ? 0 : lomoManual,
    paginas: t.plan?.plan?.paginas_libro ?? tripaDe(t).length,
    calibre_um: tripaPapel?.calibre_um ?? null, calibre_portada_um: cubierta?.calibre_um ?? null,
    tipo: o.tapaDura ? { tapa_dura: { carton: 2.5, escuadra: 3, vuelta: 15, bisagra: 8 } } : { rustica: { solapa: num(o.solapaCaratula, 0) } },
    rebase: o.rebase, derecha_a_izquierda: o.rtl, marcas: true,
    titulo: `${base(t.archivo?.nombre)} carátula`, fecha: ahora(), maquina, correcciones: { ...correcciones(), rebase_fondo: false },
    asignacion, montar,
  };
}

function pintarCaratula() {
  const caja = $("#lb-caratula");
  if (!caja) return;
  const t = estado.libro, o = t.op;
  const asignadas = (t.roles || []).map((r, i) => [r, i]).filter(([r]) => ROLES_CARATULA.includes(r));
  if (!asignadas.length || !t.info) { caja.innerHTML = ""; return; }
  const maquina = maquinaElegida("lb-maquina");
  let calculo = null, error = "";
  try { calculo = JSON.parse(motor.calcular_portada_json(JSON.stringify(peticionCaratula(maquina, false)))); }
  catch (e) { error = mensaje(e); }
  const tiro = asignadas.filter(([r]) => !["segunda", "tercera", "interior", "solapa_portada_interior", "solapa_contraportada_interior"].includes(r));
  const retiro = asignadas.filter(([r]) => !tiro.some(([x]) => x === r));
  const opcionesPapel = estado.papeles.map((p) => `<option value="${esc(p.id)}" ${p.id === o.papelCaratula ? "selected" : ""}>${esc(p.nombre)} · ${p.calibre_um} µm</option>`).join("");
  caja.innerHTML = `<section class="tarjeta caratula">
    <div class="paso-titulo"><span class="orbe cotizacion-orbe" aria-hidden="true"></span><div><h3>Carátula</h3><p class="tenue" style="font-size:14px">Tiro: ${tiro.map(([r, i]) => `${NOMBRE_ROL[r].split(" · ")[0]} (pág. ${i + 1})`).join(", ") || "—"}<br>Retiro: ${retiro.map(([r, i]) => `${NOMBRE_ROL[r].split(" · ")[0]} (pág. ${i + 1})`).join(", ") || "sin interior impreso"}</p></div></div>
    <div class="fila">
      <label class="campo"><span>Papel de la carátula</span><select data-car="papelCaratula"><option value="">Sin sumar al lomo</option>${opcionesPapel}</select></label>
      <label class="campo"><span>Solapas (mm)</span><input type="number" min="0" data-car="solapaCaratula" value="${o.solapaCaratula ?? 0}" ${o.tapaDura ? "disabled" : ""}></label>
    </div>
    <div class="fila">
      <label class="campo"><span>Lomo manual (mm)</span><input type="number" min="0" step="0.1" data-car="lomoCaratula" value="${o.lomoCaratula ?? ""}" placeholder="${o.encuadernacion === "caballete" ? "0 (caballete)" : "se calcula con el papel"}" ${o.encuadernacion === "caballete" ? "disabled" : ""}></label>
      <div class="campo" style="align-content:end">${interruptor("car-tapa-dura", "Tapa dura", o.tapaDura)}</div>
    </div>
    ${error ? `<div class="aviso-suave">${esc(/calibre|páginas/.test(error) ? "Elige el papel de la tripa (o escribe el lomo a mano) para calcular la carátula." : error)}</div>` : ""}
    ${calculo ? `<p class="explicacion-clara">Carátula extendida de <b>${mm(calculo.tamano.ancho, 1)} × ${mm(calculo.tamano.alto, 1)} mm</b> con lomo de <span class="resaltado">${mm(calculo.lomo, 2)} mm</span>. El retiro se arma reflejado: al voltear el pliego, la 2.ª de forros cae detrás de la portada.</p>
    <div class="acciones-resultado">
      <button class="boton" type="button" id="car-montada">Descargar carátula montada en ${esc(maquina?.nombre || "la máquina")}</button>
      <button class="boton boton-claro" type="button" id="car-sola">Descargar carátula sola</button>
    </div>` : ""}
  </section>`;
  $$("[data-car]", caja).forEach((el) => el.addEventListener("change", (e) => {
    e.stopPropagation();
    o[el.dataset.car] = el.value;
    setTimeout(pintarCaratula, 0);
  }));
  $("#car-tapa-dura", caja)?.addEventListener("change", (e) => { e.stopPropagation(); o.tapaDura = e.target.checked; setTimeout(pintarCaratula, 0); });
  const descargarCaratula = async (montar, boton) => {
    const texto = boton.textContent;
    boton.disabled = true; boton.textContent = "Generando…";
    await respirar();
    try {
      const icc = montar && maquina ? (await iccDB.leer(maquina.id)) || new Uint8Array() : new Uint8Array();
      const r = motor.generar_caratula(t.archivo.bytes, JSON.stringify(peticionCaratula(maquina, montar)), icc);
      const inf = JSON.parse(r.informe);
      descargar(r.pdf, `${base(t.archivo.nombre)}-caratula${montar ? "-pliego" : ""}.pdf`);
      avisar(inf.por_pliego ? `Carátula lista: ${inf.por_pliego} por pliego${inf.con_retiro ? ", con tiro y retiro" : ""}` : `Carátula lista${inf.con_retiro ? " (tiro y retiro)" : ""}`);
    } catch (err) { avisar(`${mensaje(err)}`); }
    boton.disabled = false; boton.textContent = texto;
  };
  $("#car-montada", caja)?.addEventListener("click", (e) => descargarCaratula(true, e.currentTarget));
  $("#car-sola", caja)?.addEventListener("click", (e) => descargarCaratula(false, e.currentTarget));
}

// Guardas: cada una es un pliego extendido (dos veces el formato, pliegue al
// centro) que se imprime aparte, casi siempre en otro papel.
function peticionGuardas(maquina, montar) {
  const t = estado.libro, o = t.op;
  const guardas = ROLES_GUARDA.map((rol) => ({ nombre: NOMBRE_ROL[rol], paginas: t.roles.map((r, i) => (r === rol ? i : -1)).filter((i) => i >= 0) }))
    .filter((g) => g.paginas.length);
  return {
    formato: formatoTripa(t), guardas, rebase: o.rebase, marcas: true, maquina, montar,
    titulo: `${base(t.archivo?.nombre)} guardas`, fecha: ahora(), correcciones: { ...correcciones(), rebase_fondo: false },
  };
}

function pintarGuardas() {
  const caja = $("#lb-guardas");
  if (!caja) return;
  const t = estado.libro;
  const p = t.info ? peticionGuardas(null, false) : { guardas: [] };
  if (!p.guardas.length) { caja.innerHTML = ""; return; }
  const maquina = maquinaElegida("lb-maquina");
  const f = p.formato;
  const descripcion = p.guardas.map((g) => `${g.nombre}: ${g.paginas.length === 1 ? `pág. ${g.paginas[0] + 1} (extendida)` : `págs. ${g.paginas[0] + 1} y ${g.paginas[1] + 1}`}`).join("<br>");
  caja.innerHTML = `<section class="tarjeta caratula">
    <div class="paso-titulo"><span class="orbe cotizacion-orbe" aria-hidden="true"></span><div><h3>Guardas</h3><p class="tenue" style="font-size:14px">${descripcion}</p></div></div>
    <p class="explicacion-clara">Cada guarda se arma extendida, de <b>${mm(f.ancho * 2, 1)} × ${mm(f.alto, 1)} mm</b> con el pliegue al centro, y se monta aparte de la tripa${p.guardas.length > 1 ? "; la delantera y la trasera van juntas en el mismo pliego cuando caben" : ""}. Marca dos páginas sueltas (izquierda y derecha) o una sola ya extendida.</p>
    <div class="acciones-resultado">
      <button class="boton" type="button" id="gu-montada">Descargar guardas montadas en ${esc(maquina?.nombre || "la máquina")}</button>
      <button class="boton boton-claro" type="button" id="gu-sola">Descargar guardas sueltas</button>
    </div>
  </section>`;
  const descargarGuardas = async (montar, boton) => {
    const texto = boton.textContent;
    boton.disabled = true; boton.textContent = "Generando…";
    await respirar();
    try {
      const icc = montar && maquina ? (await iccDB.leer(maquina.id)) || new Uint8Array() : new Uint8Array();
      const r = motor.generar_guardas(t.archivo.bytes, JSON.stringify(peticionGuardas(maquina, montar)), icc);
      const inf = JSON.parse(r.informe);
      descargar(r.pdf, `${base(t.archivo.nombre)}-guardas${montar ? "-pliego" : ""}.pdf`);
      avisar(inf.por_pliego ? `Guardas listas: ${inf.por_pliego} por pliego` : `${inf.guardas === 1 ? "Guarda lista" : "Guardas listas"}`);
      if (inf.avisos?.length) console.info(inf.avisos);
    } catch (err) { avisar(`${mensaje(err)}`); }
    boton.disabled = false; boton.textContent = texto;
  };
  $("#gu-montada", caja)?.addEventListener("click", (e) => descargarGuardas(true, e.currentTarget));
  $("#gu-sola", caja)?.addEventListener("click", (e) => descargarGuardas(false, e.currentTarget));
}

// ───────────── Cotización ─────────────
const pesos = (v) => "$" + Math.round(v).toLocaleString("es-CO");
const UNIDADES = [["ejemplar", "por ejemplar"], ["millar", "por millar"], ["pliego", "por pliego"], ["fijo", "valor fijo"]];

function datosCotizacion(tipo) {
  estado.cotizar ??= {};
  const pref = estado.preferencias.cotizacion || {};
  estado.cotizar[tipo] ??= {
    cantidad: 1000, tiro: null, retiro: null, papel: tipo === "libro" ? estado.preferencias.papelTripa || "" : "",
    acabados: pref.acabados?.[tipo] || (tipo === "libro" ? [{ concepto: "Encuadernación", unidad: "ejemplar", valor: 0 }] : [{ concepto: "Corte", unidad: "millar", valor: 0 }]),
  };
  return estado.cotizar[tipo];
}

/** Tirajes del trabajo actual: un pliego distinto por diseño, firma o combinado. */
function tirajesDe(tipo, c, maquina) {
  const tiro = c.tiro ?? maquina.colores;
  if (tipo === "libro") {
    const plan = estado.libro.plan?.plan;
    if (!plan) return null;
    const retiro = c.retiro ?? maquina.colores;
    // Las firmas combinadas comparten pliego: un tiraje por pliego impreso.
    const grupos = new Map();
    for (const f of plan.firmas) {
      const k = f.pliego || f.numero;
      if (!grupos.has(k)) grupos.set(k, { nombre: f.pliego && plan.firmas.filter((x) => x.pliego === f.pliego).length > 1 ? `Pliego ${k}` : `Firma ${f.numero}`, pliegos_netos: Math.ceil(c.cantidad / f.copias), colores_tiro: tiro, colores_retiro: retiro, tira_retira: f.tira_retira });
    }
    return [...grupos.values()];
  }
  const t = estado.piezas;
  if (!t.plan) return null;
  const retiro = c.retiro ?? (t.op.dorso ? maquina.colores : 0);
  if (t.op.modo === "combinar") {
    return [{ nombre: "Combinado", pliegos_netos: t.plan.plan.pliegos, colores_tiro: tiro, colores_retiro: retiro, tira_retira: false }];
  }
  const d = t.plan.distribucion;
  const disenos = t.op.dorso ? Math.floor(t.info.paginas.length / 2) : t.info.paginas.length;
  return Array.from({ length: disenos }, (_, i) => ({ nombre: `Diseño ${i + 1}`, pliegos_netos: Math.ceil(c.cantidad / (d.columnas * d.filas)), colores_tiro: tiro, colores_retiro: retiro, tira_retira: false }));
}

function pintarCotizacion(tipo) {
  const caja = $(tipo === "libro" ? "#lb-cotizacion" : "#pz-cotizacion");
  if (!caja) return;
  const maquina = maquinaElegida(tipo === "libro" ? "lb-maquina" : "pz-maquina");
  const c = datosCotizacion(tipo);
  const tirajes = maquina && tirajesDe(tipo, c, maquina);
  if (!tirajes) { caja.innerHTML = ""; return; }
  const combinado = tipo === "piezas" && estado.piezas.op.modo === "combinar";
  const cantidad = combinado ? disenosCombinado(estado.piezas).reduce((a, x) => a + x.cantidad, 0) : c.cantidad;
  const pref = estado.preferencias.cotizacion || {};
  const utilidad = pref.utilidad ?? 30, impuesto = pref.impuesto ?? 0;
  const papel = estado.papeles.find((p) => p.id === c.papel);
  const costos = maquina.costos || costosPorDefecto(maquina.tipo);
  let resultado = null, error = "";
  try {
    const compra = papel?.pliegos?.[0] || { ancho: 700, alto: 1000 };
    const pliego = (tipo === "libro" ? estado.libro.plan?.pliego : estado.piezas.plan?.pliego) || maquina.pliego_max;
    resultado = JSON.parse(motor.cotizar(JSON.stringify({
      cantidad, tirajes, utilidad_porcentaje: utilidad, impuesto_porcentaje: impuesto,
      maquina: { digital: maquina.tipo === "digital", duplex: !!maquina.duplex, ...costos },
      papel: papel ? { nombre: papel.nombre, precio_pliego_compra: papel.precio || 0, salen_por_pliego: motor.salen_de(compra.ancho, compra.alto, pliego.ancho, pliego.alto) } : null,
      acabados: c.acabados.filter((a) => a.concepto && a.valor > 0),
    })));
  } catch (e) { error = mensaje(e); }
  const sinCostos = !costos.costo_plancha && !costos.costo_millar && !costos.costo_clic && !costos.costo_arranque;
  caja.innerHTML = `<section class="tarjeta cotizacion">
    <div class="paso-titulo"><span class="orbe cotizacion-orbe" aria-hidden="true"></span><div><h3>Cotización</h3><p class="tenue" style="font-size:14px">Papel con mácula, planchas, impresión y acabados de ${esc(maquina.nombre)}.</p></div></div>
    <div class="fila-3">
      ${combinado ? `<div class="campo"><span>Ejemplares</span><b class="num" style="padding:11px 0">${cantidad.toLocaleString("es-CO")}</b></div>` : `<label class="campo"><span>Ejemplares</span><input type="number" min="1" step="100" data-cot="cantidad" value="${c.cantidad}"></label>`}
      <label class="campo"><span>Colores tiro</span><input type="number" min="0" max="12" data-cot="tiro" value="${tirajes[0].colores_tiro}"></label>
      <label class="campo"><span>Colores retiro</span><input type="number" min="0" max="12" data-cot="retiro" value="${tirajes[0].colores_retiro}"></label>
    </div>
    <label class="campo"><span>Papel</span><select data-cot="papel"><option value="">Sin papel (lo pone el cliente)</option>${estado.papeles.map((p) => `<option value="${esc(p.id)}" ${p.id === c.papel ? "selected" : ""}>${esc(p.nombre)}${p.precio ? ` · ${pesos(p.precio)} pliego` : " · sin precio"}</option>`).join("")}</select></label>
    <div class="campo"><span>Acabados</span>
      <div class="acabados">${c.acabados.map((a, i) => `<div class="acabado">
        <input type="text" data-acabado="${i}" data-campo="concepto" value="${esc(a.concepto)}" placeholder="Corte, plegado, laminado…" aria-label="Concepto">
        <select data-acabado="${i}" data-campo="unidad" aria-label="Unidad">${UNIDADES.map(([v, t]) => `<option value="${v}" ${v === a.unidad ? "selected" : ""}>${t}</option>`).join("")}</select>
        <input type="number" min="0" step="any" data-acabado="${i}" data-campo="valor" value="${a.valor}" aria-label="Valor">
        <button class="boton boton-claro boton-chico" type="button" data-quitar-acabado="${i}" aria-label="Quitar">×</button></div>`).join("")}
        <button class="boton boton-claro boton-chico" type="button" id="${tipo}-agregar-acabado">+ Agregar acabado</button></div></div>
    <div class="fila">
      <label class="campo"><span>Utilidad (%)</span><input type="number" min="0" step="1" data-cot="utilidad" value="${utilidad}"></label>
      <label class="campo"><span>Impuesto (%)</span><input type="number" min="0" step="1" data-cot="impuesto" value="${impuesto}"></label>
    </div>
    ${sinCostos ? `<div class="aviso-suave">La máquina no tiene costos cargados: complétalos en Catálogos → Editar máquina → «Mácula y costos».</div>` : ""}
    ${papel && !papel.precio ? `<div class="aviso-suave">El papel «${esc(papel.nombre)}» no tiene precio: agrégalo en Catálogos → Papeles.</div>` : ""}
    ${error ? `<div class="error-caja">${esc(error)}</div>` : ""}
    ${resultado ? `<div class="cotizacion-cifras">
        <div><b class="num">${resultado.pliegos_netos.toLocaleString("es-CO")}</b><span>pliegos netos</span></div>
        <div><b class="num">+${resultado.macula.toLocaleString("es-CO")}</b><span>mácula</span></div>
        <div><b class="num">${resultado.pliegos_compra ? resultado.pliegos_compra.toLocaleString("es-CO") : "—"}</b><span>pliegos de compra</span></div>
        <div><b class="num">${resultado.planchas}</b><span>planchas</span></div>
      </div>
      <table class="cotizacion-tabla"><tbody>
        ${resultado.lineas.map((l) => `<tr><td><b>${esc(l.concepto)}</b><br><small class="tenue">${esc(l.detalle)}</small></td><td class="num">${pesos(l.valor)}</td></tr>`).join("")}
        <tr class="sub"><td>Subtotal</td><td class="num">${pesos(resultado.subtotal)}</td></tr>
        ${resultado.utilidad ? `<tr><td>Utilidad ${utilidad} %</td><td class="num">${pesos(resultado.utilidad)}</td></tr>` : ""}
        ${resultado.impuesto ? `<tr><td>Impuesto ${impuesto} %</td><td class="num">${pesos(resultado.impuesto)}</td></tr>` : ""}
      </tbody></table>
      <div class="cotizacion-total"><div><span>Total</span><b class="num">${pesos(resultado.total)}</b></div><div><span>Por ejemplar</span><b class="num">${pesos(resultado.unitario)}</b></div></div>
      <div class="acciones-resultado"><button class="boton boton-chico" type="button" id="${tipo}-copiar-cotizacion">Copiar cotización</button></div>` : ""}
  </section>`;
  // Se redibuja después del evento: un «change» que llega al perder el foco
  // no debe reemplazar la tarjeta mientras el navegador todavía la usa.
  const repintar = () => { clearTimeout(caja._repintar); caja._repintar = setTimeout(() => pintarCotizacion(tipo), 0); };
  $$("[data-cot]", caja).forEach((el) => el.addEventListener("change", (e) => {
    e.stopPropagation();
    const k = el.dataset.cot;
    if (k === "utilidad" || k === "impuesto") preferir("cotizacion", { ...(estado.preferencias.cotizacion || {}), [k]: Math.max(0, num(el.value, 0)) });
    else if (k === "papel") c.papel = el.value;
    else c[k] = Math.max(k === "cantidad" ? 1 : 0, Math.round(num(el.value, 0)));
    repintar();
  }));
  const guardarAcabados = () => preferir("cotizacion", { ...(estado.preferencias.cotizacion || {}), acabados: { ...(estado.preferencias.cotizacion?.acabados || {}), [tipo]: c.acabados } });
  $$("[data-acabado]", caja).forEach((el) => el.addEventListener("change", (e) => {
    e.stopPropagation();
    const a = c.acabados[Number(el.dataset.acabado)];
    a[el.dataset.campo] = el.dataset.campo === "valor" ? Math.max(0, num(el.value, 0)) : el.value;
    guardarAcabados(); repintar();
  }));
  $$("[data-quitar-acabado]", caja).forEach((b) => b.addEventListener("click", () => { c.acabados.splice(Number(b.dataset.quitarAcabado), 1); guardarAcabados(); repintar(); }));
  $(`#${tipo}-agregar-acabado`, caja)?.addEventListener("click", () => { c.acabados.push({ concepto: "", unidad: "ejemplar", valor: 0 }); repintar(); });
  $(`#${tipo}-copiar-cotizacion`, caja)?.addEventListener("click", async () => {
    const r = resultado;
    const texto = [
      `Cotización · ${cantidad.toLocaleString("es-CO")} ejemplares`,
      ...r.lineas.map((l) => `• ${l.concepto}: ${pesos(l.valor)} (${l.detalle})`),
      `Subtotal: ${pesos(r.subtotal)}`,
      r.utilidad ? `Utilidad: ${pesos(r.utilidad)}` : "",
      r.impuesto ? `Impuesto: ${pesos(r.impuesto)}` : "",
      `TOTAL: ${pesos(r.total)} · ${pesos(r.unitario)} por ejemplar`,
    ].filter(Boolean).join("\n");
    try { await navigator.clipboard.writeText(texto); avisar("Cotización copiada"); } catch { avisar("No se pudo copiar"); }
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
          <div class="paso-titulo"><span class="paso-num">1</span><h3>PDF del libro</h3></div>
          ${t.archivo ? tarjetaArchivo(t.archivo, t.info, "lb-cambiar") : zonaArchivo("lb-archivo", "Sube el PDF del libro (con o sin carátula)")}
          ${bloquePreflight(t)}
        </section>
        ${t.info ? bloquePaginasLibro(t) : ""}
        <section class="tarjeta paso">
          <div class="paso-titulo"><span class="paso-num">2</span><h3>Encuadernación</h3></div>
          ${chips("encuadernacion", [["caballete", "Caballete"], ["lomo", "Al lomo (PUR)"], ["cosido", "Cosido"]], o.encuadernacion)}
          ${selectorMaquina("lb-maquina")}
          ${selectorPapel("lb", o, maquinaElegida("lb-maquina"), papeles.find((p) => p.id === papelElegido))}
          ${campoTintas(o, true)}
          <label class="campo"><span>Papel de la tripa</span>
            <select id="lb-papel"><option value="">Sin papel (no calcula creep ni lomo)</option>${papeles.map((p) => `<option value="${esc(p.id)}" ${p.id === papelElegido ? "selected" : ""}>${esc(p.nombre)} · ${p.calibre_um} µm</option>`).join("")}</select>
            ${papeles.length ? "" : `<small><a href="#catalogos">Agrega papeles</a> para calcular el lomo y el creep.</small>`}
          </label>
          <div class="campo"><span>Páginas por firma</span>${chips("firma", [["auto", "Auto"], ["4", "4"], ["8", "8"], ["16", "16"], ["32", "32"], ["64", "64"]], o.firma)}</div>
          <label class="campo"><span>Cuadernillos a mano (opcional)</span><input type="text" id="lb-cuadernillos" value="${esc(o.cuadernillos || "")}" placeholder="Ej.: 16,16,16,8" autocomplete="off"><small>${textoCuadernillos(t)}</small></label>
          ${campoEscala("lb", o, t.info ? formatoTripa(t) : null)}
          <div class="campo"><span>Orientación de las páginas en el pliego</span>${chips("orientacionPaginas", [["auto", "Automática (la que más rinda)"], ["normal", "Derechas"], ["girada", "Giradas 90°"]], o.orientacionPaginas || "auto")}<div id="lb-orientaciones" class="tenue" style="font-size:13px"></div></div>
          <div class="campo"><span>Volteo del retiro</span>${chips("volteo", [["maquina", `Según la máquina${maquinaElegida("lb-maquina") ? ` (${maquinaElegida("lb-maquina").volteo === "cabeza" ? "de cabeza" : "de lado"})` : ""}`], ["lateral", "De lado"], ["cabeza", "De cabeza"]], o.volteo || "maquina")}<small>De lado: se voltea conservando la pinza. De cabeza: la cola pasa a ser pinza (pinza y cola se igualan).</small></div>
          <div class="campo"><span>Armado</span>${chips("armado", [["auto", "Automático"], ["plegado", "Pliegos plegados (8, 16, 32 pp)"], ["sueltas", "Hojas sueltas de 4 pp (corte y anidado)"]], o.armado || "auto")}
            <small>${armadoDe(o, maquinaElegida("lb-maquina")) === "sueltas" ? "Cada hoja lleva 4 páginas (2 por cara) y en el pliego van hojas distintas, en orden: se imprime, se corta, se dobla cada hoja y se anidan." : "Cada firma se pliega entera (8, 16 o 32 páginas) y las firmas se anidan o se alzan."}${(o.armado || "auto") === "auto" ? " Automático: hojas sueltas en digital a caballete; plegado en lo demás." : ""}</small></div>
          <div class="campo"><span>Firmas por pliego</span>${chips("aprovechamiento", [["auto", "Auto"], ["una", "Una"], ["repetir", "Repetir"], ["tira_retira", "Tira y retira"]], o.aprovechamiento)}<small>Auto monta en tira y retira (una sola plancha para las dos caras) cuando la firma cabe dos veces lado a lado.</small></div>
          <details class="avanzado"><summary>Márgenes, lectura y marcas</summary>
            <div class="paso">
              <div class="fila-3">
                <label class="campo"><span>Rebase</span><input type="number" step="0.5" min="3" id="lb-rebase" value="${o.rebase}"></label>
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
      <div class="columna-resultado"><div class="resultado" id="lb-resultado"></div><div id="lb-comparar"></div><div id="lb-caratula"></div><div id="lb-guardas"></div><div id="lb-cotizacion"></div></div>
    </div>`;
  // Libro nuevo: los cuadernillos a mano eran del anterior.
  conectarArchivo("lb-archivo", (a) => { analizarArchivo(t, a, true); t.roles = t.info ? rolesIniciales(t.info) : []; o.cuadernillos = ""; vistaLibro(main); });
  conectarPaginasLibro(main, t);
  $("#lb-cambiar")?.addEventListener("click", () => { t.archivo = null; t.info = null; t.plan = null; t.error = null; vistaLibro(main); });
  conectarCorrecciones(main, () => vistaLibro(main));
  conectarSelectorPapel("lb", o, (redibujar) => { t.cara = 0; if (redibujar) vistaLibro(main); else calcularLibro(); });
  conectarEscala("lb", o, () => { t.cara = 0; vistaLibro(main); });
  $$("input, select", main).forEach((el) => el.addEventListener("change", () => {
    if (el.type === "file" || el.dataset.correccion || el.dataset.rol || /papel-tam|pliego-(ancho|alto)|-escala|-pinza$/.test(el.id)) return;
    const antes = { rebase: o.rebase, maquina: estado.preferencias.maquina };
    o.cuadernillos = $("#lb-cuadernillos").value.trim();
    $("#lb-cuadernillos").nextElementSibling.textContent = textoCuadernillos(t);
    o.rebase = rebaseMinimo(num($("#lb-rebase").value, 3));
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
  conectarChips(main, (n, v) => { o[n] = v; t.cara = 0; if (n === "encuadernacion" || n === "armado") vistaLibro(main); else calcularLibro(); });
  calcularLibro();
}

/** Cómo se arma el libro: plegado (firmas grandes) u hojas sueltas de 4 pp combinadas. */
function armadoDe(o, maquina) {
  const a = o.armado || "auto";
  if (a !== "auto") return a;
  return maquina?.tipo === "digital" && o.encuadernacion === "caballete" ? "sueltas" : "plegado";
}

function peticionLibro(maquina, info) {
  const o = estado.libro.op;
  const papel = estado.papeles.find((p) => p.id === $("#lb-papel")?.value);
  const tripa = tripaDe(estado.libro);
  const enOrden = tripa.length === info.paginas.length;
  return {
    maquina, formato: formatoTripa(estado.libro), paginas: tripa.length, encuadernacion: o.encuadernacion,
    mapa: enOrden ? [] : tripa, cuadernillos: armadoDe(o, maquina) === "sueltas" ? [] : cuadernillosDe(o, tripa.length),
    firma: armadoDe(o, maquina) === "sueltas" ? 4 : o.firma === "auto" ? null : Number(o.firma), rebase: o.rebase, fresado: o.fresado, refile: o.refile,
    calibre_um: papel && (o.creep || o.encuadernacion !== "caballete") ? papel.calibre_um : null,
    derecha_a_izquierda: o.rtl, marcas: o.marcas, tira_color: o.tira, aprovechamiento: armadoDe(o, maquina) === "sueltas" ? "combinar" : o.aprovechamiento || "auto",
    volteo: !o.volteo || o.volteo === "maquina" ? null : o.volteo,
    pliego: pliegoDe(o, maquina), orientacion_papel: orientacionPapel(o), orientacion_paginas: o.orientacionPaginas || "auto", escala: escalaDe(o),
    titulo: base(estado.libro.archivo?.nombre), fecha: ahora(), correcciones: correcciones(),
  };
}

function calcularLibro() {
  const t = estado.libro;
  const caja = $("#lb-resultado");
  if (!caja) return;
  const maquina = maquinaElegida("lb-maquina");
  t.plan = null;
  if (t.info && tripaDe(t).length && maquina && !t.error) {
    try { t.plan = JSON.parse(motor.planear_libro(JSON.stringify(peticionLibro(maquina, t.info)))); }
    catch (e) { t.plan = null; caja.dataset.error = mensaje(e); }
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
    const hoja = t.plan.pliego;
    explicacion = `${p.paginas_libro} páginas en <span class="resaltado">${composicion}</span>${girada ? ", con las páginas giradas 90° en el pliego" : ", con las páginas derechas"}, en papel ${comoPapel(hoja)} de ${cm(hoja.ancho)} × ${cm(hoja.alto)} cm. `;
    const esc0 = escalaDe(t.op);
    if (esc0 !== 1) { const f0 = formatoTripa(t); explicacion += `Arte al <span class="resaltado">${Math.round(esc0 * 100)} %</span>: cada página queda de ${cm(f0.ancho * esc0)} × ${cm(f0.alto * esc0)} cm. `; }
    explicacion += o.encuadernacion === "caballete" ? "Las firmas van anidadas una dentro de otra." : "Las firmas se alzan una tras otra" + (o.encuadernacion === "lomo" ? `, con ${mm(o.fresado)} mm de fresado en el lomo.` : ".");
    if (p.blancas) explicacion += ` Se agregan <b>${p.blancas}</b> páginas en blanco al final.`;
    if (armadoDe(t.op, maquina) === "sueltas") {
      const hojas = Math.max(...p.firmas.map((f) => f.pliego || 1));
      const porPliego = Math.max(...Object.values(p.firmas.reduce((a, f) => ({ ...a, [f.pliego]: (a[f.pliego] || 0) + 1 }), {})));
      explicacion += ` Hojas sueltas: <span class="resaltado">${porPliego} hojas distintas por pliego</span> (${porPliego * 2} páginas por cara), ${hojas} ${hojas === 1 ? "pliego" : "pliegos"} por ejemplar. Se corta el pliego y cada hoja se dobla y se anida en orden.`;
    }
    const multiples = p.firmas.filter((f) => f.copias > 1);
    if (multiples.length) {
      const tr = multiples.some((f) => f.tira_retira);
      explicacion += ` ${multiples.length === p.firmas.length ? "Cada pliego" : `En ${multiples.length} ${multiples.length === 1 ? "firma" : "firmas"}, cada pliego`} da <span class="resaltado">${Math.max(...multiples.map((f) => f.copias))} firmas${tr ? " en tira y retira" : ""}</span>: ${p.juegos_planchas} juegos de planchas y ${mm(p.pliegos_por_ejemplar, 2)} pliegos por ejemplar.`;
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
    ${t.op.marcas && (t.plan?.avisos || []).some((a) => a.includes("sin marcas de corte sí entrarían"))
      ? `<div class="aviso-dobles">Sin marcas de corte entran más páginas por pliego. Se corta con las medidas del montaje (o con marcas en una prueba).
          <button class="boton boton-naranja boton-chico" type="button" id="lb-sin-marcas">Montar sin marcas para meter más páginas</button></div>` : ""}
    ${tarjetaVistaPrevia(t, maquina, t.archivo ? "Ajusta las opciones para ver las firmas" : "Sube el interior para ver las firmas", true)}`;
  pintarCotizacion("libro");
  pintarCompararLibro();
  pintarOrientaciones(maquina);
  if (t.info && t.plan && maquina) {
    const base = peticionLibro(maquina, t.info);
    sugerirEscala("lb", t.op, formatoTripa(t), (e) => {
      try { return -JSON.parse(motor.planear_libro(JSON.stringify({ ...base, escala: e }))).plan.pliegos_por_ejemplar; } catch { return null; }
    }, (v, b) => `salen ${mm(-v, 2)} pliegos por libro en vez de ${mm(-b, 2)}`, () => { t.cara = 0; vistaLibro($("#vista")); });
  }
  pintarCaratula();
  pintarGuardas();
  const carasLb = t.plan?.plan?.caras;
  if (carasLb?.length) pedirMiniaturas(t, carasLb[Math.min(t.cara, carasLb.length - 1)].ubicaciones.map((u) => u.pagina), calcularLibro);
  if (!pintar(caja, html)) return;
  conectarPaginador(caja, t, calcularLibro);
  $("#lb-sin-marcas", caja)?.addEventListener("click", () => { t.op.marcas = false; t.cara = 0; vistaLibro($("#vista")); avisar("Marcas de corte quitadas"); });
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
      const r = motor.generar_libro(t.archivo.bytes, JSON.stringify({ ...peticionLibro(maquina, t.info), correcciones: await correccionesParaGenerar(t, tripaDe(t)) }), icc);
      descargar(r.pdf, `${base(t.archivo.nombre)}-pliegos.pdf`);
      const corregidos = JSON.parse(r.informe).avisos.filter((a) => a.startsWith("corregido")).length;
      avisar(`Pliegos listos${corregidos ? ` · ${corregidos} correcciones aplicadas` : ""}`);
    } catch (err) { avisar(`Error: ${mensaje(err)}`); }
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
            <label class="campo"><span>Rebase (mm)</span><input type="number" step="0.5" min="3" id="po-rebase" value="${o.rebase}"></label>
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
    o.rebase = rebaseMinimo(num($("#po-rebase").value, 3));
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
    titulo: tripa ? `${o.paginas} pp en ${tripa.nombre}` : "Macula", fecha: ahora(),
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
  catch (e) { error = mensaje(e); }
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
    catch (e) { avisar(`Error: ${mensaje(e)}`); }
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
    } catch (err) { avisar(`${mensaje(err)}`); }
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
    pinza: d.pinza, cola: d.cola, lateral: d.lateral, plancha: null, colores: d.colores, duplex: d.duplex, volteo: d.volteo || "lateral",
    salida: { pdfx: d.pdfx || "PDF/X-4", perfil_icc: null, condicion: d.condicion || null, jdf: !!d.jdf }, notas: d.notas || "",
    costos: d.costos || costosPorDefecto(d.tipo),
  };
}

/** Mácula y costos de partida: se ajustan en cada máquina. */
function costosPorDefecto(tipo) {
  return tipo === "digital"
    ? { costo_plancha: 0, costo_arranque: 0, costo_millar: 0, costo_minimo: 0, costo_clic: 0, macula_arranque: 5, macula_porcentaje: 1 }
    : { costo_plancha: 0, costo_arranque: 0, costo_millar: 0, costo_minimo: 0, costo_clic: 0, macula_arranque: 100, macula_porcentaje: 3 };
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
          <dt>Volteo</dt><dd>${m.volteo === "cabeza" ? "De cabeza" : "De lado"}</dd>
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
    <label class="campo"><span>Volteo del pliego para el retiro</span><select name="volteo">
      <option value="lateral" ${(v.volteo || "lateral") === "lateral" ? "selected" : ""}>De lado: la pinza se conserva (tira y retira)</option>
      <option value="cabeza" ${v.volteo === "cabeza" ? "selected" : ""}>De cabeza: la cola pasa a ser pinza (tumble)</option>
    </select><small>Así se arma el retiro de libros, revistas y carátulas en esta máquina. Se puede cambiar en cada trabajo.</small></label>
    <label class="campo"><span>Condición de impresión</span><select name="condicion">${CONDICIONES.map((c) => `<option ${c === v.salida.condicion ? "selected" : ""}>${c}</option>`).join("")}</select></label>
    <label class="campo"><span>Perfil ICC de salida (opcional)</span><input type="file" name="icc" accept=".icc,.icm"><small>Con el perfil, el PDF sale identificado como PDF/X con su OutputIntent.</small></label>
    ${interruptor("mq-jdf", "Su RIP/CTP recibe JDF", v.salida.jdf)}
    <details class="avanzado" open><summary>Mácula y costos (para cotizar)</summary><div class="paso">
      <div class="fila">
        <label class="campo"><span>Mácula de arranque</span><input type="number" min="0" step="10" name="macula_arranque" value="${(v.costos || costosPorDefecto(v.tipo)).macula_arranque}"><small>Pliegos por cada pasada (puesta a punto)</small></label>
        <label class="campo"><span>Mácula de tiraje (%)</span><input type="number" min="0" step="0.5" name="macula_porcentaje" value="${(v.costos || costosPorDefecto(v.tipo)).macula_porcentaje}"><small>Desperdicio durante la impresión</small></label>
      </div>
      <div class="fila">
        <label class="campo"><span>Plancha (c/u)</span><input type="number" min="0" name="costo_plancha" value="${v.costos?.costo_plancha ?? 0}"></label>
        <label class="campo"><span>Arranque por pasada</span><input type="number" min="0" name="costo_arranque" value="${v.costos?.costo_arranque ?? 0}"></label>
      </div>
      <div class="fila">
        <label class="campo"><span>Millar de pliegos (por pasada)</span><input type="number" min="0" name="costo_millar" value="${v.costos?.costo_millar ?? 0}"></label>
        <label class="campo"><span>Clic digital (por cara)</span><input type="number" min="0" step="0.1" name="costo_clic" value="${v.costos?.costo_clic ?? 0}"></label>
      </div>
      <label class="campo"><span>Mínimo de impresión por tiraje</span><input type="number" min="0" name="costo_minimo" value="${v.costos?.costo_minimo ?? 0}"></label>
    </div></details>
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
      colores: Math.round(num(f.get("colores"), 4)), duplex: $("#mq-duplex").checked, volteo: f.get("volteo") === "cabeza" ? "cabeza" : "lateral",
      pdfx: f.get("pdfx"), condicion: f.get("condicion") === "Otra" ? null : f.get("condicion"), jdf: $("#mq-jdf").checked, notas: f.get("notas"),
      pliego_min: v.pliego_min,
      costos: Object.fromEntries(["macula_arranque", "macula_porcentaje", "costo_plancha", "costo_arranque", "costo_millar", "costo_clic", "costo_minimo"].map((k) => [k, Math.max(0, num(f.get(k), 0))])),
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
    <div class="fila">
      <label class="campo"><span>Formato de compra (mm)</span><input type="text" name="compra" value="${v.pliegos?.[0] ? `${v.pliegos[0].ancho}x${v.pliegos[0].alto}` : "700x1000"}" placeholder="700x1000"></label>
      <label class="campo"><span>Precio por pliego de compra</span><input type="number" min="0" step="any" name="precio" value="${v.precio ?? 0}"></label>
    </div>
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
    const [ca, cb] = String(f.get("compra") || "").toLowerCase().split(/[x×]/).map((x) => num(x, 0));
    const datos = { ...v, nombre: String(f.get("nombre")).trim(), gramaje: num(f.get("gramaje")), calibre_um: num(f.get("calibre")), estucado: $("#pp-estucado").checked, fibra: fibra || null, notas: f.get("notas"),
      pliegos: ca > 0 && cb > 0 ? [{ ancho: ca, alto: cb }] : v.pliegos, precio: Math.max(0, num(f.get("precio"), 0)) };
    if (!p) { datos.id = slug(datos.nombre); while (estado.papeles.some((x) => x.id === datos.id)) datos.id += "-2"; estado.papeles.push(datos); }
    else Object.assign(p, datos);
    guardarCatalogos();
    avisar("Papel guardado");
    vistaCatalogos(main);
  });
}

// ───────────── Arranque ─────────────
// ───────────── Sesión, suscripción y licencia ─────────────
// La app solo llega a cuentas al día (puerta.php); además el motor exige una
// licencia firmada por el servidor, vigente hasta 3 días sin conexión.
async function refrescarCuenta() {
  const yo = await api("cuenta.php?accion=yo");
  if (!yo.sesion) { location.href = "/acceso.html"; return null; }
  if (!yo.suscripcion.puede_usar) { location.href = "/acceso.html#pagar"; return null; }
  estado.cuenta = yo;
  if (yo.licencia) {
    motor.activar_licencia(yo.licencia);
    almacen.guardar("licencia", yo.licencia);
  }
  pintarCuenta();
  return yo;
}

async function activarSesion() {
  try {
    return await refrescarCuenta();
  } catch (e) {
    // Sin conexión: vale la última licencia mientras siga vigente.
    const guardada = almacen.leer("licencia", "");
    try { if (guardada) { motor.activar_licencia(guardada); return { sinConexion: true }; } } catch { /* vencida */ }
    throw new Error("No hay conexión con Macula y la licencia de este equipo venció. Conéctate a internet para seguir.");
  }
}

function pintarCuenta() {
  const c = estado.cuenta;
  const boton = $("#cuenta-boton");
  if (boton && c) {
    boton.hidden = false;
    boton.querySelector("span").textContent = c.empresa.nombre;
  }
  const franja = $("#franja-cuenta");
  if (!franja || !c) return;
  const s = c.suscripcion;
  const dias = Math.max(0, Math.ceil((s.acceso_hasta - Date.now() / 1000) / 86400));
  const mostrar = s.estado === "prueba" || s.estado === "gracia" || (s.estado === "activa" && dias <= 5 && !(c.empresa.tarjeta && c.empresa.renovar));
  franja.hidden = !mostrar;
  if (mostrar) {
    franja.dataset.tono = s.estado === "gracia" ? "error" : "aviso";
    franja.innerHTML = s.estado === "prueba"
      ? `Prueba gratis: te ${dias === 1 ? "queda 1 día" : `quedan ${dias} días`}. <a href="#cuenta">Elegir plan y pagar</a>`
      : s.estado === "gracia"
        ? `No pudimos cobrar la suscripción. En ${dias} ${dias === 1 ? "día" : "días"} la cuenta se bloquea. <a href="#cuenta">Pagar ahora</a>`
        : `Tu suscripción vence en ${dias} ${dias === 1 ? "día" : "días"}. <a href="#cuenta">Renovar</a>`;
  }
}

async function arrancar() {
  try {
    await iniciarMotor();
    $("#version").textContent = `Motor ${motor.version()}`;
  } catch (e) {
    $("#vista").innerHTML = `<div class="error-caja">No se pudo cargar el motor de imposición en este navegador (${esc(e.message || e)}). Usa una versión reciente de Chrome, Edge, Firefox o Safari.</div>`;
    return;
  }
  try {
    if (!(await activarSesion())) return;
  } catch (e) {
    $("#vista").innerHTML = `<div class="error-caja">${esc(e.message)}</div>`;
    return;
  }
  window.addEventListener("hashchange", navegar);
  navegar();
  // Pide al navegador no borrar estos datos cuando le falte espacio.
  navigator.storage?.persist?.().catch(() => {});
  sincronizar();
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState !== "visible") return;
    sincronizar();
    refrescarCuenta().catch(() => {});
  });
  // La licencia dura 3 días: se renueva cada 6 horas mientras la app esté abierta.
  setInterval(() => refrescarCuenta().catch(() => {}), 6 * 3600 * 1000);
}
arrancar();
