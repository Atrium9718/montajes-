// Diagramación de libros: del manuscrito al interior listo para imponer.
// Compone con reglas editoriales (aperturas en impar, cornisas, folios,
// viudas y huérfanas, división silábica en español), cuadra las páginas en
// cuadernillos y entrega el PDF con TrimBox y rebase para enviarlo a Libros.

import * as C from "./composicion.js";

const FORMATOS = [
  ["120x190", "12 × 19"], ["140x210", "14 × 21"], ["148x210", "A5"], ["150x230", "15 × 23"],
  ["170x240", "17 × 24"], ["216x279", "Carta"], ["otro", "Otro"],
];

const PREDETERMINADAS = {
  formato: "140x210", ancho: 140, alto: 210, rebase: 3, margenes: C.margenesSugeridos(140, 210),
  fresado: false, fresadoMm: 3,
  fuente: "Libertinus Serif", cuerpo: 11, interlineado: 14.5, sangria: 1.2,
  justificar: true, silabas: true, capImpar: true, cornisa: true, cornisaPar: "titulo", folio: "pie",
  numeracion: "original", comillas: "angulares", limpieza: true,
  preliminares: { portadilla: true, portada: true, legal: true, dedicatoria: "", indice: "final", colofon: true },
  datos: { titulo: "", subtitulo: "", autor: "", editorial: "", isbn: "", edicion: "Primera edición", ciudad: "", impresor: "", creditos: "" },
  firma: 16, cuadrar: "ajustar",
};

const D = { manuscrito: null, pdf: null, error: null, estado: "", resultado: null, pliego: 0, imagenes: {}, generacion: 0, fuentes: null };

export function vistaDiagramacion(main, h) {
  const { $, $$, esc, chips, conectarChips, interruptor, num } = h;
  const op = opciones(h);
  const d = op.datos, pre = op.preliminares;
  const m = D.manuscrito;
  const r = m ? C.resumen(m.bloques) : null;
  const campo = (id, texto, valor, extra = "") => `<label class="campo"><span>${texto}</span><input type="text" id="dg-${id}" value="${esc(valor)}" autocomplete="off" ${extra}></label>`;
  const numero = (id, texto, valor, paso = 0.5) => `<label class="campo"><span>${texto}</span><input type="number" step="${paso}" min="0" id="dg-${id}" value="${valor}"></label>`;
  const familias = [...C.FUENTES_INCLUIDAS, ...(D.fuentes || []).map((f) => f.familia)].filter((f, i, a) => a.indexOf(f) === i);
  main.innerHTML = `
    <div class="encabezado"><div><h1>Diagramación de <span class="serif">libros</span></h1><p>Sube el manuscrito y sale el interior compuesto con reglas editoriales, cuadrado en cuadernillos y listo para montar.</p></div></div>
    <div class="trabajo">
      <div class="panel">
        <section class="tarjeta paso ${m ? "listo" : ""}">
          <div class="paso-titulo"><span class="paso-num">1</span><h3>Manuscrito</h3></div>
          ${m ? `<div class="archivo"><span class="orbe" aria-hidden="true"></span><div><b>${esc(m.nombre)}</b><span>${r.palabras.toLocaleString("es-CO")} palabras · ${r.capitulos.length} capítulos${r.partes ? ` · ${r.partes} partes` : ""}${r.notas ? ` · ${r.notas} notas` : ""}${r.imagenes ? ` · ${r.imagenes} imágenes` : ""}</span></div>
              <button class="boton boton-claro boton-chico" id="dg-cambiar" type="button">Cambiar</button></div>
            ${r.capitulos.length ? `<details class="avanzado"><summary>Capítulos encontrados</summary><ol class="lista-capitulos">${r.capitulos.map((c) => `<li>${esc(c)}</li>`).join("")}</ol></details>`
              : `<ul class="avisos"><li>No encontré capítulos. En Word marca los títulos con el estilo «Título 1», o escribe «Capítulo 1» en su propia línea.</li></ul>`}
            ${m.avisos?.length ? `<ul class="avisos">${m.avisos.slice(0, 3).map((a) => `<li>${esc(a)}</li>`).join("")}</ul>` : ""}`
          : `<label class="soltar" id="dg-archivo-zona"><span class="orbe" aria-hidden="true"></span><b>Sube el manuscrito</b><span class="tenue">Word (.docx), Markdown (.md) o texto (.txt)</span>
              <input type="file" id="dg-archivo" accept=".docx,.md,.markdown,.txt,application/vnd.openxmlformats-officedocument.wordprocessingml.document,text/plain,text/markdown"></label>
            <details class="avanzado"><summary>O pega el texto</summary><div class="paso">
              <textarea id="dg-pegado" rows="8" placeholder="Capítulo 1&#10;El comienzo&#10;&#10;Había una vez…"></textarea>
              <button class="boton boton-chico" id="dg-usar-pegado" type="button">Usar este texto</button></div></details>`}
          <div class="campo"><span>Comillas</span>${chips("comillas", [["angulares", "« » españolas"], ["inglesas", "“ ” inglesas"], ["dejar", "Como vienen"]], op.comillas)}</div>
          ${interruptor("dg-limpieza", "Limpieza editorial (rayas de diálogo, puntos suspensivos, espacios dobles)", op.limpieza)}
        </section>

        <section class="tarjeta paso">
          <div class="paso-titulo"><span class="paso-num">2</span><h3>Datos del libro</h3></div>
          ${campo("titulo", "Título", d.titulo)}
          ${campo("subtitulo", "Subtítulo", d.subtitulo)}
          <div class="fila-2">${campo("autor", "Autor", d.autor)}${campo("editorial", "Editorial", d.editorial)}</div>
          <details class="avanzado"><summary>Página legal, dedicatoria y colofón</summary><div class="paso">
            <div class="fila-2">${campo("isbn", "ISBN", d.isbn, 'placeholder="978-…"')}${campo("edicion", "Edición", d.edicion)}</div>
            <div class="fila-2">${campo("impresor", "Impreso en (taller)", d.impresor)}${campo("ciudad", "Ciudad", d.ciudad)}</div>
            <label class="campo"><span>Créditos (diseño, corrección, ilustración…)</span><textarea id="dg-creditos" rows="3">${esc(d.creditos)}</textarea></label>
            <label class="campo"><span>Dedicatoria</span><textarea id="dg-dedicatoria" rows="2" placeholder="Vacío: sin dedicatoria">${esc(pre.dedicatoria)}</textarea></label>
          </div></details>
          ${interruptor("dg-portadilla", "Portadilla (solo el título)", pre.portadilla)}
          ${interruptor("dg-portada", "Portada interior (autor, título, editorial)", pre.portada)}
          ${interruptor("dg-legal", "Página legal (créditos e ISBN)", pre.legal)}
          ${interruptor("dg-colofon", "Colofón en la última página", pre.colofon)}
          <div class="campo"><span>Índice</span>${chips("indice", [["inicio", "Al inicio"], ["final", "Al final"], ["no", "Sin índice"]], pre.indice)}</div>
        </section>

        <section class="tarjeta paso">
          <div class="paso-titulo"><span class="paso-num">3</span><h3>Formato y márgenes</h3></div>
          ${chips("formato", FORMATOS, op.formato)}
          <div class="fila-3">${numero("ancho", "Ancho (mm)", op.ancho)}${numero("alto", "Alto (mm)", op.alto)}${numero("rebase", "Rebase", op.rebase)}</div>
          <div class="fila-4">${numero("m-interior", "Interior", op.margenes.interior)}${numero("m-exterior", "Exterior", op.margenes.exterior)}${numero("m-superior", "Superior", op.margenes.superior)}${numero("m-inferior", "Inferior", op.margenes.inferior)}</div>
          <button class="boton boton-claro boton-chico" id="dg-sugeridos" type="button">Márgenes sugeridos para este formato</button>
          ${interruptor("dg-fresado", `Sumar ${h.mm(op.fresadoMm)} mm al interior por el fresado (libro al lomo)`, op.fresado)}
        </section>

        <section class="tarjeta paso">
          <div class="paso-titulo"><span class="paso-num">4</span><h3>Tipografía</h3></div>
          <label class="campo"><span>Fuente</span><select id="dg-fuente">${familias.map((f) => `<option ${f === op.fuente ? "selected" : ""}>${esc(f)}</option>`).join("")}</select>
            <small>¿Tienes la fuente del libro? <label class="enlace">súbela (.otf o .ttf, todas sus variantes)<input type="file" id="dg-subir-fuente" accept=".otf,.ttf,font/otf,font/ttf" multiple hidden></label>${(D.fuentes || []).length ? ` · <button type="button" class="enlace" id="dg-borrar-fuentes">quitar las propias</button>` : ""}</small></label>
          <div class="fila-3">${numero("cuerpo", "Cuerpo (pt)", op.cuerpo)}${numero("interlineado", "Interlineado (pt)", op.interlineado)}${numero("sangria", "Sangría (em)", op.sangria, 0.1)}</div>
          ${interruptor("dg-justificar", "Texto justificado", op.justificar)}
          ${interruptor("dg-silabas", "Dividir palabras (silabeo en español)", op.silabas)}
          ${interruptor("dg-impar", "Capítulos siempre en página impar", op.capImpar)}
          ${interruptor("dg-cornisa", "Cornisas (título del libro y del capítulo)", op.cornisa)}
          <div class="campo"><span>Folio</span>${chips("folio", [["pie", "Al pie, centrado"], ["cabeza", "En la cabeza, al exterior"]], op.folio)}</div>
          <div class="campo"><span>Etiqueta de capítulos</span>${chips("numeracion", [["original", "Como el original"], ["arabigo", "Capítulo 1"], ["romano", "Capítulo I"], ["ninguna", "Sin etiqueta"]], op.numeracion)}</div>
          <details class="avanzado"><summary>Más ajustes</summary><div class="paso">
            ${numero("caida", "Caída de capítulo (mm desde el margen)", op.caida ?? Math.round(op.alto * 0.25))}
            <div class="campo"><span>Cornisa de páginas pares</span>${chips("cornisaPar", [["titulo", "Título del libro"], ["autor", "Autor"]], op.cornisaPar)}</div>
          </div></details>
        </section>

        <section class="tarjeta paso">
          <div class="paso-titulo"><span class="paso-num">5</span><h3>Cuadrar en cuadernillos</h3></div>
          <div class="campo"><span>Páginas por cuadernillo</span>${chips("firma", [["4", "4"], ["8", "8"], ["16", "16"], ["32", "32"]], op.firma)}</div>
          <div class="campo"><span>Cómo cuadrar</span>${chips("cuadrar", [["ajustar", "Ajustar el texto"], ["blancas", "Blancas al final"], ["no", "No cuadrar"]], op.cuadrar)}
            <small>«Ajustar» mueve el interlineado y el cuerpo muy poco (máx. ±6 % y ±2,5 %) para cerrar en un múltiplo exacto; lo que falte se completa con blancas antes del colofón.</small></div>
        </section>
      </div>
      <div class="columna-resultado"><div class="resultado" id="dg-resultado"></div></div>
    </div>`;

  const redibujar = () => { const y = window.scrollY; vistaDiagramacion(main, h); window.scrollTo(0, y); };

  // Manuscrito
  const zona = $("#dg-archivo-zona");
  if (zona) {
    const leer = (archivo) => archivo && cargarManuscrito(archivo, h).then(() => { redibujar(); diagramar(h); });
    $("#dg-archivo").addEventListener("change", (e) => leer(e.target.files[0]));
    zona.addEventListener("dragover", (e) => { e.preventDefault(); zona.classList.add("encima"); });
    zona.addEventListener("dragleave", () => zona.classList.remove("encima"));
    zona.addEventListener("drop", (e) => { e.preventDefault(); zona.classList.remove("encima"); leer(e.dataTransfer.files[0]); });
    $("#dg-usar-pegado").addEventListener("click", () => {
      const texto = $("#dg-pegado").value;
      if (!texto.trim()) return;
      D.manuscrito = { nombre: "Texto pegado", texto, md: /^#\s/m.test(texto), imagenes: new Map(), avisos: [] };
      reconvertir(op);
      redibujar();
      diagramar(h);
    });
  }
  $("#dg-cambiar")?.addEventListener("click", () => { D.manuscrito = null; D.pdf = null; D.resultado = null; D.error = null; redibujar(); });

  // Fuentes propias
  $("#dg-subir-fuente")?.addEventListener("change", async (e) => {
    const nuevas = [];
    for (const a of e.target.files) {
      const bytes = new Uint8Array(await a.arrayBuffer());
      const familia = C.familiaDeFuente(bytes);
      if (!familia) { h.avisar(`${a.name} no parece una fuente OpenType o TrueType`); continue; }
      nuevas.push({ nombre: a.name, familia, bytes });
      await h.fuentesDB.poner(a.name, { familia, bytes });
    }
    if (!nuevas.length) return;
    D.fuentes = [...(D.fuentes || []).filter((f) => !nuevas.some((n) => n.nombre === f.nombre)), ...nuevas];
    guardar(h, { ...op, fuente: nuevas[0].familia });
    h.avisar(`Fuente ${nuevas[0].familia} lista`);
    redibujar();
    diagramar(h);
  });
  $("#dg-borrar-fuentes")?.addEventListener("click", async () => {
    for (const f of D.fuentes || []) await h.fuentesDB.borrar(f.nombre);
    D.fuentes = [];
    guardar(h, { ...op, fuente: PREDETERMINADAS.fuente });
    redibujar();
    diagramar(h);
  });

  $("#dg-sugeridos").addEventListener("click", () => {
    const o = leerFormulario();
    o.margenes = C.margenesSugeridos(o.ancho, o.alto);
    guardar(h, o);
    redibujar();
    programar(h);
  });

  function leerFormulario() {
    const v = (id) => $(`#dg-${id}`)?.value ?? "";
    const c = (id) => !!$(`#dg-${id}`)?.checked;
    const o = opciones(h);
    o.ancho = num(v("ancho"), o.ancho); o.alto = num(v("alto"), o.alto); o.rebase = num(v("rebase"), o.rebase);
    o.margenes = { interior: num(v("m-interior"), 18), exterior: num(v("m-exterior"), 15), superior: num(v("m-superior"), 18), inferior: num(v("m-inferior"), 20) };
    o.fresado = c("fresado");
    o.fuente = v("fuente") || o.fuente;
    o.cuerpo = Math.min(Math.max(num(v("cuerpo"), 11), 6), 24);
    o.interlineado = Math.max(num(v("interlineado"), 14.5), o.cuerpo);
    o.sangria = num(v("sangria"), 1.2);
    o.caida = num(v("caida"), o.alto * 0.25);
    o.justificar = c("justificar"); o.silabas = c("silabas"); o.capImpar = c("impar"); o.cornisa = c("cornisa"); o.limpieza = c("limpieza");
    o.datos = { titulo: v("titulo").trim(), subtitulo: v("subtitulo").trim(), autor: v("autor").trim(), editorial: v("editorial").trim(), isbn: v("isbn").trim(), edicion: v("edicion").trim(), ciudad: v("ciudad").trim(), impresor: v("impresor").trim(), creditos: v("creditos").trim() };
    o.preliminares = { ...o.preliminares, portadilla: c("portadilla"), portada: c("portada"), legal: c("legal"), colofon: c("colofon"), dedicatoria: v("dedicatoria") };
    const f = FORMATOS.find(([k]) => k === `${o.ancho}x${o.alto}`);
    o.formato = f ? f[0] : "otro";
    return o;
  }

  $$("input, select, textarea", main).forEach((el) => el.addEventListener("change", () => {
    if (el.type === "file" || el.id === "dg-pegado") return;
    const antes = opciones(h);
    const o = leerFormulario();
    guardar(h, o);
    if (antes.limpieza !== o.limpieza) reconvertir(o);
    if (antes.formato !== o.formato) redibujar();
    programar(h);
  }));
  conectarChips(main, (nombre, valor) => {
    const o = leerFormulario();
    if (nombre === "formato") {
      if (valor !== "otro") {
        const [a, b] = valor.split("x").map(Number);
        o.ancho = a; o.alto = b; o.margenes = C.margenesSugeridos(a, b); o.caida = Math.round(b * 0.25);
      }
      o.formato = valor;
      guardar(h, o);
      redibujar();
    } else if (nombre === "indice") { o.preliminares.indice = valor; guardar(h, o); }
    else if (nombre === "firma") { o.firma = Number(valor); guardar(h, o); }
    else {
      o[nombre] = valor;
      guardar(h, o);
      if (nombre === "comillas") reconvertir(o);
    }
    programar(h);
  });

  if (D.fuentes === null) {
    h.fuentesDB.todas().then((lista) => {
      D.fuentes = lista.map(([nombre, f]) => ({ nombre, familia: f.familia, bytes: f.bytes }));
      if (D.fuentes.length && $("#dg-fuente")) redibujar();
    });
  }
  pintarResultado(h);
}

// ───────────── Estado y opciones ─────────────
function opciones(h) {
  const g = h.estado.preferencias.diagramacion || {};
  return structuredClone({ ...PREDETERMINADAS, ...g, datos: { ...PREDETERMINADAS.datos, ...g.datos }, preliminares: { ...PREDETERMINADAS.preliminares, ...g.preliminares }, margenes: { ...PREDETERMINADAS.margenes, ...g.margenes } });
}
function guardar(h, o) { h.preferir("diagramacion", o); }

async function cargarManuscrito(archivo, h) {
  const op = opciones(h);
  D.error = null;
  D.pdf = null;
  D.resultado = null;
  try {
    if (/\.docx$/i.test(archivo.name)) {
      D.manuscrito = { nombre: archivo.name, docx: new Uint8Array(await archivo.arrayBuffer()) };
      await reconvertir(op);
    } else {
      const texto = await archivo.text();
      D.manuscrito = { nombre: archivo.name, texto, md: /\.(md|markdown)$/i.test(archivo.name) || /^#{1,2}\s/m.test(texto), imagenes: new Map(), avisos: [] };
      reconvertir(op);
    }
    // El título se toma del nombre del archivo si aún no hay uno.
    if (!op.datos.titulo) {
      op.datos.titulo = archivo.name.replace(/\.[^.]+$/, "").replace(/[_-]+/g, " ").trim();
      guardar(h, op);
    }
  } catch (e) {
    D.manuscrito = null;
    h.avisar(`No se pudo leer el manuscrito: ${e.message || e}`);
  }
}

/** Vuelve a convertir el manuscrito (cambia la limpieza o las comillas). */
async function reconvertir(op) {
  const m = D.manuscrito;
  if (!m) return;
  if (m.docx) {
    const r = await C.desdeDocx(m.docx, op);
    Object.assign(m, r);
  } else m.bloques = C.desdeTexto(m.texto, m.md, op);
}

// ───────────── Composición y cuadre ─────────────
let temporizador;
function programar(h) {
  if (!D.manuscrito) return;
  clearTimeout(temporizador);
  temporizador = setTimeout(() => diagramar(h), 600);
}

async function diagramar(h) {
  const m = D.manuscrito;
  if (!m) return;
  if (m.docx && !m.bloques) await reconvertir(opciones(h));
  const gen = ++D.generacion;
  const op = opciones(h);
  const s = op.firma;
  const propias = (D.fuentes || []).filter((f) => f.familia === op.fuente);
  const vigente = () => gen === D.generacion && D.manuscrito === m;
  const estado = (texto) => { if (vigente()) { D.estado = texto; pintarResultado(h); } };
  const cache = new Map();
  const contar = async (a, k = 0) => {
    const clave = `${a.toFixed(3)}:${k}`;
    if (!cache.has(clave)) {
      await h.respirar();
      const pdf = await C.compilar(C.plantilla(m.bloques, op, a, k).fuente, m.imagenes, propias);
      cache.set(clave, { pdf, paginas: JSON.parse(h.motor.analizar(pdf)).paginas.length });
    }
    return cache.get(clave);
  };
  D.error = null;
  try {
    estado(compiladorListo ? "Componiendo el libro…" : "Cargando el compositor tipográfico (solo la primera vez, unos segundos)…");
    await C.prepararCompilador(propias);
    compiladorListo = true;
    estado("Componiendo el libro…");
    const base = await contar(0);
    if (!vigente()) return;
    const n0 = base.paginas;
    let a = 0, k = 0;
    const resto = n0 % s;
    if (resto && op.cuadrar === "blancas") k = s - resto;
    else if (resto && op.cuadrar === "ajustar") {
      const abajo = n0 - resto, arriba = abajo + s;
      const mayorQueCumple = async (lo, hi, meta) => {
        // Bisección: el mayor ajuste (menos apretado) cuyo conteo no pasa la meta.
        for (let i = 0; i < 4; i++) {
          const mid = (lo + hi) / 2;
          estado(`Cuadrando en ${meta} páginas… (prueba ${i + 2})`);
          if ((await contar(mid)).paginas <= meta) lo = mid; else hi = mid;
          if (!vigente()) return lo;
        }
        return lo;
      };
      estado(`Cuadrando: ${n0} páginas, buscando ${abajo || arriba}…`);
      if (abajo > 0 && (await contar(-1)).paginas <= abajo) {
        a = await mayorQueCumple(-1, 0, abajo);
        k = abajo - (await contar(a)).paginas;
      } else {
        const suelto = await contar(1);
        let tope = suelto.paginas <= arriba ? 1 : await mayorQueCumple(0, 1, arriba);
        const meta = (await contar(tope)).paginas;
        // El menor ajuste que ya llega a esas páginas: soltar solo lo necesario.
        let lo = 0;
        for (let i = 0; i < 4 && meta > n0 && vigente(); i++) {
          const mid = (lo + tope) / 2;
          estado(`Cuadrando en ${arriba} páginas… (afinando ${i + 1})`);
          if ((await contar(mid)).paginas >= meta) tope = mid; else lo = mid;
        }
        a = meta > n0 ? tope : 0;
        k = arriba - (await contar(a)).paginas;
      }
      if (!vigente()) return;
    }
    estado("Preparando el PDF…");
    let final = await contar(a, k);
    // Si las blancas no dieron el múltiplo exacto (saltos a impar), corrige una vez.
    if (op.cuadrar !== "no" && final.paginas % s) { k += s - (final.paginas % s); final = await contar(a, k); }
    if (!vigente()) return;
    const pdf = h.motor.fijar_cajas(final.pdf, op.rebase);
    const fuenteTypst = C.plantilla(m.bloques, op, a, k);
    D.pdf = pdf;
    D.imagenes = {};
    D.pdfjs = null;
    D.pliego = Math.min(D.pliego, Math.floor(final.paginas / 2));
    D.resultado = { paginas: final.paginas, n0, ajuste: a, blancas: k, firma: s, cuerpo: fuenteTypst.cuerpo, paso: fuenteTypst.paso, fuente: fuenteTypst.fuente, op };
    D.estado = "";
    pintarResultado(h);
  } catch (e) {
    if (!vigente()) return;
    D.estado = "";
    D.error = String(e.message || e);
    pintarResultado(h);
  }
}
let compiladorListo = false;

// ───────────── Resultado y vista previa ─────────────
function pintarResultado(h) {
  const caja = h.$("#dg-resultado");
  if (!caja) return;
  const { esc, mm } = h;
  const r = D.resultado;
  let html = "";
  if (D.error) html += `<div class="error-caja">No se pudo componer: ${esc(D.error)}</div>`;
  if (D.estado) html += `<section class="tarjeta-oscura"><div class="componiendo"><span class="orbe orbe-respira" aria-hidden="true"></span><p>${esc(D.estado)}</p></div></section>`;
  if (r && !D.estado) {
    const cuadernillos = r.paginas / r.firma;
    const exacto = Number.isInteger(cuadernillos);
    const cambio = r.ajuste ? ` Para cerrar en <span class="resaltado">${r.paginas} páginas</span> se ${r.ajuste < 0 ? "apretó" : "soltó"} la composición a ${mm(r.cuerpo, 2)}/${mm(r.paso, 2)} pt (de ${mm(r.op.cuerpo, 2)}/${mm(r.op.interlineado, 2)}).` : "";
    const blancas = r.blancas ? ` Se agregaron <b>${r.blancas}</b> ${r.blancas === 1 ? "página en blanco" : "páginas en blanco"}${r.op.preliminares.colofon ? " antes del colofón" : " al final"}.` : "";
    html += `<section class="tarjeta-oscura">
      <div class="metricas">
        <div class="metrica"><b>${r.paginas}</b><span>páginas</span></div>
        <div class="metrica"><b>${exacto ? cuadernillos : mm(cuadernillos, 1)}</b><span>cuadernillos de ${r.firma}</span></div>
        <div class="metrica"><b>${mm(r.op.ancho, 0)}×${mm(r.op.alto, 0)}</b><span>mm al corte, +${mm(r.op.rebase)} de rebase</span></div>
      </div>
      <p class="explicacion" style="margin-top:20px">${exacto ? `Cuadrado: <span class="resaltado">${cuadernillos} × ${r.firma} pp</span>.` : `Quedan ${r.paginas % r.firma} páginas sueltas: el montaje agregará blancas.`}${cambio}${blancas}</p>
      <div class="acciones-resultado" style="margin-top:20px">
        <button class="boton boton-blanco" id="dg-descargar" type="button">Descargar interior</button>
        <button class="boton boton-naranja" id="dg-a-libro" type="button">Montar en Libros →</button>
        <button class="boton boton-claro" id="dg-a-portada" type="button">Hacer la portada →</button>
      </div>
      <p class="tenue" style="margin-top:14px;font-size:13px"><button type="button" class="enlace" id="dg-typ">Descargar la fuente Typst (.typ)</button> para retocarla a mano.</p>
    </section>`;
  }
  html += vistaPliego(h);
  caja.innerHTML = html;
  conectarResultado(h, caja);
}

function vistaPliego(h) {
  if (!D.pdf) {
    return `<section class="tarjeta vista-previa"><div class="lienzo"><div class="vacio"><span class="orbe orbe-respira" aria-hidden="true"></span><p>${D.manuscrito ? "Componiendo…" : "Sube el manuscrito para ver el libro"}</p></div></div></section>`;
  }
  const total = D.resultado.paginas;
  const pliegos = Math.floor(total / 2) + 1;
  const i = Math.min(D.pliego, pliegos - 1);
  const izq = i * 2, der = i * 2 + 1; // páginas 1-indexadas: izq par, der impar
  const pagina = (n) => (n >= 1 && n <= total
    ? (D.imagenes[n] ? `<img src="${D.imagenes[n]}" alt="Página ${n}">` : `<div class="pagina-cargando" style="aspect-ratio:${D.resultado.op.ancho}/${D.resultado.op.alto}"></div>`)
    : `<div class="pagina-vacia" style="aspect-ratio:${D.resultado.op.ancho}/${D.resultado.op.alto}"></div>`);
  const pedir = [izq, der].filter((n) => n >= 1 && n <= total && !(n in D.imagenes));
  if (pedir.length) setTimeout(() => miniaturas(h, pedir), 0);
  const nombre = izq < 1 ? "Página 1" : der > total ? `Página ${izq}` : `Páginas ${izq} y ${der}`;
  return `<section class="tarjeta vista-previa">
    <div class="vista-previa-cabeza">
      <h3>${nombre} <span class="tenue num">· pliego ${i + 1} de ${pliegos}</span></h3>
      <div class="paginador">
        <button class="boton boton-claro boton-chico" data-pliego="${i - 1}" ${i === 0 ? "disabled" : ""} aria-label="Pliego anterior">←</button>
        <input type="range" min="0" max="${pliegos - 1}" value="${i}" id="dg-deslizar" aria-label="Ir al pliego">
        <button class="boton boton-claro boton-chico" data-pliego="${i + 1}" ${i === pliegos - 1 ? "disabled" : ""} aria-label="Pliego siguiente">→</button>
      </div>
    </div>
    <div class="lienzo"><div class="pliego-libro">${pagina(izq)}${pagina(der)}</div></div>
    <div class="leyenda"><span>Las páginas se ven con su rebase; la línea punteada del PDF es el corte (TrimBox).</span></div>
  </section>`;
}

async function miniaturas(h, paginas) {
  const pdf = D.pdf;
  try {
    const lib = await h.cargarPdfjs();
    D.pdfjs ??= lib.getDocument({ data: pdf.slice() }).promise;
    const doc = await D.pdfjs;
    for (const n of paginas) {
      if (D.pdf !== pdf || n in D.imagenes) continue;
      D.imagenes[n] = null;
      const p = await doc.getPage(n);
      const base = p.getViewport({ scale: 1 });
      const vp = p.getViewport({ scale: 900 / base.height });
      const lienzo = Object.assign(document.createElement("canvas"), { width: Math.ceil(vp.width), height: Math.ceil(vp.height) });
      const ctx = lienzo.getContext("2d");
      ctx.fillStyle = "#fff";
      ctx.fillRect(0, 0, lienzo.width, lienzo.height);
      await p.render({ canvasContext: ctx, viewport: vp }).promise;
      // Línea de corte, como guía.
      const r = (D.resultado.op.rebase / 25.4) * 72 * vp.scale;
      ctx.setLineDash([6, 5]);
      ctx.strokeStyle = "rgba(217,59,43,.55)";
      ctx.strokeRect(r, r, lienzo.width - 2 * r, lienzo.height - 2 * r);
      D.imagenes[n] = lienzo.toDataURL("image/jpeg", 0.85);
    }
  } catch (e) {
    console.warn("Vista previa no disponible:", e);
    return;
  }
  if (D.pdf === pdf && !D.estado) pintarResultado(h);
}

function conectarResultado(h, caja) {
  const { $, $$ } = h;
  $$("[data-pliego]", caja).forEach((b) => b.addEventListener("click", () => { D.pliego = Number(b.dataset.pliego); pintarResultado(h); }));
  $("#dg-deslizar", caja)?.addEventListener("input", (e) => { D.pliego = Number(e.target.value); pintarResultado(h); });
  const r = D.resultado;
  if (!r) return;
  const nombre = (r.op.datos.titulo || "libro").replace(/[\\/:*?"<>|]+/g, "").trim() || "libro";
  $("#dg-descargar", caja)?.addEventListener("click", () => { h.descargar(D.pdf, `${nombre}-interior.pdf`); h.avisar(`Interior de ${r.paginas} páginas descargado`); });
  $("#dg-typ", caja)?.addEventListener("click", () => {
    const url = URL.createObjectURL(new Blob([r.fuente], { type: "text/plain" }));
    const a = Object.assign(document.createElement("a"), { href: url, download: `${nombre}.typ` });
    document.body.append(a); a.click(); a.remove();
    setTimeout(() => URL.revokeObjectURL(url), 10_000);
  });
  $("#dg-a-libro", caja)?.addEventListener("click", () => h.enviarALibro({ nombre: `${nombre}-interior.pdf`, tamano: D.pdf.length, bytes: D.pdf }));
  $("#dg-a-portada", caja)?.addEventListener("click", () => h.enviarAPortada(r.op.ancho, r.op.alto, r.paginas));
}
