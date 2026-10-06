//! Preflight: revisión del PDF antes de imprimir, con criterios de las
//! especificaciones GWG y PDF/X.
//!
//! Recorre el contenido de cada página (y de los Form XObjects anidados)
//! siguiendo la matriz de transformación, así la resolución de las imágenes
//! y el grosor de las líneas se miden como quedarán impresos.

use std::collections::BTreeMap;

use lopdf::content::Content;
use lopdf::{Dictionary, Document, Object, ObjectId};
use serde::{Deserialize, Serialize};

use crate::catalogo::VersionPdfx;
use crate::pdf::{Fuente, resolver};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Nivel {
    Error,
    Advertencia,
    Info,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hallazgo {
    pub nivel: Nivel,
    /// Identificador estable, p. ej. `fuente_no_incrustada`.
    pub codigo: String,
    pub mensaje: String,
    /// Páginas afectadas (desde 1).
    pub paginas: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpcionesPreflight {
    pub pdfx: VersionPdfx,
    /// Rebase requerido (mm).
    pub rebase: f64,
    /// Resolución mínima de imágenes en color o gris: debajo es error (ppi).
    pub ppi_error: f64,
    /// Resolución recomendada: debajo es advertencia (ppi).
    pub ppi_advertencia: f64,
    /// Resolución mínima de imágenes de 1 bit (line art).
    pub ppi_lineart: f64,
    /// Cobertura total de tinta máxima (%), según la condición de impresión.
    pub cobertura_maxima: f64,
    /// Grosor mínimo de línea (pt).
    pub linea_minima: f64,
}

impl Default for OpcionesPreflight {
    fn default() -> Self {
        // Valores de GWG 2022 para impresión comercial en papel estucado.
        Self {
            pdfx: VersionPdfx::X4,
            rebase: 3.0,
            ppi_error: 150.0,
            ppi_advertencia: 250.0,
            ppi_lineart: 550.0,
            cobertura_maxima: 320.0,
            linea_minima: 0.25,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct InformePreflight {
    pub hallazgos: Vec<Hallazgo>,
    pub tintas_directas: Vec<String>,
    pub errores: usize,
    pub advertencias: usize,
}

impl InformePreflight {
    pub fn listo(&self) -> bool {
        self.errores == 0
    }
}

type Matriz = [f64; 6];
const IDENTIDAD: Matriz = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// `a × b`: primero se aplica `a`, luego `b` (convención PDF).
fn multiplicar(a: &Matriz, b: &Matriz) -> Matriz {
    [
        a[0] * b[0] + a[1] * b[2],
        a[0] * b[1] + a[1] * b[3],
        a[2] * b[0] + a[3] * b[2],
        a[2] * b[1] + a[3] * b[3],
        a[4] * b[0] + a[5] * b[2] + b[4],
        a[4] * b[1] + a[5] * b[3] + b[5],
    ]
}

fn numero(o: &Object) -> Option<f64> {
    o.as_float().ok().map(f64::from)
}

fn matriz(ops: &[Object]) -> Option<Matriz> {
    let v: Vec<f64> = ops.iter().filter_map(numero).collect();
    (v.len() == 6).then(|| [v[0], v[1], v[2], v[3], v[4], v[5]])
}

#[derive(Debug, Clone, PartialEq)]
enum Color {
    Rgb,
    Cmyk,
    Gris,
    Lab,
    Directas(Vec<String>),
    Otro,
}

#[derive(Clone, Copy)]
struct Estado {
    ctm: Matriz,
    linea: f64,
}

/// Acumula hallazgos agrupados por código y detalle.
#[derive(Default)]
struct Acumulador {
    grupos: BTreeMap<(String, String), (Nivel, String, Vec<usize>)>,
    tintas: Vec<String>,
}

impl Acumulador {
    fn agregar(&mut self, nivel: Nivel, codigo: &str, detalle: &str, mensaje: String, pagina: usize) {
        let entrada = self
            .grupos
            .entry((codigo.to_string(), detalle.to_string()))
            .or_insert_with(|| (nivel, mensaje.clone(), Vec::new()));
        // Se conserva el caso más grave (p. ej. la resolución más baja).
        if nivel < entrada.0 {
            *entrada = (nivel, mensaje, std::mem::take(&mut entrada.2));
        }
        if !entrada.2.contains(&pagina) {
            entrada.2.push(pagina);
        }
    }
}

struct Revisor<'a> {
    doc: &'a Document,
    op: &'a OpcionesPreflight,
    acc: Acumulador,
    /// Peor valor visto por código (para dar un solo mensaje con el extremo).
    peor_ppi: f64,
    peor_lineart: f64,
    peor_cobertura: f64,
    peor_linea: f64,
}

impl<'a> Revisor<'a> {
    fn dict(&self, o: &'a Object) -> Option<&'a Dictionary> {
        match resolver(self.doc, o) {
            Object::Dictionary(d) => Some(d),
            Object::Stream(s) => Some(&s.dict),
            _ => None,
        }
    }

    fn nombre(o: &Object) -> Option<String> {
        o.as_name().ok().map(|n| String::from_utf8_lossy(n).into_owned())
    }

    fn color(&self, o: &Object, profundidad: u8) -> Color {
        let o = resolver(self.doc, o);
        if let Some(n) = Self::nombre(o) {
            return match n.as_str() {
                "DeviceRGB" | "RGB" | "CalRGB" => Color::Rgb,
                "DeviceCMYK" | "CMYK" => Color::Cmyk,
                "DeviceGray" | "G" | "CalGray" => Color::Gris,
                "Lab" => Color::Lab,
                _ => Color::Otro,
            };
        }
        let Ok(v) = o.as_array() else { return Color::Otro };
        let familia = v.first().and_then(Self::nombre).unwrap_or_default();
        match familia.as_str() {
            "CalRGB" => Color::Rgb,
            "CalGray" => Color::Gris,
            "Lab" => Color::Lab,
            "ICCBased" => {
                let n = v.get(1).and_then(|s| self.dict(s)).and_then(|d| d.get(b"N").ok()).and_then(numero);
                match n.map(|n| n as u8) {
                    Some(3) => Color::Rgb,
                    Some(4) => Color::Cmyk,
                    Some(1) => Color::Gris,
                    _ => Color::Otro,
                }
            }
            "Separation" => {
                let n = v.get(1).and_then(Self::nombre).unwrap_or_default();
                if n == "All" || n == "None" { Color::Otro } else { Color::Directas(vec![n]) }
            }
            "DeviceN" => {
                let nombres: Vec<String> = v
                    .get(1)
                    .and_then(|x| resolver(self.doc, x).as_array().ok())
                    .map(|a| a.iter().filter_map(Self::nombre).collect())
                    .unwrap_or_default();
                let directas: Vec<String> = nombres
                    .into_iter()
                    .filter(|n| !["Cyan", "Magenta", "Yellow", "Black", "None", "All"].contains(&n.as_str()))
                    .collect();
                if directas.is_empty() { Color::Cmyk } else { Color::Directas(directas) }
            }
            "Indexed" | "Pattern" if profundidad < 4 => {
                v.get(1).map_or(Color::Otro, |base| self.color(base, profundidad + 1))
            }
            _ => Color::Otro,
        }
    }

    fn registrar_color(&mut self, c: Color, origen: &str, pagina: usize) {
        match c {
            Color::Rgb => self.acc.agregar(
                Nivel::Advertencia,
                "rgb",
                origen,
                format!(
                    "Hay {origen} en RGB: conviértalo a CMYK con el perfil de impresión para controlar el resultado"
                ),
                pagina,
            ),
            Color::Lab => self.acc.agregar(
                Nivel::Advertencia,
                "lab",
                origen,
                format!("Hay {origen} en Lab: se convertirá a CMYK en el RIP"),
                pagina,
            ),
            Color::Directas(nombres) => {
                for n in nombres {
                    if !self.acc.tintas.contains(&n) {
                        self.acc.tintas.push(n.clone());
                    }
                    self.acc.agregar(
                        Nivel::Info,
                        "tinta_directa",
                        &n,
                        format!("Tinta directa «{n}»: necesita su propia plancha o se convertirá a CMYK"),
                        pagina,
                    );
                }
            }
            Color::Cmyk | Color::Gris | Color::Otro => {}
        }
    }

    fn revisar_fuente(&mut self, fuente: &Dictionary, nombre_recurso: &str, pagina: usize) {
        let subtipo = fuente.get(b"Subtype").ok().and_then(Self::nombre).unwrap_or_default();
        if subtipo == "Type3" {
            return;
        }
        let base = fuente.get(b"BaseFont").ok().and_then(Self::nombre).unwrap_or_else(|| nombre_recurso.to_string());
        let descriptor = if subtipo == "Type0" {
            fuente
                .get(b"DescendantFonts")
                .ok()
                .and_then(|d| resolver(self.doc, d).as_array().ok())
                .and_then(|a| a.first())
                .and_then(|f| self.dict(f))
                .and_then(|f| f.get(b"FontDescriptor").ok())
                .and_then(|d| self.dict(d))
        } else {
            fuente.get(b"FontDescriptor").ok().and_then(|d| self.dict(d))
        };
        let incrustada = descriptor.is_some_and(|d| d.has(b"FontFile") || d.has(b"FontFile2") || d.has(b"FontFile3"));
        if !incrustada {
            // Quita el prefijo de subconjunto (ABCDEF+Nombre).
            let limpio = base.split_once('+').map_or(base.as_str(), |(_, n)| n).to_string();
            self.acc.agregar(
                Nivel::Error,
                "fuente_no_incrustada",
                &limpio,
                format!("La fuente «{limpio}» no está incrustada: el RIP la reemplazará por otra"),
                pagina,
            );
        }
    }

    fn revisar_imagen(&mut self, img: &Dictionary, ctm: &Matriz, pagina: usize) {
        let ancho_px = img.get(b"Width").ok().and_then(numero).unwrap_or(0.0);
        let alto_px = img.get(b"Height").ok().and_then(numero).unwrap_or(0.0);
        // Tamaño impreso de la imagen (el cuadrado unidad transformado por la CTM).
        let ancho_pt = ctm[0].hypot(ctm[1]);
        let alto_pt = ctm[2].hypot(ctm[3]);
        if ancho_px > 0.0 && alto_px > 0.0 && ancho_pt > 0.5 && alto_pt > 0.5 {
            let ppi = (ancho_px / (ancho_pt / 72.0)).min(alto_px / (alto_pt / 72.0));
            let un_bit = img.get(b"ImageMask").ok().and_then(|o| o.as_bool().ok()).unwrap_or(false)
                || img.get(b"BitsPerComponent").ok().and_then(numero) == Some(1.0);
            if un_bit {
                if ppi < self.op.ppi_lineart {
                    self.peor_lineart = self.peor_lineart.min(ppi);
                    self.acc.agregar(
                        Nivel::Advertencia,
                        "resolucion_lineart",
                        "",
                        format!(
                            "Imagen de línea (1 bit) a {:.0} ppi; se recomiendan {:.0} ppi o más",
                            self.peor_lineart, self.op.ppi_lineart
                        ),
                        pagina,
                    );
                }
            } else if ppi < self.op.ppi_advertencia {
                self.peor_ppi = self.peor_ppi.min(ppi);
                let nivel = if self.peor_ppi < self.op.ppi_error { Nivel::Error } else { Nivel::Advertencia };
                self.acc.agregar(
                    nivel,
                    "resolucion_baja",
                    "",
                    format!(
                        "Imagen a {:.0} ppi impresa (mínimo {:.0}, recomendado {:.0}): saldrá pixelada",
                        self.peor_ppi, self.op.ppi_error, self.op.ppi_advertencia
                    ),
                    pagina,
                );
            }
        }
        if let Ok(cs) = img.get(b"ColorSpace") {
            let c = self.color(cs, 0);
            self.registrar_color(c, "imágenes", pagina);
        }
        if img.get(b"SMask").is_ok_and(|s| Self::nombre(s).as_deref() != Some("None")) {
            self.transparencia("imagen con máscara suave", pagina);
        }
    }

    fn transparencia(&mut self, que: &str, pagina: usize) {
        let (nivel, mensaje) = match self.op.pdfx {
            VersionPdfx::X1a => {
                (Nivel::Error, format!("PDF/X-1a no admite transparencias ({que}); use PDF/X-4 o aplánelas"))
            }
            VersionPdfx::X4 => {
                (Nivel::Info, format!("Tiene transparencias ({que}); PDF/X-4 las admite, el RIP debe ser compatible"))
            }
        };
        self.acc.agregar(nivel, "transparencia", "", mensaje, pagina);
    }

    fn revisar_grafico(&mut self, gs: &Dictionary, pagina: usize) {
        let menor_uno = |k: &[u8]| gs.get(k).ok().and_then(numero).is_some_and(|v| v < 0.999);
        if menor_uno(b"CA") || menor_uno(b"ca") {
            self.transparencia("opacidad", pagina);
        }
        if let Ok(bm) = gs.get(b"BM") {
            let modo = Self::nombre(resolver(self.doc, bm))
                .or_else(|| resolver(self.doc, bm).as_array().ok().and_then(|a| a.first()).and_then(Self::nombre));
            if modo.is_some_and(|m| m != "Normal" && m != "Compatible") {
                self.transparencia("modo de fusión", pagina);
            }
        }
        if gs.get(b"SMask").is_ok_and(|s| Self::nombre(resolver(self.doc, s)).as_deref() != Some("None")) {
            self.transparencia("máscara suave", pagina);
        }
    }

    fn recurso(&self, recursos: Option<&'a Dictionary>, tipo: &[u8], nombre: &Object) -> Option<&'a Object> {
        let nombre = nombre.as_name().ok()?;
        let tabla = self.dict(recursos?.get(tipo).ok()?)?;
        tabla.get(nombre).ok()
    }

    fn recorrer(&mut self, contenido: &[u8], recursos: Option<&'a Dictionary>, ctm: Matriz, pagina: usize, nivel: u8) {
        if nivel > 12 {
            return;
        }
        let Ok(c) = Content::decode(contenido) else {
            self.acc.agregar(
                Nivel::Advertencia,
                "contenido_ilegible",
                "",
                "Parte del contenido no se pudo analizar".into(),
                pagina,
            );
            return;
        };
        let mut estado = Estado { ctm, linea: 1.0 };
        let mut pila = Vec::new();
        for op in &c.operations {
            let a = &op.operands;
            match op.operator.as_str() {
                "q" => pila.push(estado),
                "Q" => estado = pila.pop().unwrap_or(estado),
                "cm" => {
                    if let Some(m) = matriz(a) {
                        estado.ctm = multiplicar(&m, &estado.ctm);
                    }
                }
                "w" => estado.linea = a.first().and_then(numero).unwrap_or(estado.linea),
                "S" | "s" | "B" | "B*" | "b" | "b*" => {
                    let escala = (estado.ctm[0] * estado.ctm[3] - estado.ctm[1] * estado.ctm[2]).abs().sqrt();
                    let grosor = estado.linea * escala;
                    if grosor < self.op.linea_minima {
                        self.peor_linea = self.peor_linea.min(grosor);
                        let mensaje = if self.peor_linea <= 0.0 {
                            "Hay líneas de grosor 0 (la más fina del equipo): pueden desaparecer al imprimir"
                                .to_string()
                        } else {
                            format!(
                                "Hay líneas de {:.2} pt (mínimo recomendado {:.2} pt): pueden no verse",
                                self.peor_linea, self.op.linea_minima
                            )
                        };
                        self.acc.agregar(Nivel::Advertencia, "linea_fina", "", mensaje, pagina);
                    }
                }
                "k" | "K" => {
                    let total: f64 = a.iter().filter_map(numero).sum::<f64>() * 100.0;
                    if total > self.op.cobertura_maxima + 0.5 {
                        self.peor_cobertura = self.peor_cobertura.max(total);
                        self.acc.agregar(
                            Nivel::Advertencia,
                            "cobertura",
                            "",
                            format!(
                                "Colores con {:.0} % de cobertura total de tinta (máximo {:.0} %): puede repintar o no secar",
                                self.peor_cobertura, self.op.cobertura_maxima
                            ),
                            pagina,
                        );
                    }
                }
                "rg" | "RG" => self.registrar_color(Color::Rgb, "colores vectoriales", pagina),
                "cs" | "CS" => {
                    if let Some(n) = a.first() {
                        let directo = n.as_name().ok().map(|n| n.to_vec());
                        let c = match directo.as_deref() {
                            Some(b"DeviceRGB") => Color::Rgb,
                            Some(b"DeviceCMYK") | Some(b"DeviceGray") | Some(b"Pattern") => Color::Otro,
                            _ => self.recurso(recursos, b"ColorSpace", n).map_or(Color::Otro, |cs| self.color(cs, 0)),
                        };
                        self.registrar_color(c, "colores vectoriales", pagina);
                    }
                }
                "sh" => {
                    if let Some(sh) =
                        a.first().and_then(|n| self.recurso(recursos, b"Shading", n)).and_then(|s| self.dict(s))
                        && let Ok(cs) = sh.get(b"ColorSpace")
                    {
                        let c = self.color(cs, 0);
                        self.registrar_color(c, "degradados", pagina);
                    }
                }
                "gs" => {
                    if let Some(gs) =
                        a.first().and_then(|n| self.recurso(recursos, b"ExtGState", n)).and_then(|g| self.dict(g))
                    {
                        self.revisar_grafico(gs, pagina);
                    }
                }
                "Tf" => {
                    if let Some(n) = a.first()
                        && let Some(f) = self.recurso(recursos, b"Font", n).and_then(|f| self.dict(f))
                    {
                        let nombre = Self::nombre(n).unwrap_or_default();
                        self.revisar_fuente(f, &nombre, pagina);
                    }
                }
                "Do" => {
                    let Some(n) = a.first() else { continue };
                    let Some(obj) = self.recurso(recursos, b"XObject", n) else { continue };
                    let Object::Stream(s) = resolver(self.doc, obj) else { continue };
                    match s.dict.get(b"Subtype").ok().and_then(Self::nombre).as_deref() {
                        Some("Image") => self.revisar_imagen(&s.dict, &estado.ctm, pagina),
                        Some("Form") => {
                            if s.dict.get(b"Group").ok().and_then(|g| self.dict(g)).is_some_and(|g| {
                                g.get(b"S").ok().and_then(Self::nombre).as_deref() == Some("Transparency")
                            }) {
                                self.transparencia("grupo de transparencia", pagina);
                            }
                            let m = s.dict.get(b"Matrix").ok().and_then(|o| resolver(self.doc, o).as_array().ok());
                            let m = m.and_then(|m| matriz(m)).unwrap_or(IDENTIDAD);
                            let propios = s.dict.get(b"Resources").ok().and_then(|r| self.dict(r)).or(recursos);
                            if let Ok(datos) = s.decompressed_content().or_else(|_| Ok::<_, ()>(s.content.clone())) {
                                self.recorrer(&datos, propios, multiplicar(&m, &estado.ctm), pagina, nivel + 1);
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }
}

/// Revisa todas las páginas del PDF.
pub fn revisar(fuente: &Fuente, op: &OpcionesPreflight) -> InformePreflight {
    let doc = fuente.documento();
    let mut r = Revisor {
        doc,
        op,
        acc: Acumulador::default(),
        peor_ppi: f64::INFINITY,
        peor_lineart: f64::INFINITY,
        peor_cobertura: 0.0,
        peor_linea: f64::INFINITY,
    };
    let ids: Vec<ObjectId> = doc.get_pages().into_values().collect();
    for (i, (p, id)) in fuente.paginas.iter().zip(ids).enumerate() {
        let pagina = i + 1;
        if !p.tiene_trimbox {
            r.acc.agregar(
                Nivel::Advertencia,
                "sin_trimbox",
                "",
                "Páginas sin TrimBox: se toma el tamaño de la página como formato final".into(),
                pagina,
            );
        }
        if p.rebase_disponible() + 0.05 < op.rebase {
            r.acc.agregar(
                Nivel::Advertencia,
                "rebase",
                "",
                format!("Rebase menor a {} mm: puede quedar un filo blanco al cortar", op.rebase),
                pagina,
            );
        }
        let anotaciones = doc
            .get_dictionary(id)
            .ok()
            .and_then(|d| d.get(b"Annots").ok())
            .and_then(|a| resolver(doc, a).as_array().ok())
            .is_some_and(|a| !a.is_empty());
        if anotaciones {
            r.acc.agregar(
                Nivel::Info,
                "anotaciones",
                "",
                "Hay comentarios, enlaces o campos de formulario: no se imprimen en el montaje".into(),
                pagina,
            );
        }
        if doc.get_dictionary(id).ok().and_then(|d| d.get(b"Group").ok()).is_some() {
            r.transparencia("grupo de página", pagina);
        }
        let recursos = recursos_de(doc, id);
        let contenido = doc.get_page_content(id);
        r.recorrer(&contenido, recursos, IDENTIDAD, pagina, 0);
    }
    if fuente.formato_comun(None, 0.0).is_err() {
        r.acc.agregar(Nivel::Error, "tamanos_distintos", "", "Las páginas tienen formatos finales distintos".into(), 1);
    }

    let mut hallazgos: Vec<Hallazgo> = r
        .acc
        .grupos
        .into_iter()
        .map(|((codigo, _), (nivel, mensaje, mut paginas))| {
            paginas.sort_unstable();
            Hallazgo { nivel, codigo, mensaje, paginas }
        })
        .collect();
    hallazgos.sort_by(|a, b| a.nivel.cmp(&b.nivel).then(a.codigo.cmp(&b.codigo)));
    let errores = hallazgos.iter().filter(|h| h.nivel == Nivel::Error).count();
    let advertencias = hallazgos.iter().filter(|h| h.nivel == Nivel::Advertencia).count();
    InformePreflight { hallazgos, tintas_directas: r.acc.tintas, errores, advertencias }
}

/// Recursos de la página (propios o heredados) como referencia al documento.
fn recursos_de(doc: &Document, pagina: ObjectId) -> Option<&Dictionary> {
    let mut actual = doc.get_dictionary(pagina).ok()?;
    for _ in 0..64 {
        if let Ok(r) = actual.get(b"Resources") {
            return match r {
                Object::Reference(id) => doc.get_dictionary(*id).ok(),
                Object::Dictionary(d) => Some(d),
                _ => None,
            };
        }
        let padre = actual.get(b"Parent").ok()?.as_reference().ok()?;
        actual = doc.get_dictionary(padre).ok()?;
    }
    None
}

/// Resume una lista de páginas: «1–3, 7».
pub fn rango_paginas(paginas: &[usize]) -> String {
    let mut partes: Vec<String> = Vec::new();
    let mut i = 0;
    while i < paginas.len() {
        let inicio = paginas[i];
        let mut fin = inicio;
        while i + 1 < paginas.len() && paginas[i + 1] == fin + 1 {
            i += 1;
            fin = paginas[i];
        }
        partes.push(if fin == inicio { inicio.to_string() } else { format!("{inicio}–{fin}") });
        i += 1;
    }
    partes.join(", ")
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use lopdf::{Stream, dictionary};

    /// Página A6 con: fuente sin incrustar, relleno RGB, CMYK al 400 %,
    /// línea de 0,1 pt, imagen de 100 px estirada a 2 pulgadas (50 ppi),
    /// tinta directa y opacidad.
    fn pdf_problematico() -> Vec<u8> {
        let mut doc = Document::with_version("1.6");
        let arbol = doc.new_object_id();
        let fuente = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" });
        let imagen = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Image", "Width" => 100, "Height" => 100,
                "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8,
            },
            vec![0; 30_000],
        ));
        let funcion = doc.add_object(dictionary! {
            "FunctionType" => 2, "Domain" => vec![0.into(), 1.into()],
            "C0" => vec![0.into(), 0.into(), 0.into(), 0.into()], "C1" => vec![0.into(), 1.into(), 1.into(), 0.into()], "N" => 1,
        });
        let pantone = Object::Array(vec![
            "Separation".into(),
            Object::Name(b"PANTONE 186 C".to_vec()),
            "DeviceCMYK".into(),
            funcion.into(),
        ]);
        let contenido = b"q 1 0 0 rg 0 0 50 50 re f 1 1 1 1 k 50 0 50 50 re f 0.1 w 0 0 m 100 100 l S /P0 cs 1 scn 0 60 10 10 re f /G0 gs 144 0 0 144 100 100 cm /Im0 Do Q BT /F1 12 Tf 10 10 Td (Hola) Tj ET".to_vec();
        let flujo = doc.add_object(Stream::new(dictionary! {}, contenido));
        let pagina = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => arbol,
            "MediaBox" => vec![0.into(), 0.into(), 298.into(), 420.into()],
            "Contents" => flujo,
            "Resources" => dictionary! {
                "Font" => dictionary! { "F1" => fuente },
                "XObject" => dictionary! { "Im0" => imagen },
                "ColorSpace" => dictionary! { "P0" => pantone },
                "ExtGState" => dictionary! { "G0" => dictionary! { "ca" => 0.5 } },
            },
        });
        doc.objects.insert(
            arbol,
            Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![pagina.into()], "Count" => 1 }),
        );
        let cat = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => arbol });
        doc.trailer.set("Root", cat);
        let mut b = Vec::new();
        doc.save_to(&mut b).unwrap();
        b
    }

    #[test]
    fn detecta_los_problemas_tipicos() {
        let f = Fuente::desde_bytes(&pdf_problematico()).unwrap();
        let inf = revisar(&f, &OpcionesPreflight::default());
        let codigo = |c: &str| inf.hallazgos.iter().find(|h| h.codigo == c);
        assert_eq!(codigo("fuente_no_incrustada").unwrap().nivel, Nivel::Error);
        assert!(codigo("fuente_no_incrustada").unwrap().mensaje.contains("Helvetica"));
        let res = codigo("resolucion_baja").unwrap();
        assert_eq!(res.nivel, Nivel::Error);
        assert!(res.mensaje.contains("50 ppi"), "{}", res.mensaje);
        assert!(codigo("cobertura").unwrap().mensaje.contains("400"));
        assert!(codigo("linea_fina").is_some());
        assert!(inf.hallazgos.iter().filter(|h| h.codigo == "rgb").count() >= 2, "RGB en vector e imagen");
        assert_eq!(inf.tintas_directas, ["PANTONE 186 C"]);
        assert_eq!(codigo("transparencia").unwrap().nivel, Nivel::Info);
        assert!(codigo("sin_trimbox").is_some());
        assert!(!inf.listo());

        // En PDF/X-1a la transparencia pasa a ser error.
        let x1a = OpcionesPreflight { pdfx: VersionPdfx::X1a, ..Default::default() };
        let inf = revisar(&f, &x1a);
        assert_eq!(inf.hallazgos.iter().find(|h| h.codigo == "transparencia").unwrap().nivel, Nivel::Error);
    }

    #[test]
    fn un_pdf_limpio_pasa() {
        let mut doc = Document::with_version("1.6");
        let arbol = doc.new_object_id();
        let flujo = doc.add_object(Stream::new(dictionary! {}, b"0 0 0 1 k 0 0 100 100 re f".to_vec()));
        let caja: Object = vec![0.into(), 0.into(), 300.into(), 300.into()].into();
        let trim: Object = vec![9.into(), 9.into(), 291.into(), 291.into()].into();
        let pagina = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => arbol, "MediaBox" => caja, "TrimBox" => trim, "Contents" => flujo,
        });
        doc.objects.insert(
            arbol,
            Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![pagina.into()], "Count" => 1 }),
        );
        let cat = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => arbol });
        doc.trailer.set("Root", cat);
        let mut b = Vec::new();
        doc.save_to(&mut b).unwrap();
        let inf = revisar(&Fuente::desde_bytes(&b).unwrap(), &OpcionesPreflight::default());
        assert!(inf.listo() && inf.advertencias == 0, "{:?}", inf.hallazgos);
    }

    #[test]
    fn rangos() {
        assert_eq!(rango_paginas(&[1, 2, 3, 7, 9, 10]), "1–3, 7, 9–10");
    }
}
