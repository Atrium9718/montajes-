// Búsqueda de fichas técnicas de máquinas en internet con Claude (búsqueda web
// del servidor) para modelos que no están en el catálogo de referencia.
//
// Corre en el navegador con la clave de API que el usuario guarda en este
// equipo; la clave no pasa por ningún servidor propio.

const SDK = "https://cdn.jsdelivr.net/npm/@anthropic-ai/sdk@0.131.0/+esm";
const MODELO = "claude-opus-5-5";
const MAX_REANUDACIONES = 4;

const FICHA = {
  name: "guardar_ficha_maquina",
  description:
    "Guarda la ficha técnica encontrada de la máquina de impresión. Llámala una sola vez, al final, con los datos de la ficha oficial del fabricante o del distribuidor. Medidas en milímetros; usa 0 cuando un dato no aparezca en ninguna fuente.",
  strict: true,
  input_schema: {
    type: "object",
    properties: {
      encontrado: { type: "boolean", description: "Se encontró una ficha técnica confiable del modelo." },
      modelo: { type: "string", description: "Fabricante y modelo exacto, p. ej. «Heidelberg Speedmaster SM 74»." },
      tipo: { type: "string", enum: ["offset", "digital", "gran_formato"] },
      pliego_max_ancho_mm: { type: "number", description: "Pliego máximo: lado por donde entra (pinza o borde de entrada)." },
      pliego_max_alto_mm: { type: "number", description: "Pliego máximo: lado en el sentido de avance." },
      pliego_min_ancho_mm: { type: "number" },
      pliego_min_alto_mm: { type: "number" },
      area_impresion_ancho_mm: { type: "number", description: "Área máxima de impresión, mismo lado que pliego_max_ancho_mm." },
      area_impresion_alto_mm: { type: "number" },
      pinza_mm: { type: "number", description: "Margen de pinza (offset) o pérdida de imagen en el borde de entrada (digital)." },
      colores: { type: "integer", description: "Cuerpos de impresión o colores de la configuración más común." },
      duplex: { type: "boolean", description: "Imprime ambas caras en una pasada (dúplex o perfecting)." },
      notas: { type: "string", description: "Aclaraciones breves: configuraciones, unidades convertidas, datos dudosos." },
      fuentes: { type: "array", items: { type: "string" }, description: "URLs de las fichas usadas." },
    },
    required: [
      "encontrado", "modelo", "tipo", "pliego_max_ancho_mm", "pliego_max_alto_mm", "pliego_min_ancho_mm",
      "pliego_min_alto_mm", "area_impresion_ancho_mm", "area_impresion_alto_mm", "pinza_mm", "colores",
      "duplex", "notas", "fuentes",
    ],
    additionalProperties: false,
  },
};

function instrucciones(consulta) {
  return `Busca en internet la ficha técnica oficial de esta máquina de impresión: «${consulta}».

Necesito los datos para imponer pliegos (montajes) en una imprenta:
- pliego máximo y mínimo,
- área máxima de impresión,
- margen de pinza (offset) o pérdida de imagen en el borde de entrada (digital),
- número de colores o cuerpos, y si imprime las dos caras en una pasada.

Orientación: el «ancho» es el lado del pliego que entra primero en la máquina (el de la pinza). En offset de pliego suele ser el lado largo (p. ej. 740 en un 530×740). En digital con papel SRA3/13×19" suele entrar por el lado corto (ancho 330, alto 488).

Prefiere las fichas del fabricante; si solo hay distribuidores o anuncios de máquinas usadas, compara al menos dos fuentes. Convierte pulgadas a milímetros (1" = 25,4 mm). Si no encuentras el modelo con certeza, usa encontrado=false.

Cuando tengas los datos, llama a la herramienta guardar_ficha_maquina una sola vez.`;
}

/**
 * Busca la ficha de `consulta`. Devuelve `{ ficha, fuentes }` o lanza un error
 * con un mensaje para mostrar.
 */
export async function buscarFichaEnInternet(consulta, clave) {
  const { default: Anthropic } = await import(SDK);
  const cliente = new Anthropic({ apiKey: clave, dangerouslyAllowBrowser: true });
  const mensajes = [{ role: "user", content: instrucciones(consulta) }];
  const fuentes = new Set();

  for (let intento = 0; intento <= MAX_REANUDACIONES; intento++) {
    let respuesta;
    try {
      respuesta = await cliente.beta.messages.create({
        model: MODELO,
        max_tokens: 16000,
        output_config: { effort: "medium" },
        betas: ["server-side-fallback-2026-07-01"],
        fallbacks: "default",
        tools: [{ type: "web_search_20260209", name: "web_search", max_uses: 6 }, FICHA],
        messages: mensajes,
      });
    } catch (e) {
      throw new Error(mensajeDeError(e));
    }

    for (const bloque of respuesta.content) {
      // Un resultado exitoso trae una lista; un error trae un objeto.
      if (bloque.type === "web_search_tool_result" && Array.isArray(bloque.content)) {
        for (const r of bloque.content) if (r.url) fuentes.add(r.url);
      }
    }
    if (respuesta.stop_reason === "refusal") {
      throw new Error("La búsqueda fue rechazada. Prueba escribiendo solo el fabricante y el modelo.");
    }
    const ficha = respuesta.content.find((b) => b.type === "tool_use" && b.name === FICHA.name)?.input;
    if (ficha) {
      if (!ficha.encontrado) throw new Error(`No se encontró una ficha confiable de «${consulta}». ${ficha.notas || ""}`.trim());
      for (const u of ficha.fuentes || []) fuentes.add(u);
      return { ficha, fuentes: [...fuentes].slice(0, 8) };
    }
    if (respuesta.stop_reason === "pause_turn") {
      // La búsqueda del servidor se pausó: se reenvía el turno y continúa sola.
      mensajes.splice(1, mensajes.length - 1, { role: "assistant", content: respuesta.content });
      continue;
    }
    const texto = respuesta.content.filter((b) => b.type === "text").map((b) => b.text).join(" ").trim();
    throw new Error(texto || "La búsqueda terminó sin datos de la máquina.");
  }
  throw new Error("La búsqueda tardó demasiado; intenta con un nombre de modelo más preciso.");
}

function mensajeDeError(e) {
  const estado = e?.status;
  if (estado === 401) return "La clave de API no es válida. Revísala en Catálogos.";
  if (estado === 403) return "La clave no tiene permiso para usar este modelo o la búsqueda web.";
  if (estado === 429) return "Demasiadas búsquedas seguidas; espera un momento y vuelve a intentar.";
  if (estado >= 500) return "El servicio no respondió; vuelve a intentar en unos segundos.";
  if (!estado) return "No hay conexión con el servicio de búsqueda.";
  return e?.message || "No se pudo completar la búsqueda.";
}

/** Convierte la ficha encontrada en datos para el formulario de máquina. */
export function fichaAMaquina(f, fuentes) {
  const positivo = (v) => (Number.isFinite(v) && v > 0 ? Math.round(v * 10) / 10 : null);
  const ancho = positivo(f.pliego_max_ancho_mm) ?? 700;
  const alto = positivo(f.pliego_max_alto_mm) ?? 500;
  const pinza = positivo(f.pinza_mm) ?? (f.tipo === "offset" ? 10 : 4);
  const anchoImp = positivo(f.area_impresion_ancho_mm);
  const altoImp = positivo(f.area_impresion_alto_mm);
  const minimoBorde = f.tipo === "offset" ? 3 : 2;
  const cola = altoImp ? Math.max(Math.round((alto - altoImp - pinza) * 10) / 10, minimoBorde) : minimoBorde + 2;
  const lateral = anchoImp ? Math.max(Math.round(((ancho - anchoImp) / 2) * 10) / 10, 2) : 5;
  const minAncho = positivo(f.pliego_min_ancho_mm);
  const minAlto = positivo(f.pliego_min_alto_mm);
  return {
    nombre: f.modelo,
    tipo: ["offset", "digital", "gran_formato"].includes(f.tipo) ? f.tipo : "offset",
    pliego_max: { ancho, alto },
    pliego_min: minAncho && minAlto ? { ancho: minAncho, alto: minAlto } : null,
    pinza,
    cola,
    lateral,
    colores: Math.max(1, Math.round(f.colores || 4)),
    duplex: !!f.duplex,
    condicion: "FOGRA39",
    notas: [
      `Ficha encontrada en internet: pliego ${ancho}×${alto}${anchoImp && altoImp ? `, área ${anchoImp}×${altoImp}` : ""} mm.`,
      f.notas,
      fuentes.length ? `Fuentes: ${fuentes.slice(0, 3).join(" · ")}` : "",
    ].filter(Boolean).join(" "),
  };
}
