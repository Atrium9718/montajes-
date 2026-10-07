// Composición tipográfica del interior de un libro: convierte el manuscrito
// (Word, Markdown o texto) en un documento Typst con reglas editoriales y lo
// compila a PDF dentro del navegador con typst.ts. Aquí no hay interfaz: solo
// el manuscrito, la plantilla y el compilador.

const TYPST = "https://cdn.jsdelivr.net/npm/@myriaddreamin/typst.ts@0.7.0/dist/esm/contrib/all-in-one-lite.bundle.js";
const COMPILADOR = "https://cdn.jsdelivr.net/npm/@myriaddreamin/typst-ts-web-compiler@0.7.0/pkg/typst_ts_web_compiler_bg.wasm";
const MAMMOTH = "https://cdnjs.cloudflare.com/ajax/libs/mammoth/1.13.0/mammoth.browser.min.js";

/** Fuentes que el compilador trae incluidas. */
export const FUENTES_INCLUIDAS = ["Libertinus Serif", "New Computer Modern"];

// ───────────── Manuscrito ─────────────
// Un manuscrito es una lista de bloques; el texto en línea ya va en marcado
// Typst escapado (negrita, cursiva y notas incluidas).
//   { t: "cap", etiqueta, titulo, especial }   capítulo (abre página)
//   { t: "parte", etiqueta, titulo }           portadilla de parte
//   { t: "sec", texto }                        subtítulo dentro del capítulo
//   { t: "p", texto } · { t: "cita", texto } · { t: "lista", orden, items }
//   { t: "sep" } · { t: "img", ruta, ancho, alto } · { t: "tabla", filas }

const ESPECIALES = /^(pr[oó]logo|ep[ií]logo|introducci[oó]n|prefacio|presentaci[oó]n|agradecimientos|nota (del|de la) (autor|autora|editor|editora|traductor|traductora)|nota preliminar|ap[eé]ndice|glosario|bibliograf[ií]a|post ?scriptum|pre[aá]mbulo|a modo de pr[oó]logo)\b/i;
const NUMERO = "(?:\\d+|[ivxlcdm]+|uno|dos|tres|cuatro|cinco|seis|siete|ocho|nueve|diez|once|doce|trece|catorce|quince|dieciséis|dieciseis|diecisiete|dieciocho|diecinueve|veinte|veinti\\S+|treinta(?: y \\S+)?|cuarenta(?: y \\S+)?|cincuenta(?: y \\S+)?|primer[oa]?|segund[oa]|tercer[oa]?|cuart[oa]|quint[oa]|sext[oa]|s[eé]ptim[oa]|octav[oa]|noven[oa]|d[eé]cim[oa]|[uú]ltim[oa])";
const ETIQUETA = new RegExp(`^((?:cap[ií]tulo|parte|libro|canto|jornada)\\s+${NUMERO})\\b\\s*[.:—–-]?\\s*(.*)$`, "i");
const SEPARADOR = /^\s*(\*\s*){3,}$|^\s*(-\s*){3,}$|^\s*(_\s*){3,}$|^\s*#\s*$|^\s*[⁂§~]\s*$|^\s*(·\s*){3,}$/;

/** ¿El texto de un párrafo parece el título de un capítulo o parte? */
function pareceTitulo(texto) {
  const t = texto.trim();
  if (!t || t.length > 90) return null;
  const m = t.match(ETIQUETA);
  if (m) return { parte: /^(parte|libro)\b/i.test(m[1]), etiqueta: m[1], resto: m[2].replace(/[.:]$/, "").trim() };
  if (ESPECIALES.test(t) && t.length < 60 && !/[.,;]$/.test(t)) return { especial: true, etiqueta: null, resto: t.replace(/[.:]$/, "") };
  return null;
}

/** Arma el bloque de capítulo/parte; si la etiqueta viene sola, toma el siguiente párrafo corto como título. */
function bloqueTitulo(info, siguiente) {
  let titulo = info.resto;
  let consumir = false;
  if (!titulo && siguiente && siguiente.length <= 80 && !/[.,;:]$/.test(siguiente.trim()) && !/^[—–-]/.test(siguiente.trim()) && !pareceTitulo(siguiente)) {
    titulo = siguiente.trim();
    consumir = true;
  }
  const bloque = info.parte
    ? { t: "parte", etiqueta: titulo ? info.etiqueta : null, titulo: titulo || info.etiqueta }
    : { t: "cap", etiqueta: titulo ? info.etiqueta : null, titulo: titulo || info.etiqueta, especial: !!info.especial };
  return { bloque, consumir };
}

/** Limpieza editorial del texto: rayas de diálogo, puntos suspensivos, comillas y espacios. */
export function limpiarTexto(t, op = {}, inicioParrafo = false) {
  if (op.limpieza === false) return t;
  let s = t.replace(/[ \t\u00a0]{2,}/g, " ");
  s = s.replace(/\.\.\./g, "…").replace(/---?/g, "—");
  if (inicioParrafo) s = s.replace(/^\s*[-–]\s?(?=\S)/, "—");
  // Incisos del narrador: «dijo -con voz baja- y salió» → rayas.
  s = s.replace(/(\s)[-–](\s?)(?=[A-Za-zÁÉÍÓÚÑáéíóúñ¿¡])/g, (m, a, b) => (b ? m : `${a}—`));
  s = s.replace(/([\p{L}.,;!?…])[-–](?=[\s.,;:]|$)/gu, "$1—");
  if (op.comillas === "angulares" || op.comillas === "inglesas") {
    const [abre, cierra] = op.comillas === "angulares" ? ["«", "»"] : ["“", "”"];
    s = s.replace(/"/g, (_, i) => (i === 0 || /[\s([{—¡¿]/.test(s[i - 1]) ? abre : cierra));
    if (op.comillas === "angulares") s = s.replace(/[“]/g, "«").replace(/[”]/g, "»");
    s = s.replace(/'/g, "’");
  }
  return s;
}

/** Escapa texto para el modo marcado de Typst. */
export function escaparTypst(t) {
  return t.replace(/[\\#*_`$<>@[\]~=+/'"-]/g, "\\$&").replace(/^(\d+)\./, "$1\\.");
}

/** Cadena literal de Typst. */
export const cadena = (t) => `"${String(t ?? "").replace(/\\/g, "\\\\").replace(/"/g, '\\"').replace(/\r?\n/g, "\\n")}"`;

/** Marcado en línea mínimo de Markdown (negrita, cursiva) → Typst, escapando el resto. */
function enLineaMarkdown(texto, op, inicio) {
  const limpio = limpiarTexto(texto, op, inicio);
  let salida = "";
  const re = /(\*\*|__)(.+?)\1|(\*|_)(?!\s)(.+?)(?<!\s)\3/g;
  let ultimo = 0;
  for (const m of limpio.matchAll(re)) {
    salida += escaparTypst(limpio.slice(ultimo, m.index));
    salida += m[1] ? `#strong[${escaparTypst(m[2])}];` : `#emph[${escaparTypst(m[4])}];`;
    ultimo = m.index + m[0].length;
  }
  return salida + escaparTypst(limpio.slice(ultimo));
}

/** Texto plano o Markdown → bloques. */
export function desdeTexto(texto, esMarkdown, op = {}) {
  const lineas = texto.replace(/\r\n?/g, "\n").replace(/^\ufeff/, "").split("\n");
  const vacias = lineas.filter((l) => !l.trim()).length;
  // Muchos .txt traen un párrafo por línea, sin líneas en blanco entre ellos.
  const porLinea = !esMarkdown && vacias < lineas.length * 0.15;
  const parrafos = [];
  let actual = [];
  const cerrar = () => { if (actual.length) parrafos.push(actual.join(esMarkdown ? "\n" : " ")); actual = []; };
  for (const l of lineas) {
    if (!l.trim()) { cerrar(); continue; }
    if (porLinea || (esMarkdown && /^(#{1,6}\s|>\s?|[-*+]\s|\d+[.)]\s)/.test(l.trim())) || SEPARADOR.test(l)) { cerrar(); parrafos.push(l); continue; }
    actual.push(l.trim());
  }
  cerrar();

  const bloques = [];
  for (let i = 0; i < parrafos.length; i++) {
    const p = parrafos[i].trim();
    if (SEPARADOR.test(p)) { if (bloques.length) bloques.push({ t: "sep" }); continue; }
    const h = esMarkdown && p.match(/^(#{1,6})\s+(.*)$/);
    if (h) {
      const nivel = h[1].length;
      const texto = h[2].replace(/\s+#+\s*$/, "");
      if (nivel === 1) {
        const info = pareceTitulo(texto) || { etiqueta: null, resto: texto, especial: ESPECIALES.test(texto) };
        const sig = parrafos[i + 1]?.trim().match(/^#{2,6}\s+(.*)$/);
        const { bloque, consumir } = bloqueTitulo(info, sig ? sig[1] : null);
        bloques.push(bloque);
        if (consumir) i++;
      } else bloques.push({ t: "sec", texto: enLineaMarkdown(texto, op) });
      continue;
    }
    if (esMarkdown && /^>/.test(p)) { bloques.push({ t: "cita", texto: enLineaMarkdown(p.replace(/^>\s?/gm, "").replace(/\n/g, " "), op, true) }); continue; }
    if (esMarkdown && /^([-*+]|\d+[.)])\s/.test(p)) {
      const orden = /^\d/.test(p);
      const items = [];
      while (i < parrafos.length && /^([-*+]|\d+[.)])\s/.test(parrafos[i].trim())) {
        items.push(enLineaMarkdown(parrafos[i].trim().replace(/^([-*+]|\d+[.)])\s+/, ""), op));
        i++;
      }
      i--;
      bloques.push({ t: "lista", orden, items });
      continue;
    }
    const plano = p.replace(/\n/g, " ");
    const info = pareceTitulo(plano);
    if (info) {
      const { bloque, consumir } = bloqueTitulo(info, parrafos[i + 1]);
      bloques.push(bloque);
      if (consumir) i++;
      continue;
    }
    bloques.push({ t: "p", texto: esMarkdown ? enLineaMarkdown(plano, op, true) : escaparTypst(limpiarTexto(plano, op, true)) });
  }
  return bloques;
}

// ───────────── Word (.docx) con mammoth ─────────────
let mammothListo = null;
function cargarMammoth() {
  mammothListo ??= new Promise((ok, mal) => {
    if (window.mammoth) return ok(window.mammoth);
    const s = Object.assign(document.createElement("script"), { src: MAMMOTH, async: true });
    s.onload = () => ok(window.mammoth);
    s.onerror = () => { mammothListo = null; mal(new Error("no se pudo cargar el lector de Word")); };
    document.head.append(s);
  });
  return mammothListo;
}

const ESTILOS_WORD = [
  "p[style-name='Title'] => h1:fresh", "p[style-name='Título'] => h1:fresh",
  "p[style-name='Subtitle'] => h2:fresh", "p[style-name='Subtítulo'] => h2:fresh",
  "p[style-name='Quote'] => blockquote > p:fresh", "p[style-name='Intense Quote'] => blockquote > p:fresh",
  "p[style-name='Cita'] => blockquote > p:fresh", "p[style-name='Block Text'] => blockquote > p:fresh",
];

/** Documento de Word → { bloques, imagenes: Map(ruta → bytes), avisos }. */
export async function desdeDocx(bytes, op = {}) {
  const mammoth = await cargarMammoth();
  const imagenes = new Map();
  const dims = new Map();
  let n = 0;
  const r = await mammoth.convertToHtml({ arrayBuffer: bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) }, {
    styleMap: ESTILOS_WORD,
    convertImage: mammoth.images.imgElement(async (img) => {
      const datos = await img.read();
      const ext = /png/.test(img.contentType) ? "png" : /jpe?g/.test(img.contentType) ? "jpg" : /gif/.test(img.contentType) ? "gif" : /svg/.test(img.contentType) ? "svg" : null;
      if (!ext) return { src: "" };
      const ruta = `/img/${++n}.${ext}`;
      const b = new Uint8Array(datos);
      imagenes.set(ruta, b);
      dims.set(ruta, await medirImagen(b, img.contentType));
      return { src: ruta };
    }),
  });
  const doc = new DOMParser().parseFromString(`<body>${r.value}</body>`, "text/html");
  // Notas al pie: mammoth las pone al final en <li id="footnote-N">.
  const notas = new Map();
  for (const li of doc.querySelectorAll('li[id^="footnote-"], li[id^="endnote-"]')) {
    li.querySelectorAll('a[href^="#footnote-ref-"], a[href^="#endnote-ref-"]').forEach((a) => a.remove());
    notas.set(li.id, li);
    li.closest("ol")?.setAttribute("data-notas", "");
  }
  const bloques = [];
  const avisos = r.messages.filter((m) => m.type === "error").map((m) => m.message);
  const enLinea = (nodo, inicio) => inline(nodo, notas, op, inicio);
  const nodos = [...doc.body.children].filter((el) => !el.hasAttribute("data-notas"));
  for (let i = 0; i < nodos.length; i++) {
    const el = nodos[i];
    const tag = el.tagName.toLowerCase();
    const plano = el.textContent.replace(/\s+/g, " ").trim();
    const imgs = [...el.querySelectorAll("img")].filter((im) => im.getAttribute("src"));
    if (/^h[1-6]$/.test(tag)) {
      if (!plano) continue;
      if (tag === "h1") {
        const info = pareceTitulo(plano) || { etiqueta: null, resto: plano, especial: ESPECIALES.test(plano), parte: false };
        const sig = nodos[i + 1];
        const sigTexto = sig && /^h[2-6]$/i.test(sig.tagName) ? sig.textContent.trim() : null;
        const { bloque, consumir } = bloqueTitulo(info, sigTexto);
        bloques.push(bloque);
        if (consumir) i++;
      } else bloques.push({ t: "sec", texto: enLinea(el) });
      continue;
    }
    if (tag === "blockquote") { for (const p of el.querySelectorAll("p")) bloques.push({ t: "cita", texto: enLinea(p, true) }); continue; }
    if (tag === "ul" || tag === "ol") { bloques.push({ t: "lista", orden: tag === "ol", items: [...el.children].map((li) => enLinea(li)) }); continue; }
    if (tag === "table") {
      const filas = [...el.querySelectorAll("tr")].map((tr) => [...tr.children].map((td) => enLinea(td)));
      if (filas.length) bloques.push({ t: "tabla", filas });
      continue;
    }
    for (const im of imgs) { const ruta = im.getAttribute("src"); bloques.push({ t: "img", ruta, ...dims.get(ruta) }); }
    if (!plano) continue;
    if (SEPARADOR.test(plano)) { if (bloques.length) bloques.push({ t: "sep" }); continue; }
    const info = pareceTitulo(plano);
    if (info && !el.querySelector('a[href^="#footnote"]')) {
      const sig = nodos[i + 1];
      const { bloque, consumir } = bloqueTitulo(info, sig?.tagName === "P" ? sig.textContent.replace(/\s+/g, " ").trim() : null);
      bloques.push(bloque);
      if (consumir) i++;
      continue;
    }
    bloques.push({ t: "p", texto: enLinea(el, true) });
  }
  return { bloques, imagenes, avisos };
}

function inline(nodo, notas, op, inicio = false) {
  let salida = "";
  let primero = inicio;
  for (const h of nodo.childNodes) {
    if (h.nodeType === 3) {
      const texto = h.textContent.replace(/\s+/g, " ");
      salida += escaparTypst(limpiarTexto(primero ? texto.replace(/^\s+/, "") : texto, op, primero));
      if (texto.trim()) primero = false;
      continue;
    }
    if (h.nodeType !== 1) continue;
    const tag = h.tagName.toLowerCase();
    const dentro = () => { const r = inline(h, notas, op, primero); if (h.textContent.trim()) primero = false; return r; };
    if (tag === "img") continue;
    if (tag === "br") { salida += " \\ "; continue; }
    const nota = tag === "sup" && h.querySelector('a[href^="#footnote-"], a[href^="#endnote-"]');
    if (nota || (tag === "a" && /^#(foot|end)note-\d/.test(h.getAttribute("href") || ""))) {
      const id = (nota || h).getAttribute("href").slice(1);
      const li = notas.get(id);
      if (li) salida += `#footnote[${[...li.querySelectorAll("p")].map((p) => inline(p, notas, op).trim()).join(" \\ ") || inline(li, notas, op).trim()}];`;
      continue;
    }
    const contenido = dentro();
    if (!contenido.trim()) { salida += contenido; continue; }
    if (tag === "strong" || tag === "b") salida += `#strong[${contenido}];`;
    else if (tag === "em" || tag === "i") salida += `#emph[${contenido}];`;
    else if (tag === "u") salida += `#underline[${contenido}];`;
    else if (tag === "s" || tag === "del") salida += `#strike[${contenido}];`;
    else if (tag === "sup") salida += `#super[${contenido}];`;
    else if (tag === "sub") salida += `#sub[${contenido}];`;
    else salida += contenido;
  }
  return salida.replace(/^\s+|\s+$/g, "");
}

function medirImagen(bytes, tipo) {
  return new Promise((ok) => {
    const url = URL.createObjectURL(new Blob([bytes], { type: tipo }));
    const im = new Image();
    im.onload = () => { ok({ ancho: im.naturalWidth || 1, alto: im.naturalHeight || 1 }); URL.revokeObjectURL(url); };
    im.onerror = () => { ok({ ancho: 4, alto: 3 }); URL.revokeObjectURL(url); };
    im.src = url;
  });
}

/** Resumen del manuscrito para la interfaz. */
export function resumen(bloques) {
  let palabras = 0, notas = 0;
  const contar = (t) => { palabras += (t.replace(/#\w+\[|\];?|\\./g, " ").match(/[\p{L}\p{N}]+/gu) || []).length; notas += (t.match(/#footnote\[/g) || []).length; };
  for (const b of bloques) {
    if (b.texto) contar(b.texto);
    if (b.items) b.items.forEach(contar);
    if (b.filas) b.filas.flat().forEach(contar);
  }
  return {
    palabras, notas,
    capitulos: bloques.filter((b) => b.t === "cap").map((b) => (b.etiqueta ? `${b.etiqueta}: ${b.titulo}` : b.titulo)),
    partes: bloques.filter((b) => b.t === "parte").length,
    imagenes: bloques.filter((b) => b.t === "img").length,
    parrafos: bloques.filter((b) => b.t === "p").length,
  };
}

// ───────────── Plantilla editorial (Typst) ─────────────
const romano = (n) => [["M", 1000], ["CM", 900], ["D", 500], ["CD", 400], ["C", 100], ["XC", 90], ["L", 50], ["XL", 40], ["X", 10], ["IX", 9], ["V", 5], ["IV", 4], ["I", 1]]
  .reduce((s, [l, v]) => { while (n >= v) { s += l; n -= v; } return s; }, "");
const MESES = ["enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre", "octubre", "noviembre", "diciembre"];
const r2 = (v) => Math.round(v * 100) / 100;

/** Márgenes sugeridos (mm) para un formato: proporción clásica, interior mayor. */
export function margenesSugeridos(ancho, alto) {
  const m = (v) => Math.round(v * 2) / 2;
  return { interior: m(ancho * 0.145), exterior: m(ancho * 0.11), superior: m(alto * 0.085), inferior: m(alto * 0.105) };
}

/**
 * Fuente Typst del libro.
 * @param ajuste −1…1: aprieta (−) o suelta (+) interlineado y cuerpo para cuadrar páginas.
 * @param blancas páginas en blanco que se agregan antes del colofón.
 */
export function plantilla(bloques, op, ajuste = 0, blancas = 0) {
  const d = op.datos || {};
  const rebase = Math.max(0, op.rebase || 0);
  const mg = op.margenes;
  const interior = mg.interior + (op.fresado ? op.fresadoMm || 3 : 0);
  const cuerpo = r2(op.cuerpo * (1 + 0.025 * ajuste));
  const paso = r2(op.interlineado * (1 + 0.06 * ajuste));
  const titulo = d.titulo || "Sin título";
  const fuentes = [op.fuente, ...FUENTES_INCLUIDAS].filter((f, i, a) => f && a.indexOf(f) === i);
  const L = [];
  L.push(`// Generado por Montajes · diagramación. Formato ${op.ancho} × ${op.alto} mm, rebase ${rebase} mm.`);
  L.push(`#let titulo-libro = ${cadena(titulo)}`);
  L.push(`#let cornisa-par = ${cadena(op.cornisaPar === "autor" && d.autor ? d.autor : titulo)}`);
  L.push(`#let folio-en-cabeza = ${op.folio === "cabeza"}`);
  L.push(`#let con-cornisa = ${op.cornisa !== false}`);
  L.push(`#let caida = ${r2(op.caida ?? op.alto * 0.25)}mm`);
  L.push(String.raw`#let primera-apertura() = { let c = query(heading.where(level: 1)); if c.len() > 0 { c.first().location().page() } else { 1 } }
#let es-apertura(p) = query(heading.where(level: 1)).any(h => h.location().page() == p)
#let sin-adornos(p) = {
  if p < primera-apertura() { return true }
  let f = query(<final>)
  if f.len() > 0 and p > f.first().location().page() { return true }
  if query(<parte>).any(h => h.location().page() == p) { return true }
  query(<fin>).any(m => m.location().page() == p - 1 and {
    let s = query(heading.where(level: 1).after(m.location()))
    s.len() > 0 and s.first().location().page() > p
  })
}
#let cabeza = context {
  let p = here().page()
  if not sin-adornos(p) and not es-apertura(p) {
    set text(size: 0.78em, tracking: 0.05em, hyphenate: false)
    let texto = if calc.even(p) { cornisa-par } else {
      let c = query(heading.where(level: 1).before(here()))
      if c.len() > 0 { c.last().body } else { titulo-libro }
    }
    let texto = if con-cornisa { smallcaps(texto) } else { [] }
    let folio = counter(page).display()
    if folio-en-cabeza {
      if calc.even(p) { grid(columns: (auto, 1fr), column-gutter: 1.2em, folio, align(left, texto)) }
      else { grid(columns: (1fr, auto), column-gutter: 1.2em, align(right, texto), folio) }
    } else if con-cornisa { align(center, texto) }
  }
}
#let pie = context {
  let p = here().page()
  if not sin-adornos(p) and (not folio-en-cabeza or es-apertura(p)) {
    align(center, text(size: 0.85em, counter(page).display()))
  }
}`);
  const margen = (v) => `${r2(v + rebase)}mm`;
  L.push(`#set page(width: ${r2(op.ancho + 2 * rebase)}mm, height: ${r2(op.alto + 2 * rebase)}mm, binding: left, numbering: "1",
  margin: (inside: ${margen(interior)}, outside: ${margen(mg.exterior)}, top: ${margen(mg.superior)}, bottom: ${margen(mg.inferior)}),
  header: cabeza, footer: pie, header-ascent: 38%, footer-descent: 38%)`);
  L.push(`#set document(title: titulo-libro${d.autor ? `, author: ${cadena(d.autor)}` : ""})`);
  L.push(`#set text(font: (${fuentes.map(cadena).join(", ")}), size: ${cuerpo}pt, lang: "es", hyphenate: ${op.silabas !== false}, top-edge: 0.8em, bottom-edge: -0.2em, costs: (widow: 100%, orphan: 100%))`);
  L.push(`#set par(justify: ${op.justificar !== false}, leading: ${r2(paso - cuerpo)}pt, spacing: ${r2(paso - cuerpo)}pt, first-line-indent: ${op.sangria}em)`);
  L.push(String.raw`#show heading: set text(weight: "regular", hyphenate: false)
#show heading: set par(justify: false, first-line-indent: 0pt)
#show heading.where(level: 1): it => block(width: 100%, below: 2.4em, align(center, text(size: 1.7em, it.body)))
#show heading.where(level: 2): it => block(above: 1.8em, below: 1.1em, sticky: true, text(size: 1.05em, weight: "bold", it.body))
#show heading.where(level: 3): it => block(above: 1.4em, below: 0.9em, sticky: true, emph(it.body))
#show footnote.entry: set par(first-line-indent: 0pt)
#set footnote.entry(separator: line(length: 22%, stroke: 0.4pt))
#show figure: set block(above: 1.4em, below: 1.4em)
#let fin = [#metadata("fin") <fin>]
#let etiqueta-cap(e) = if e != none { align(center, text(size: 0.82em, tracking: 0.18em, upper(e))); v(0.7em) }`);
  L.push(`#let capitulo(etiqueta, titulo) = {
  fin
  pagebreak(weak: true${op.capImpar !== false ? `, to: "odd"` : ""})
  v(caida)
  etiqueta-cap(etiqueta)
  heading(level: 1, titulo)
}
#let parte(etiqueta, titulo) = {
  fin
  pagebreak(weak: true, to: "odd")
  v(1fr)
  etiqueta-cap(etiqueta)
  [#heading(level: 1, titulo) <parte>]
  v(1.5fr)
}
#let separador() = align(center, block(above: 1.3em, below: 1.3em, text("*" + h(1.2em) + "*" + h(1.2em) + "*")))
#let cita(c) = pad(x: 1.6em, block(above: 1.1em, below: 1.1em, { set par(first-line-indent: 0pt); text(size: 0.92em, c) }))`);

  // Preliminares
  const pre = op.preliminares || {};
  const recto = () => L.push(`#pagebreak(weak: true, to: "odd")`);
  if (pre.portadilla !== false) {
    recto();
    L.push(`#v(22%)\n#align(center, text(size: 1.35em, tracking: 0.04em, smallcaps(titulo-libro)))`);
  }
  if (pre.portada !== false) {
    recto();
    L.push(`#align(center)[${d.autor ? `#text(size: 1.15em, tracking: 0.08em, upper(${cadena(d.autor)}))` : ""}]`);
    L.push(`#v(16%)\n#align(center, text(size: 2.3em, hyphenate: false, titulo-libro))`);
    if (d.subtitulo) L.push(`#v(0.8em)\n#align(center, text(size: 1.15em, style: "italic", ${cadena(d.subtitulo)}))`);
    L.push(`#v(1fr)\n#align(center, text(size: 0.95em, tracking: 0.12em, upper(${cadena(d.editorial || "")})))`);
  }
  if (pre.legal !== false) {
    L.push(`#pagebreak(weak: true)`);
    const hoy = new Date();
    const anio = d.anio || hoy.getFullYear();
    const lineas = [
      d.titulo ? `#emph[${escaparTypst(titulo)}]` : "",
      d.autor ? `© ${escaparTypst(String(anio))}, ${escaparTypst(d.autor)}` : "",
      d.editorial ? `© ${escaparTypst(String(anio))}, de esta edición: ${escaparTypst(d.editorial)}` : "",
      "",
      d.isbn ? `ISBN: ${escaparTypst(d.isbn)}` : "",
      d.deposito ? `Depósito legal: ${escaparTypst(d.deposito)}` : "",
      `${escaparTypst(d.edicion || "Primera edición")}: ${MESES[hoy.getMonth()]} de ${anio}`,
      d.creditos ? escaparTypst(d.creditos).replace(/\n/g, " \\ ") : "",
      "",
      d.impresor || d.ciudad ? `Impreso en ${escaparTypst([d.impresor, d.ciudad].filter(Boolean).join(", "))}` : "",
      "Todos los derechos reservados. Ninguna parte de esta publicación puede ser reproducida, almacenada o transmitida en forma alguna ni por ningún medio sin permiso previo y por escrito de los titulares del copyright.",
    ];
    const texto = lineas.reduce((a, l) => (l === "" ? a + " \\ " : a + (a && !a.endsWith(" \\ ") ? " \\ " : "") + l), "");
    L.push(`#v(1fr)\n#block(width: 100%, { set par(justify: false, first-line-indent: 0pt, leading: 0.45em); text(size: 0.72em)[${texto}] })`);
  }
  if (pre.dedicatoria?.trim()) {
    recto();
    L.push(`#v(24%)\n#align(right, block(width: 62%, { set par(justify: false, first-line-indent: 0pt); align(right, emph[${escaparTypst(limpiarTexto(pre.dedicatoria.trim(), op)).replace(/\n/g, " \\ ")}]) }))`);
  }
  const indice = () => L.push(`#align(center, text(size: 1.35em, "Índice"))\n#v(2.2em)\n#{ set par(first-line-indent: 0pt, justify: false); outline(title: none, target: heading.where(level: 1), indent: 1.5em) }`);
  if (pre.indice === "inicio") { recto(); indice(); }

  // Cuerpo
  const especialesNum = op.numeracion === "arabigo" || op.numeracion === "romano";
  let n = 0;
  for (const b of bloques) {
    switch (b.t) {
      case "cap": {
        let etiqueta = b.etiqueta;
        if (especialesNum && !b.especial) {
          n++;
          etiqueta = `Capítulo ${op.numeracion === "romano" ? romano(n) : n}`;
        } else if (op.numeracion === "ninguna") etiqueta = null;
        L.push(`#capitulo(${etiqueta ? cadena(etiqueta) : "none"}, ${cadena(b.titulo)})`);
        break;
      }
      case "parte": L.push(`#parte(${b.etiqueta && op.numeracion !== "ninguna" ? cadena(b.etiqueta) : "none"}, ${cadena(b.titulo)})`); break;
      case "sec": L.push(`#heading(level: 2)[${b.texto}]`); break;
      case "p": L.push(b.texto); break;
      case "cita": L.push(`#cita[${b.texto}]`); break;
      case "sep": L.push(`#separador()`); break;
      case "lista": L.push(`#${b.orden ? "enum" : "list"}(tight: true, ${b.items.map((x) => `[${x}]`).join(", ")})`); break;
      case "img": {
        const caja = op.ancho - interior - mg.exterior;
        const maxAlto = (op.alto - mg.superior - mg.inferior) * 0.72;
        const ancho = Math.min(caja, (maxAlto * (b.ancho || 4)) / (b.alto || 3));
        L.push(`#figure(image(${cadena(b.ruta)}, width: ${r2(ancho)}mm))`);
        break;
      }
      case "tabla": {
        const cols = Math.max(...b.filas.map((f) => f.length));
        const celdas = b.filas.flatMap((f) => [...f, ...Array(cols - f.length).fill("")]).map((c) => `[${c}]`);
        L.push(`#{ set par(first-line-indent: 0pt, justify: false); text(size: 0.88em, table(columns: ${cols}, stroke: 0.4pt, inset: 5pt, ${celdas.join(", ")})) }`);
        break;
      }
    }
    L.push("");
  }
  L.push(`#fin\n#metadata("final") <final>`);
  if (pre.indice === "final") { L.push(`#pagebreak(to: "odd")`); indice(); }
  // Blancas para cuadrar y colofón en la última página.
  for (let i = 0; i < blancas; i++) L.push(`#pagebreak()`);
  if (pre.colofon) {
    L.push(`#pagebreak()`);
    const hoy = new Date();
    const partes = [`Este libro se terminó de imprimir en ${MESES[hoy.getMonth()]} de ${d.anio || hoy.getFullYear()}`];
    if (d.impresor) partes.push(` en ${d.impresor}`);
    if (d.ciudad) partes.push(`, ${d.ciudad}`);
    partes.push(`. En su composición se usó la fuente ${op.fuente} en cuerpo ${String(cuerpo).replace(".", ",")} puntos.`);
    L.push(`#v(1fr)\n#align(center, block(width: 70%, { set par(justify: false, first-line-indent: 0pt); text(size: 0.75em, ${cadena(partes.join(""))}) }))\n#v(12%)`);
  } else if (blancas) {
    L.push(`#box()`); // sin contenido Typst no emite la última página vacía
  }
  return { fuente: L.join("\n"), cuerpo, paso };
}

// ───────────── Compilador ─────────────
let typstMod = null;
let compiladorActual = null; // { clave, compilador }

async function modulo() {
  typstMod ??= import(TYPST).catch((e) => { typstMod = null; throw e; });
  return typstMod;
}

/** Compilador con las fuentes del usuario (se recrea si cambian). */
async function compilador(fuentesPropias) {
  const clave = fuentesPropias.map((f) => `${f.nombre}:${f.bytes.length}`).join("|");
  if (compiladorActual?.clave === clave) return compiladorActual.compilador;
  const m = await modulo();
  const c = m.createTypstCompiler();
  await c.init({
    getModule: () => COMPILADOR,
    beforeBuild: [
      m.TypstSnippet.preloadFontAssets({ assets: ["text"] }).provides[0],
      ...(fuentesPropias.length ? [m.loadFonts(fuentesPropias.map((f) => f.bytes))] : []),
    ],
  });
  compiladorActual = { clave, compilador: c };
  return c;
}

/** Carga el compilador (descarga el motor la primera vez). */
export const prepararCompilador = (fuentesPropias = []) => compilador(fuentesPropias);

/** Compila una fuente Typst a PDF. Lanza un error legible si falla. */
export async function compilar(fuente, imagenes = new Map(), fuentesPropias = []) {
  const c = await compilador(fuentesPropias);
  c.resetShadow?.();
  for (const [ruta, bytes] of imagenes) c.mapShadow(ruta, bytes);
  c.addSource("/libro.typ", fuente);
  const r = await c.compile({ mainFilePath: "/libro.typ", format: 1, diagnostics: "full" });
  const errores = (r.diagnostics || []).filter((x) => x.severity === "error" || x.severity === 1 || /error/i.test(String(x.severity)));
  if (!r.result || errores.length) {
    const e = errores[0] || (r.diagnostics || [])[0];
    const donde = e?.range ? ` (línea ${String(e.range).split(":")[0]})` : "";
    throw new Error(e ? `${e.message}${donde}` : "la composición no produjo un PDF");
  }
  return r.result;
}

/** Nombre de familia de un archivo de fuente (tabla «name» de OpenType). */
export function familiaDeFuente(bytes) {
  try {
    const v = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    const tablas = v.getUint16(4);
    for (let i = 0; i < tablas; i++) {
      const r = 12 + i * 16;
      if (String.fromCharCode(...bytes.slice(r, r + 4)) !== "name") continue;
      const ini = v.getUint32(r + 8);
      const cuenta = v.getUint16(ini + 2), cadenas = ini + v.getUint16(ini + 4);
      const encontrados = {};
      for (let k = 0; k < cuenta; k++) {
        const e = ini + 6 + k * 12;
        const plataforma = v.getUint16(e), id = v.getUint16(e + 6), largo = v.getUint16(e + 8), desp = v.getUint16(e + 10);
        if (id !== 1 && id !== 16) continue;
        const crudo = bytes.slice(cadenas + desp, cadenas + desp + largo);
        let texto = "";
        if (plataforma === 0 || plataforma === 3) for (let j = 0; j + 1 < crudo.length; j += 2) texto += String.fromCharCode((crudo[j] << 8) | crudo[j + 1]);
        else texto = String.fromCharCode(...crudo);
        if (texto && !encontrados[id]) encontrados[id] = texto;
      }
      return encontrados[16] || encontrados[1] || null;
    }
  } catch { /* fuente ilegible */ }
  return null;
}
