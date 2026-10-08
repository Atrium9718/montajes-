//! Puente entre el motor y el navegador (WebAssembly).
//!
//! Todo entra y sale como JSON; los PDF viajan como bytes. Los `planear_*`
//! solo calculan (para la vista previa) y los `generar_*` escriben el PDF.

use montajes_core::catalogo::{Maquina, PerfilSalida, VersionPdfx, papeles_de_referencia};
use montajes_core::correcciones::Correcciones;
use montajes_core::cotizacion;
use montajes_core::geometria::Tamano;
use montajes_core::imposicion::firmas::{self, Aprovechamiento, Encuadernacion, ParametrosLibro, PlanLibro};
use montajes_core::imposicion::marcas::OpcionesMarcas;
use montajes_core::imposicion::nup::{self, Diseno, Distribucion, Orientacion, ParametrosNup, PlanCombinado};
use montajes_core::imposicion::{Cara, Margenes, Volteo};
use montajes_core::pdf::{self, Fuente, OpcionesSalida};
use montajes_core::portada::{self, ParametrosPortada, Portada, TipoPanel, TipoPortada};
use montajes_core::preflight::{self, OpcionesPreflight};
use montajes_core::{Error, libro};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

mod licencia;

type R<T> = Result<T, JsError>;

fn error(e: impl std::fmt::Display) -> JsError {
    JsError::new(&e.to_string())
}

/// PDF generado y su informe en JSON.
#[wasm_bindgen]
pub struct Resultado {
    pdf: Vec<u8>,
    informe: String,
}

#[wasm_bindgen]
impl Resultado {
    #[wasm_bindgen(getter)]
    pub fn pdf(&self) -> Vec<u8> {
        self.pdf.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn informe(&self) -> String {
        self.informe.clone()
    }
}

/// Activa la licencia firmada por el servidor; devuelve sus datos.
#[wasm_bindgen]
pub fn activar_licencia(token: &str) -> R<String> {
    let l = licencia::activar(token).map_err(|e| JsError::new(&e))?;
    serde_json::to_string(&l).map_err(error)
}

#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").into()
}

#[wasm_bindgen]
pub fn papeles_referencia() -> String {
    serde_json::to_string(&papeles_de_referencia()).unwrap_or_default()
}

#[derive(Serialize)]
struct InfoPagina {
    /// CropBox y TrimBox sin girar, en mm y en coordenadas de la página
    /// (para colocar la miniatura igual que el motor coloca la página).
    vista: [f64; 4],
    corte: [f64; 4],
    ancho: f64,
    alto: f64,
    rebase: f64,
    giro: u16,
    trimbox: bool,
}

#[derive(Serialize)]
struct InfoPdf {
    paginas: Vec<InfoPagina>,
    /// Formato común si todas las páginas miden lo mismo.
    formato: Option<Tamano>,
}

/// Formato, rebase y giro de cada página.
#[wasm_bindgen]
pub fn analizar(pdf: &[u8]) -> R<String> {
    licencia::exigir()?;
    let f = Fuente::desde_bytes(pdf).map_err(error)?;
    let paginas = f
        .paginas
        .iter()
        .map(|p| {
            let t = p.tamano_corte();
            let mm = |c: &pdf::Caja| [c.x0, c.y0, c.x1, c.y1].map(montajes_core::unidades::pt_a_mm);
            InfoPagina {
                vista: mm(&p.vista),
                corte: mm(&p.corte),
                ancho: t.ancho,
                alto: t.alto,
                rebase: p.rebase_disponible(),
                giro: p.giro,
                trimbox: p.tiene_trimbox,
            }
        })
        .collect();
    let formato = f.formato_comun(None, 0.0).ok().map(|(t, _)| t);
    serde_json::to_string(&InfoPdf { paginas, formato }).map_err(error)
}

fn pliego(m: &Maquina, pedido: Option<Tamano>) -> R<Tamano> {
    let p = pedido.unwrap_or(m.pliego_max);
    if p.ancho > m.pliego_max.ancho + 0.01 || p.alto > m.pliego_max.alto + 0.01 {
        return Err(error(format!("el pliego {p} excede el máximo de «{}» ({})", m.nombre, m.pliego_max)));
    }
    Ok(p)
}

fn marcas(con_marcas: bool, tira_color: bool) -> OpcionesMarcas {
    let mut m = if con_marcas { OpcionesMarcas::default() } else { OpcionesMarcas::ninguna() };
    m.tira_color = m.tira_color && tira_color;
    m
}

fn por_defecto_verdadero() -> bool {
    true
}

/// Opciones de salida a partir del perfil de la máquina.
fn salida(perfil: &PerfilSalida, icc: &[u8], titulo: &str, fecha: u64, correcciones: &Correcciones) -> OpcionesSalida {
    OpcionesSalida {
        titulo: titulo.into(),
        pdfx: perfil.pdfx,
        icc: (!icc.is_empty()).then(|| icc.to_vec()),
        condicion: perfil.condicion.clone(),
        fecha: Some(fecha),
        correcciones: correcciones.clone(),
    }
}

/// Escribe el PDF y devuelve los bytes, los avisos y si quedó identificado como PDF/X.
fn escribir(fuente: Fuente, caras: &[Cara], opciones: &OpcionesSalida) -> R<(Vec<u8>, Vec<String>, bool)> {
    let (mut doc, informe) = pdf::componer(fuente, caras, opciones).map_err(error)?;
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).map_err(error)?;
    Ok((bytes, informe.avisos, informe.pdfx_identificado))
}

// ───────────────────────── Cotización ─────────────────────────

/// Cotiza un trabajo (papel con mácula, planchas, impresión, acabados).
#[wasm_bindgen]
pub fn cotizar(peticion: &str) -> R<String> {
    licencia::exigir()?;
    let p: cotizacion::ParametrosCotizacion = serde_json::from_str(peticion).map_err(error)?;
    serde_json::to_string(&cotizacion::cotizar(&p).map_err(error)?).map_err(error)
}

/// Código de barras EAN-13 de un ISBN (barras en mm) para dibujarlo en la portada.
#[wasm_bindgen]
pub fn codigo_isbn(isbn: &str, escala: f64) -> R<String> {
    licencia::exigir()?;
    serde_json::to_string(&montajes_core::codigo_barras::isbn(isbn, escala, 0.0).map_err(error)?).map_err(error)
}

/// Marca TrimBox y BleedBox en el interior que sale de la diagramación
/// (páginas compuestas con el rebase dentro de la MediaBox).
#[wasm_bindgen]
pub fn fijar_cajas(pdf: &[u8], rebase_mm: f64) -> R<Vec<u8>> {
    licencia::exigir()?;
    pdf::fijar_cajas(pdf, rebase_mm).map_err(error)
}

/// Pliegos de máquina que salen de un pliego de compra.
#[wasm_bindgen]
pub fn salen_de(compra_ancho: f64, compra_alto: f64, pliego_ancho: f64, pliego_alto: f64) -> u32 {
    cotizacion::salen_de(Tamano::new(compra_ancho, compra_alto), Tamano::new(pliego_ancho, pliego_alto))
}

// ───────────────────────── Preflight ─────────────────────────

#[derive(Deserialize)]
struct PeticionPreflight {
    #[serde(default)]
    pdfx: Option<VersionPdfx>,
    #[serde(default = "tres")]
    rebase: f64,
    #[serde(default)]
    cobertura: Option<f64>,
}

/// Revisión del PDF antes de imprimir.
#[wasm_bindgen]
pub fn revisar_pdf(pdf: &[u8], peticion: &str) -> R<String> {
    licencia::exigir()?;
    let p: PeticionPreflight = serde_json::from_str(peticion).map_err(error)?;
    let f = Fuente::desde_bytes(pdf).map_err(error)?;
    let base = OpcionesPreflight::default();
    let op = OpcionesPreflight {
        pdfx: p.pdfx.unwrap_or(base.pdfx),
        rebase: p.rebase,
        cobertura_maxima: p.cobertura.unwrap_or(base.cobertura_maxima),
        ..base
    };
    let mut inf = preflight::revisar(&f, &op);
    // Rango legible para la interfaz.
    #[derive(Serialize)]
    struct Salida<'a> {
        #[serde(flatten)]
        informe: &'a preflight::InformePreflight,
        rangos: Vec<String>,
    }
    inf.hallazgos.truncate(60);
    let rangos = inf.hallazgos.iter().map(|h| preflight::rango_paginas(&h.paginas)).collect();
    serde_json::to_string(&Salida { informe: &inf, rangos }).map_err(error)
}

// ───────────────────────── Piezas sueltas ─────────────────────────

#[derive(Deserialize, Clone)]
struct PeticionNup {
    maquina: Maquina,
    formato: Tamano,
    paginas: usize,
    #[serde(default)]
    pliego: Option<Tamano>,
    #[serde(default = "tres")]
    rebase: f64,
    #[serde(default)]
    calle: f64,
    #[serde(default = "auto")]
    orientacion: Orientacion,
    #[serde(default)]
    dorso: bool,
    #[serde(default = "lateral")]
    volteo: Volteo,
    #[serde(default = "por_defecto_verdadero")]
    marcas: bool,
    #[serde(default = "por_defecto_verdadero")]
    tira_color: bool,
    #[serde(default)]
    titulo: String,
    #[serde(default)]
    fecha: u64,
    #[serde(default)]
    correcciones: Correcciones,
}

fn tres() -> f64 {
    3.0
}

fn auto() -> Orientacion {
    Orientacion::Auto
}

fn lateral() -> Volteo {
    Volteo::Lateral
}

#[derive(Serialize)]
struct InformeNup<'a> {
    distribucion: &'a Distribucion,
    pliego: Tamano,
    caras: &'a [Cara],
    avisos: Vec<String>,
    pdfx: bool,
}

fn plan_nup(p: &PeticionNup) -> R<(ParametrosNup, Distribucion, Vec<Cara>)> {
    if p.dorso && !p.paginas.is_multiple_of(2) {
        return Err(error("con frente y dorso el PDF debe tener un número par de páginas"));
    }
    let mut margenes = Margenes::de_maquina(&p.maquina);
    if p.dorso && !p.maquina.duplex {
        margenes = margenes.para_volteo(p.volteo);
    }
    let parametros = ParametrosNup {
        pliego: pliego(&p.maquina, p.pliego)?,
        margenes,
        pieza: p.formato,
        rebase: p.rebase,
        calle: p.calle,
        orientacion: p.orientacion,
        marcas: marcas(p.marcas, p.tira_color),
    };
    let d = nup::calcular(&parametros).map_err(error)?;
    let caras = nup::caras_trabajo(&parametros, &d, p.paginas, p.dorso.then_some(p.volteo));
    Ok((parametros, d, caras))
}

#[wasm_bindgen]
pub fn planear_nup(peticion: &str) -> R<String> {
    licencia::exigir()?;
    let p: PeticionNup = serde_json::from_str(peticion).map_err(error)?;
    let (par, d, caras) = plan_nup(&p)?;
    let avisos = caras.iter().flat_map(|c| c.marcas.avisos.clone()).collect();
    serde_json::to_string(&InformeNup { distribucion: &d, pliego: par.pliego, caras: &caras, avisos, pdfx: false })
        .map_err(error)
}

#[wasm_bindgen]
pub fn generar_nup(pdf: &[u8], peticion: &str, icc: &[u8]) -> R<Resultado> {
    licencia::exigir()?;
    let mut p: PeticionNup = serde_json::from_str(peticion).map_err(error)?;
    let fuente = Fuente::desde_bytes(pdf).map_err(error)?;
    let (formato, mut avisos) = fuente.formato_comun(Some(p.formato), p.rebase).map_err(error)?;
    p.formato = formato;
    p.paginas = fuente.paginas.len();
    let (par, d, caras) = plan_nup(&p)?;
    let s = &p.maquina.salida;
    let (bytes, mas, pdfx) = escribir(fuente, &caras, &salida(s, icc, &p.titulo, p.fecha, &p.correcciones))?;
    avisos.extend(mas);
    let informe =
        serde_json::to_string(&InformeNup { distribucion: &d, pliego: par.pliego, caras: &caras, avisos, pdfx })
            .map_err(error)?;
    Ok(Resultado { pdf: bytes, informe })
}

// ───────────────────────── Combinado (varios diseños) ─────────────────────────

/// Junta varios PDF en uno (en orden) y devuelve sus bytes.
#[wasm_bindgen]
pub fn unir_pdfs(archivos: js_sys::Array) -> R<Vec<u8>> {
    licencia::exigir()?;
    let contenidos: Vec<Vec<u8>> = archivos.iter().map(|a| js_sys::Uint8Array::new(&a).to_vec()).collect();
    let docs = contenidos.iter().map(|b| lopdf::Document::load_mem(b)).collect::<Result<Vec<_>, _>>().map_err(error)?;
    let mut doc = pdf::unir_documentos(docs).map_err(error)?;
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).map_err(error)?;
    Ok(bytes)
}

#[derive(Deserialize)]
struct PeticionCombinado {
    #[serde(flatten)]
    nup: PeticionNup,
    disenos: Vec<Diseno>,
}

#[derive(Serialize)]
struct InformeCombinado<'a> {
    plan: &'a PlanCombinado,
    distribucion: &'a Distribucion,
    pliego: Tamano,
    avisos: Vec<String>,
    pdfx: bool,
}

fn plan_combinado(p: &PeticionCombinado) -> R<(ParametrosNup, Distribucion, PlanCombinado)> {
    let mut nup_p = PeticionNup { dorso: false, ..p.nup.clone() };
    nup_p.paginas = 1;
    let (mut parametros, d, _) = plan_nup(&nup_p)?;
    let con_dorso = p.disenos.iter().any(|x| x.dorso.is_some());
    if con_dorso && !p.nup.maquina.duplex {
        parametros.margenes = Margenes::de_maquina(&p.nup.maquina).para_volteo(p.nup.volteo);
    }
    let d = if con_dorso { nup::calcular(&parametros).map_err(error)? } else { d };
    let plan = nup::combinar(&parametros, &d, &p.disenos, p.nup.volteo).map_err(error)?;
    Ok((parametros, d, plan))
}

#[wasm_bindgen]
pub fn planear_combinado(peticion: &str) -> R<String> {
    licencia::exigir()?;
    let p: PeticionCombinado = serde_json::from_str(peticion).map_err(error)?;
    let (par, d, plan) = plan_combinado(&p)?;
    let avisos = plan.caras.iter().flat_map(|c| c.marcas.avisos.clone()).collect();
    serde_json::to_string(&InformeCombinado { plan: &plan, distribucion: &d, pliego: par.pliego, avisos, pdfx: false })
        .map_err(error)
}

#[wasm_bindgen]
pub fn generar_combinado(pdf: &[u8], peticion: &str, icc: &[u8]) -> R<Resultado> {
    licencia::exigir()?;
    let mut p: PeticionCombinado = serde_json::from_str(peticion).map_err(error)?;
    let fuente = Fuente::desde_bytes(pdf).map_err(error)?;
    let (formato, mut avisos) = fuente.formato_comun(Some(p.nup.formato), p.nup.rebase).map_err(error)?;
    p.nup.formato = formato;
    let (par, d, plan) = plan_combinado(&p)?;
    let s = &p.nup.maquina.salida;
    let (bytes, mas, pdfx) =
        escribir(fuente, &plan.caras, &salida(s, icc, &p.nup.titulo, p.nup.fecha, &p.nup.correcciones))?;
    avisos.extend(mas);
    let informe =
        serde_json::to_string(&InformeCombinado { plan: &plan, distribucion: &d, pliego: par.pliego, avisos, pdfx })
            .map_err(error)?;
    Ok(Resultado { pdf: bytes, informe })
}

// ───────────────────────── Guardas ─────────────────────────

#[derive(Deserialize)]
struct Guarda {
    nombre: String,
    /// Una página (la guarda extendida) o dos (izquierda y derecha).
    paginas: Vec<usize>,
}

#[derive(Deserialize)]
struct PeticionGuardas {
    /// Formato final de la tripa (cada mitad de la guarda).
    formato: Tamano,
    guardas: Vec<Guarda>,
    #[serde(default = "tres")]
    rebase: f64,
    #[serde(default = "por_defecto_verdadero")]
    marcas: bool,
    #[serde(default)]
    maquina: Option<Maquina>,
    #[serde(default)]
    montar: bool,
    #[serde(default)]
    titulo: String,
    #[serde(default)]
    fecha: u64,
    #[serde(default)]
    correcciones: Correcciones,
}

#[derive(Serialize)]
struct InformeGuardas {
    guardas: usize,
    tamano: Tamano,
    por_pliego: Option<u32>,
    pliego: Option<Tamano>,
    avisos: Vec<String>,
    pdfx: bool,
}

/// Guardas (hojas de cortesía pegadas a la tapa) armadas en su pliego
/// extendido —dos veces el formato con el pliegue al centro— y, si se pide,
/// montadas aparte en el pliego de la máquina.
#[wasm_bindgen]
pub fn generar_guardas(pdf: &[u8], peticion: &str, icc: &[u8]) -> R<Resultado> {
    licencia::exigir()?;
    let p: PeticionGuardas = serde_json::from_str(peticion).map_err(error)?;
    if p.guardas.is_empty() {
        return Err(error("marque al menos una página como guarda"));
    }
    // La guarda extendida es una «portada» sin lomo: izquierda + derecha.
    let extendida = portada::calcular(&ParametrosPortada {
        pagina: p.formato,
        lomo_bloque: 0.0,
        tipo: TipoPortada::Rustica { solapa: 0.0 },
        rebase: p.rebase,
        derecha_a_izquierda: false,
    })
    .map_err(error)?;
    let fuente = Fuente::desde_bytes(pdf).map_err(error)?;
    let op = marcas(p.marcas, false);
    let mut caras = Vec::new();
    for g in &p.guardas {
        let tam = |i: usize| -> R<Tamano> {
            fuente
                .paginas
                .get(i)
                .map(|x| x.tamano_corte())
                .ok_or_else(|| error(format!("el PDF no tiene página {}", i + 1)))
        };
        let mut cara = match g.paginas.as_slice() {
            [i] => {
                let t = tam(*i)?;
                if (t.ancho - extendida.tamano.ancho).abs() > 0.5 || (t.alto - extendida.tamano.alto).abs() > 0.5 {
                    return Err(error(format!(
                        "la página {} ({t}) no mide lo que la guarda extendida: {} × {} mm; marque dos páginas sueltas o revise el formato",
                        i + 1,
                        pdf::mm_es(extendida.tamano.ancho),
                        pdf::mm_es(extendida.tamano.alto)
                    )));
                }
                portada::cara_completa(&extendida, *i, &op)
            }
            [izq, der] => portada::cara_armada(
                &extendida,
                &[(TipoPanel::Contratapa, *izq, tam(*izq)?), (TipoPanel::Tapa, *der, tam(*der)?)],
                &op,
            )
            .map_err(error)?,
            _ => return Err(error(format!("la {} debe tener una página extendida o dos sueltas", g.nombre))),
        };
        cara.nombre = g.nombre.clone();
        caras.push(cara);
    }
    let perfil = p.maquina.as_ref().map(|m| m.salida.clone()).unwrap_or_default();
    let opciones = salida(&perfil, icc, &p.titulo, p.fecha, &p.correcciones);
    let mut avisos = Vec::new();
    let maquina = match (&p.maquina, p.montar) {
        (Some(m), true) => m,
        _ => {
            let (bytes, mas, pdfx) = escribir(fuente, &caras, &opciones)?;
            avisos.extend(mas);
            let informe = InformeGuardas {
                guardas: caras.len(),
                tamano: extendida.tamano,
                por_pliego: None,
                pliego: None,
                avisos,
                pdfx,
            };
            return Ok(Resultado { pdf: bytes, informe: serde_json::to_string(&informe).map_err(error)? });
        }
    };

    // Montaje aparte: las guardas ya armadas se reparten en el pliego de la
    // máquina; si son dos distintas, van combinadas en partes iguales.
    let sin_correcciones = OpcionesSalida { correcciones: Correcciones::ninguna(), icc: None, ..opciones.clone() };
    let (mut doc, _) = pdf::componer(fuente, &caras, &sin_correcciones).map_err(error)?;
    let mut intermedio = Vec::new();
    doc.save_to(&mut intermedio).map_err(error)?;
    let fuente = Fuente::desde_bytes(&intermedio).map_err(error)?;
    let parametros = ParametrosNup {
        pliego: maquina.pliego_max,
        margenes: Margenes::de_maquina(maquina),
        pieza: extendida.tamano,
        rebase: p.rebase,
        calle: 0.0,
        orientacion: Orientacion::Auto,
        marcas: marcas(p.marcas, true),
    };
    let d = nup::calcular(&parametros).map_err(|e| error(format!("la guarda no cabe en «{}»: {e}", maquina.nombre)))?;
    let mut caras_pliego = if caras.len() == 1 {
        nup::caras_trabajo(&parametros, &d, 1, None)
    } else {
        let disenos: Vec<Diseno> = (0..caras.len()).map(|i| Diseno { frente: i, dorso: None, cantidad: 1 }).collect();
        match nup::combinar(&parametros, &d, &disenos, Volteo::Lateral) {
            Ok(plan) => plan.caras,
            Err(_) => {
                avisos.push("las guardas no caben juntas en un pliego: van en pliegos separados".into());
                (0..caras.len())
                    .flat_map(|i| {
                        let mut c = nup::caras_trabajo(&parametros, &d, 1, None);
                        for cara in &mut c {
                            for u in &mut cara.ubicaciones {
                                u.pagina = i;
                            }
                        }
                        c
                    })
                    .collect()
            }
        }
    };
    for (k, c) in caras_pliego.iter_mut().enumerate() {
        c.nombre = if caras.len() == 1 {
            format!("{} montada", caras[0].nombre)
        } else {
            format!("Guardas · pliego {}", k + 1)
        };
    }
    let (bytes, mas, pdfx) = escribir(fuente, &caras_pliego, &opciones)?;
    avisos.extend(mas);
    let informe = InformeGuardas {
        guardas: caras.len(),
        tamano: extendida.tamano,
        por_pliego: Some(d.piezas()),
        pliego: Some(parametros.pliego),
        avisos,
        pdfx,
    };
    Ok(Resultado { pdf: bytes, informe: serde_json::to_string(&informe).map_err(error)? })
}

// ───────────────────────── Libros y revistas ─────────────────────────

#[derive(Deserialize)]
struct PeticionLibro {
    maquina: Maquina,
    formato: Tamano,
    paginas: u32,
    encuadernacion: Encuadernacion,
    #[serde(default)]
    firma: Option<u32>,
    #[serde(default)]
    pliego: Option<Tamano>,
    #[serde(default = "tres")]
    rebase: f64,
    #[serde(default = "tres")]
    fresado: f64,
    #[serde(default = "tres")]
    refile: f64,
    /// Calibre del papel de la tripa en micras.
    #[serde(default)]
    calibre_um: Option<f64>,
    #[serde(default)]
    derecha_a_izquierda: bool,
    #[serde(default = "por_defecto_verdadero")]
    marcas: bool,
    #[serde(default = "por_defecto_verdadero")]
    tira_color: bool,
    #[serde(default)]
    titulo: String,
    #[serde(default)]
    fecha: u64,
    #[serde(default)]
    correcciones: Correcciones,
    #[serde(default)]
    aprovechamiento: Aprovechamiento,
    #[serde(default)]
    cuadernillos: Vec<u32>,
    /// Páginas del PDF que forman la tripa, en orden (vacío = todas).
    #[serde(default)]
    mapa: Vec<usize>,
}

#[derive(Serialize)]
struct InformeLibro<'a> {
    plan: &'a PlanLibro,
    pliego: Tamano,
    /// Lomo del bloque (sin portada) si se conoce el calibre.
    lomo: Option<f64>,
    avisos: Vec<String>,
    pdfx: bool,
}

fn plan_libro(p: &PeticionLibro) -> R<(ParametrosLibro, PlanLibro, Option<f64>)> {
    let parametros = ParametrosLibro {
        pliego: pliego(&p.maquina, p.pliego)?,
        margenes: Margenes::de_maquina(&p.maquina),
        pagina: p.formato,
        paginas: if p.mapa.is_empty() { p.paginas } else { p.mapa.len() as u32 },
        encuadernacion: p.encuadernacion,
        firma: p.firma,
        rebase: p.rebase,
        fresado: p.fresado,
        refile: p.refile,
        calibre_mm: p.calibre_um.map(|c| c / 1000.0),
        derecha_a_izquierda: p.derecha_a_izquierda,
        marcas: marcas(p.marcas, p.tira_color),
        aprovechamiento: p.aprovechamiento,
        cuadernillos: p.cuadernillos.clone(),
        mapa: p.mapa.clone(),
    };
    let plan = firmas::planificar(&parametros).map_err(error)?;
    let lomo = p.calibre_um.map(|c| f64::from(plan.paginas_libro / 2) * c / 1000.0);
    Ok((parametros, plan, lomo))
}

#[wasm_bindgen]
pub fn planear_libro(peticion: &str) -> R<String> {
    licencia::exigir()?;
    let p: PeticionLibro = serde_json::from_str(peticion).map_err(error)?;
    let (par, plan, lomo) = plan_libro(&p)?;
    let mut avisos = plan.avisos.clone();
    avisos.extend(plan.caras.iter().flat_map(|c| c.marcas.avisos.clone()));
    serde_json::to_string(&InformeLibro { plan: &plan, pliego: par.pliego, lomo, avisos, pdfx: false }).map_err(error)
}

#[wasm_bindgen]
pub fn generar_libro(pdf: &[u8], peticion: &str, icc: &[u8]) -> R<Resultado> {
    licencia::exigir()?;
    let mut p: PeticionLibro = serde_json::from_str(peticion).map_err(error)?;
    let fuente = Fuente::desde_bytes(pdf).map_err(error)?;
    let tripa: Vec<usize> = if p.mapa.is_empty() { (0..fuente.paginas.len()).collect() } else { p.mapa.clone() };
    let (formato, mut avisos) = fuente.formato_de(&tripa, Some(p.formato), p.rebase).map_err(error)?;
    p.formato = formato;
    p.paginas = tripa.len() as u32;
    let (par, plan, lomo) = plan_libro(&p)?;
    avisos.extend(plan.avisos.iter().cloned());
    let s = &p.maquina.salida;
    let (bytes, mas, pdfx) = escribir(fuente, &plan.caras, &salida(s, icc, &p.titulo, p.fecha, &p.correcciones))?;
    avisos.extend(mas);
    let informe =
        serde_json::to_string(&InformeLibro { plan: &plan, pliego: par.pliego, lomo, avisos, pdfx }).map_err(error)?;
    Ok(Resultado { pdf: bytes, informe })
}

// ───────────────────────── Portada ─────────────────────────

#[derive(Deserialize, Clone)]
struct PeticionPortada {
    formato: Tamano,
    /// Lomo indicado; si falta se calcula con páginas y calibres.
    #[serde(default)]
    lomo: Option<f64>,
    #[serde(default)]
    paginas: Option<u32>,
    #[serde(default)]
    calibre_um: Option<f64>,
    #[serde(default)]
    calibre_portada_um: Option<f64>,
    tipo: TipoPortada,
    #[serde(default = "tres")]
    rebase: f64,
    #[serde(default)]
    derecha_a_izquierda: bool,
    #[serde(default = "por_defecto_verdadero")]
    marcas: bool,
    /// Qué es cada página del PDF al armar: "tapa", "contratapa", "lomo"…
    #[serde(default)]
    orden: Vec<TipoPanel>,
    /// ISBN para el código de barras de la contratapa.
    #[serde(default)]
    isbn: Option<String>,
    #[serde(default)]
    titulo: String,
    #[serde(default)]
    fecha: u64,
    #[serde(default)]
    maquina: Option<Maquina>,
    #[serde(default)]
    correcciones: Correcciones,
}

fn calcular_portada(p: &PeticionPortada) -> R<Portada> {
    let lomo_bloque = match p.lomo {
        Some(l) => l,
        None => {
            let paginas = p.paginas.ok_or_else(|| error("indique el lomo o las páginas"))?;
            let calibre = p.calibre_um.ok_or_else(|| error("indique el lomo o el calibre del papel"))?;
            let hojas = f64::from(paginas.div_ceil(2));
            let cubierta = match p.tipo {
                TipoPortada::Rustica { .. } => 2.0 * p.calibre_portada_um.unwrap_or(0.0),
                TipoPortada::TapaDura { .. } => 0.0,
            };
            (hojas * calibre + cubierta) / 1000.0
        }
    };
    portada::calcular(&ParametrosPortada {
        pagina: p.formato,
        lomo_bloque,
        tipo: p.tipo,
        rebase: p.rebase,
        derecha_a_izquierda: p.derecha_a_izquierda,
    })
    .map_err(error)
}

#[wasm_bindgen]
pub fn calcular_portada_json(peticion: &str) -> R<String> {
    licencia::exigir()?;
    let p: PeticionPortada = serde_json::from_str(peticion).map_err(error)?;
    serde_json::to_string(&calcular_portada(&p)?).map_err(error)
}

#[wasm_bindgen]
pub fn plantilla_portada(peticion: &str) -> R<Resultado> {
    licencia::exigir()?;
    let p: PeticionPortada = serde_json::from_str(peticion).map_err(error)?;
    let c = calcular_portada(&p)?;
    let titulo = format!(
        "Portada {} × {} mm · lomo {} mm",
        pdf::mm_es(c.tamano.ancho),
        pdf::mm_es(c.tamano.alto),
        pdf::mm_es(c.lomo)
    );
    let nota =
        format!("{} · rebase {} mm", if p.titulo.is_empty() { "Macula" } else { &p.titulo }, pdf::mm_es(c.rebase));
    let codigo = match p.isbn.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => Some(montajes_core::codigo_barras::isbn(t, 1.0, 0.0).map_err(error)?),
        None => None,
    };
    let mut doc = pdf::documento_plantilla_portada(&c, &titulo, &nota, codigo.as_ref());
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).map_err(error)?;
    Ok(Resultado { pdf: bytes, informe: serde_json::to_string(&c).map_err(error)? })
}

#[wasm_bindgen]
pub fn armar_portada(pdf: &[u8], peticion: &str, icc: &[u8]) -> R<Resultado> {
    licencia::exigir()?;
    let p: PeticionPortada = serde_json::from_str(peticion).map_err(error)?;
    let c = calcular_portada(&p)?;
    let fuente = Fuente::desde_bytes(pdf).map_err(error)?;
    let op = marcas(p.marcas, false);
    let primera = fuente.paginas[0].tamano_corte();
    let completa = fuente.paginas.len() == 1
        && (primera.ancho - c.tamano.ancho).abs() <= 0.5
        && (primera.alto - c.tamano.alto).abs() <= 0.5;
    let cara = if completa {
        portada::cara_completa(&c, 0, &op)
    } else {
        if matches!(p.tipo, TipoPortada::TapaDura { .. }) {
            return Err(error(format!(
                "en tapa dura el forro va en una sola página de {} × {} mm (use la plantilla)",
                pdf::mm_es(c.tamano.ancho),
                pdf::mm_es(c.tamano.alto)
            )));
        }
        if fuente.paginas.len() > p.orden.len() {
            return Err(error(format!(
                "el PDF tiene {} páginas y el orden nombra {}",
                fuente.paginas.len(),
                p.orden.len()
            )));
        }
        let asignacion: Vec<_> =
            fuente.paginas.iter().enumerate().map(|(i, pg)| (p.orden[i], i, pg.tamano_corte())).collect();
        portada::cara_armada(&c, &asignacion, &op).map_err(error)?
    };
    let perfil = p.maquina.as_ref().map(|m| m.salida.clone()).unwrap_or_default();
    let (bytes, avisos, identificado) =
        escribir(fuente, &[cara], &salida(&perfil, icc, &p.titulo, p.fecha, &p.correcciones))?;
    #[derive(Serialize)]
    struct InformePortada<'a> {
        portada: &'a Portada,
        completa: bool,
        avisos: Vec<String>,
        pdfx: bool,
    }
    let informe =
        serde_json::to_string(&InformePortada { portada: &c, completa, avisos, pdfx: identificado }).map_err(error)?;
    Ok(Resultado { pdf: bytes, informe })
}

// ───────────────────────── Carátula del libro ─────────────────────────

/// Qué página del PDF va en cada panel de la carátula (índices desde 0).
#[derive(Deserialize, Default)]
struct PaginasCaratula {
    // Tiro: cara exterior.
    portada: Option<usize>,
    contraportada: Option<usize>,
    lomo: Option<usize>,
    solapa_portada: Option<usize>,
    solapa_contraportada: Option<usize>,
    /// Exterior completo en una sola página (contraportada + lomo + portada).
    exterior: Option<usize>,
    // Retiro: cara interior.
    segunda: Option<usize>,
    tercera: Option<usize>,
    solapa_portada_interior: Option<usize>,
    solapa_contraportada_interior: Option<usize>,
    /// Interior completo en una sola página.
    interior: Option<usize>,
}

#[derive(Deserialize)]
struct PeticionCaratula {
    #[serde(flatten)]
    portada: PeticionPortada,
    /// Página del PDF para cada panel (no se llama «paginas»: ese campo es el
    /// número de páginas de la tripa, que viene de la petición de portada).
    #[serde(default)]
    asignacion: PaginasCaratula,
    /// Montar la carátula en el pliego de la máquina (tiro y retiro).
    #[serde(default)]
    montar: bool,
}

#[derive(Serialize)]
struct InformeCaratula<'a> {
    portada: &'a Portada,
    con_retiro: bool,
    /// Carátulas por pliego cuando se montó en la máquina.
    por_pliego: Option<u32>,
    pliego: Option<Tamano>,
    avisos: Vec<String>,
    pdfx: bool,
}

/// Carátula con su tiro (exterior) y retiro (interior). El interior se arma
/// reflejado: visto desde adentro, la segunda de forros queda a la izquierda.
#[wasm_bindgen]
pub fn generar_caratula(pdf: &[u8], peticion: &str, icc: &[u8]) -> R<Resultado> {
    licencia::exigir()?;
    let p: PeticionCaratula = serde_json::from_str(peticion).map_err(error)?;
    let pp = &p.portada;
    let exterior = calcular_portada(pp)?;
    let interior_dims =
        calcular_portada(&PeticionPortada { derecha_a_izquierda: !pp.derecha_a_izquierda, ..pp.clone() })?;
    let fuente = Fuente::desde_bytes(pdf).map_err(error)?;
    let tam = |i: usize| -> R<Tamano> {
        fuente
            .paginas
            .get(i)
            .map(|x| x.tamano_corte())
            .ok_or_else(|| error(format!("el PDF no tiene página {}", i + 1)))
    };
    let op = marcas(pp.marcas, false);
    let pg = &p.asignacion;
    let armar = |c: &Portada,
                 completa: Option<usize>,
                 asignar: &[(TipoPanel, Option<usize>)],
                 nombre: &str|
     -> R<Option<Cara>> {
        let mut cara = if let Some(i) = completa {
            let t = tam(i)?;
            if (t.ancho - c.tamano.ancho).abs() > 0.5 || (t.alto - c.tamano.alto).abs() > 0.5 {
                return Err(error(format!(
                    "la página {} ({t}) no mide lo que la carátula: {} × {} mm",
                    i + 1,
                    pdf::mm_es(c.tamano.ancho),
                    pdf::mm_es(c.tamano.alto)
                )));
            }
            portada::cara_completa(c, i, &op)
        } else {
            let lista: Vec<(TipoPanel, usize, Tamano)> =
                asignar.iter().filter_map(|(t, i)| i.map(|i| tam(i).map(|s| (*t, i, s)))).collect::<R<_>>()?;
            if lista.is_empty() {
                return Ok(None);
            }
            portada::cara_armada(c, &lista, &op).map_err(error)?
        };
        cara.nombre = nombre.into();
        Ok(Some(cara))
    };
    let tiro = armar(
        &exterior,
        pg.exterior,
        &[
            (TipoPanel::Tapa, pg.portada),
            (TipoPanel::Contratapa, pg.contraportada),
            (TipoPanel::Lomo, pg.lomo),
            (TipoPanel::SolapaTapa, pg.solapa_portada),
            (TipoPanel::SolapaContratapa, pg.solapa_contraportada),
        ],
        "Carátula tiro (exterior)",
    )?
    .ok_or_else(|| error("marque al menos la portada o el exterior completo de la carátula"))?;
    let mut avisos = Vec::new();
    let tapa_dura = matches!(pp.tipo, TipoPortada::TapaDura { .. });
    let retiro = if tapa_dura {
        if pg.segunda.is_some() || pg.tercera.is_some() || pg.interior.is_some() {
            avisos.push("en tapa dura el interior del forro queda bajo las guardas: el retiro no se imprime".into());
        }
        None
    } else {
        armar(
            &interior_dims,
            pg.interior,
            &[
                (TipoPanel::Tapa, pg.segunda),
                (TipoPanel::Contratapa, pg.tercera),
                (TipoPanel::SolapaTapa, pg.solapa_portada_interior),
                (TipoPanel::SolapaContratapa, pg.solapa_contraportada_interior),
            ],
            "Carátula retiro (interior)",
        )?
    };
    let con_retiro = retiro.is_some();
    if con_retiro && exterior.lomo > 0.0 {
        avisos.push("deje sin imprimir la zona de encolado del lomo en el interior si el pegante lo requiere".into());
    }
    let caras: Vec<Cara> = std::iter::once(tiro).chain(retiro).collect();
    let perfil = pp.maquina.as_ref().map(|m| m.salida.clone()).unwrap_or_default();
    let opciones = salida(&perfil, icc, &pp.titulo, pp.fecha, &pp.correcciones);

    if !(p.montar && pp.maquina.is_some()) {
        let (bytes, mas, pdfx) = escribir(fuente, &caras, &opciones)?;
        avisos.extend(mas);
        let informe = InformeCaratula { portada: &exterior, con_retiro, por_pliego: None, pliego: None, avisos, pdfx };
        return Ok(Resultado { pdf: bytes, informe: serde_json::to_string(&informe).map_err(error)? });
    }

    // Montaje en el pliego: primero la carátula sola (con sus cajas) y luego
    // se repite en el pliego con su tiro y retiro.
    let maquina = pp.maquina.as_ref().expect("comprobado arriba");
    let sin_correcciones = OpcionesSalida { correcciones: Correcciones::ninguna(), icc: None, ..opciones.clone() };
    let (mut doc, _) = pdf::componer(fuente, &caras, &sin_correcciones).map_err(error)?;
    let mut intermedio = Vec::new();
    doc.save_to(&mut intermedio).map_err(error)?;
    let fuente = Fuente::desde_bytes(&intermedio).map_err(error)?;
    let volteo = Volteo::Lateral;
    let mut margenes = Margenes::de_maquina(maquina);
    if con_retiro && !maquina.duplex {
        margenes = margenes.para_volteo(volteo);
    }
    let parametros = ParametrosNup {
        pliego: maquina.pliego_max,
        margenes,
        pieza: exterior.tamano,
        rebase: pp.rebase,
        calle: 0.0,
        orientacion: Orientacion::Auto,
        marcas: marcas(pp.marcas, true),
    };
    let d =
        nup::calcular(&parametros).map_err(|e| error(format!("la carátula no cabe en «{}»: {e}", maquina.nombre)))?;
    let mut caras_pliego =
        nup::caras_trabajo(&parametros, &d, if con_retiro { 2 } else { 1 }, con_retiro.then_some(volteo));
    for (c, nombre) in caras_pliego.iter_mut().zip(["Carátulas tiro", "Carátulas retiro"]) {
        c.nombre = nombre.into();
    }
    let (bytes, mas, pdfx) = escribir(fuente, &caras_pliego, &opciones)?;
    avisos.extend(mas);
    let informe = InformeCaratula {
        portada: &exterior,
        con_retiro,
        por_pliego: Some(d.piezas()),
        pliego: Some(parametros.pliego),
        avisos,
        pdfx,
    };
    Ok(Resultado { pdf: bytes, informe: serde_json::to_string(&informe).map_err(error)? })
}

/// Lomo de un libro al lomo (para el asistente rápido).
#[wasm_bindgen]
pub fn calcular_lomo(paginas: u32, calibre_um: f64, calibre_portada_um: f64) -> R<f64> {
    licencia::exigir()?;
    let papel = |c: f64| montajes_core::catalogo::Papel {
        id: "p".into(),
        nombre: String::new(),
        gramaje: 1.0,
        calibre_um: c,
        estucado: false,
        fibra: None,
        pliegos: vec![],
        notas: String::new(),
    };
    let tripa = papel(calibre_um);
    let cubierta = (calibre_portada_um > 0.0).then(|| papel(calibre_portada_um));
    libro::calcular_lomo(paginas.div_ceil(2) * 2, &tripa, cubierta.as_ref(), 0.0)
        .map(|c| c.lomo_mm)
        .map_err(|e: Error| error(e))
}
