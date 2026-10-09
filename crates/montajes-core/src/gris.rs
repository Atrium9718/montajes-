//! Conversión a una tinta (negro).
//!
//! Para los trabajos a una tinta se hace una copia de cada página en escala de
//! grises (`DeviceGray`, que separa solo en la plancha de negro): los colores
//! de los trazos y rellenos, los degradados y las imágenes (sin comprimir,
//! Flate o JPEG). Lo que no se puede convertir se deja igual y se avisa.

use std::collections::HashMap;

use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, dictionary};

use crate::pdf::resolver;

const PROFUNDIDAD_MAXIMA: usize = 16;

/// Espacio de color, lo justo para pasar un color a gris.
#[derive(Clone, Debug)]
enum Espacio {
    Gris,
    Rgb,
    Cmyk,
    /// Separación (una tinta directa): el tono pasa a negro.
    Tinta,
    /// DeviceN con n tintas.
    Varias,
    Lab,
    /// Indexado: espacio base y tabla.
    Indexado(Box<Espacio>, Vec<u8>),
    Patron,
    Otro,
}

fn luminancia(r: f64, g: f64, b: f64) -> f64 {
    (0.30 * r + 0.59 * g + 0.11 * b).clamp(0.0, 1.0)
}

fn cmyk_a_gris(c: f64, m: f64, y: f64, k: f64) -> f64 {
    luminancia((1.0 - c) * (1.0 - k), (1.0 - m) * (1.0 - k), (1.0 - y) * (1.0 - k))
}

impl Espacio {
    fn componentes(&self) -> usize {
        match self {
            Espacio::Rgb | Espacio::Lab => 3,
            Espacio::Cmyk => 4,
            _ => 1,
        }
    }

    /// Gris (0 negro – 1 blanco) de un color en este espacio.
    fn gris(&self, v: &[f64]) -> Option<f64> {
        Some(match (self, v) {
            (Espacio::Gris, [g, ..]) => *g,
            (Espacio::Rgb, [r, g, b, ..]) => luminancia(*r, *g, *b),
            (Espacio::Cmyk, [c, m, y, k, ..]) => cmyk_a_gris(*c, *m, *y, *k),
            (Espacio::Tinta, [t, ..]) => 1.0 - t.clamp(0.0, 1.0),
            (Espacio::Varias, v) if !v.is_empty() => 1.0 - v.iter().sum::<f64>().clamp(0.0, 1.0),
            (Espacio::Lab, [l, ..]) => (l / 100.0).clamp(0.0, 1.0),
            (Espacio::Indexado(base, tabla), [i, ..]) => {
                let n = base.componentes();
                let i = (*i).max(0.0) as usize * n;
                let c: Vec<f64> = tabla.get(i..i + n)?.iter().map(|b| f64::from(*b) / 255.0).collect();
                base.gris(&c)?
            }
            _ => return None,
        })
    }
}

fn nombre(o: &Object) -> Option<&[u8]> {
    o.as_name().ok()
}

fn numeros(ops: &[Object]) -> Vec<f64> {
    ops.iter().filter_map(|o| o.as_float().ok().map(f64::from)).collect()
}

/// Espacio de color de un objeto (nombre o arreglo), resolviendo referencias.
fn espacio(doc: &Document, o: &Object, recursos: Option<&Dictionary>, profundidad: u8) -> Espacio {
    if profundidad > 6 {
        return Espacio::Otro;
    }
    let o = resolver(doc, o);
    if let Some(n) = nombre(o) {
        return match n {
            b"DeviceGray" | b"G" | b"CalGray" => Espacio::Gris,
            b"DeviceRGB" | b"RGB" | b"CalRGB" => Espacio::Rgb,
            b"DeviceCMYK" | b"CMYK" => Espacio::Cmyk,
            b"Pattern" => Espacio::Patron,
            otro => {
                // Nombre de un recurso /ColorSpace.
                let tabla = recursos
                    .and_then(|r| r.get(b"ColorSpace").ok())
                    .and_then(|t| resolver(doc, t).as_dict().ok())
                    .and_then(|t| t.get(otro).ok());
                match tabla {
                    Some(cs) => espacio(doc, cs, None, profundidad + 1),
                    None => Espacio::Otro,
                }
            }
        };
    }
    let Ok(a) = o.as_array() else { return Espacio::Otro };
    match a.first().and_then(nombre) {
        Some(b"ICCBased") => {
            let n = a
                .get(1)
                .and_then(|s| resolver(doc, s).as_stream().ok())
                .and_then(|s| s.dict.get(b"N").ok().and_then(|n| n.as_i64().ok()));
            match n {
                Some(1) => Espacio::Gris,
                Some(3) => Espacio::Rgb,
                Some(4) => Espacio::Cmyk,
                _ => Espacio::Otro,
            }
        }
        Some(b"CalGray") => Espacio::Gris,
        Some(b"CalRGB") => Espacio::Rgb,
        Some(b"Lab") => Espacio::Lab,
        Some(b"Separation") => Espacio::Tinta,
        Some(b"DeviceN") => Espacio::Varias,
        Some(b"Pattern") => Espacio::Patron,
        Some(b"Indexed" | b"I") => {
            let base = a.get(1).map_or(Espacio::Otro, |b| espacio(doc, b, recursos, profundidad + 1));
            let tabla = match a.get(3).map(|t| resolver(doc, t)) {
                Some(Object::String(s, _)) => s.clone(),
                Some(Object::Stream(s)) => s.decompressed_content().unwrap_or_else(|_| s.content.clone()),
                _ => Vec::new(),
            };
            Espacio::Indexado(Box::new(base), tabla)
        }
        _ => Espacio::Otro,
    }
}

/// Convierte formas, imágenes y degradados a gris, reutilizando lo ya hecho.
pub struct Conversor {
    hechos: HashMap<ObjectId, ObjectId>,
    pub avisos: Vec<String>,
}

impl Default for Conversor {
    fn default() -> Self {
        Self::new()
    }
}

impl Conversor {
    pub fn new() -> Self {
        Self { hechos: HashMap::new(), avisos: Vec::new() }
    }

    fn avisar(&mut self, texto: &str) {
        if !self.avisos.iter().any(|a| a == texto) {
            self.avisos.push(texto.to_string());
        }
    }

    /// Copia en gris de un Form XObject (p. ej. la forma de una página).
    pub fn forma(&mut self, doc: &mut Document, id: ObjectId) -> ObjectId {
        self.forma_en(doc, id, 0)
    }

    fn forma_en(&mut self, doc: &mut Document, id: ObjectId, profundidad: usize) -> ObjectId {
        if let Some(hecho) = self.hechos.get(&id) {
            return *hecho;
        }
        if profundidad > PROFUNDIDAD_MAXIMA {
            return id;
        }
        let Ok(s) = doc.get_object(id).and_then(|o| o.as_stream()).cloned() else { return id };
        // Se reserva el destino antes de recorrer (formas que se usan a sí mismas).
        let nuevo = doc.new_object_id();
        self.hechos.insert(id, nuevo);
        let datos = s.decompressed_content().unwrap_or_else(|_| s.content.clone());
        let recursos = s.dict.get(b"Resources").ok().and_then(|r| resolver(doc, r).as_dict().ok()).cloned();
        let contenido = self.contenido(doc, &datos, recursos.as_ref(), profundidad);
        let mut dict = s.dict.clone();
        dict.remove(b"Filter");
        dict.remove(b"DecodeParms");
        dict.remove(b"Length");
        if let Some(r) = recursos {
            dict.set("Resources", self.recursos(doc, &r, profundidad));
        }
        // El grupo de transparencia también pasa a gris.
        if let Ok(Object::Dictionary(g)) = dict.get(b"Group").map(|g| resolver(doc, g).clone()) {
            let mut g = g;
            if g.has(b"CS") {
                g.set("CS", "DeviceGray");
            }
            dict.set("Group", g);
        }
        let mut flujo = Stream::new(dict, contenido);
        let _ = flujo.compress();
        doc.objects.insert(nuevo, Object::Stream(flujo));
        nuevo
    }

    /// Recursos con sus imágenes, formas, degradados y patrones en gris.
    fn recursos(&mut self, doc: &mut Document, r: &Dictionary, profundidad: usize) -> Dictionary {
        let mut salida = r.clone();
        for clave in [&b"XObject"[..], b"Shading", b"Pattern"] {
            let Some(tabla) = r.get(clave).ok().and_then(|t| resolver(doc, t).as_dict().ok()).cloned() else {
                continue;
            };
            let mut nueva = Dictionary::new();
            for (n, v) in tabla.iter() {
                let convertido = match (clave, v) {
                    (b"XObject", Object::Reference(id)) => self.xobjeto(doc, *id, profundidad),
                    (b"Shading", v) => self.sombreado(doc, v),
                    (b"Pattern", Object::Reference(id)) => self.patron(doc, *id, profundidad),
                    _ => v.clone(),
                };
                nueva.set(n.clone(), convertido);
            }
            salida.set(clave.to_vec(), nueva);
        }
        salida
    }

    fn xobjeto(&mut self, doc: &mut Document, id: ObjectId, profundidad: usize) -> Object {
        let tipo = doc
            .get_object(id)
            .and_then(|o| o.as_stream())
            .ok()
            .and_then(|s| s.dict.get(b"Subtype").ok().and_then(nombre).map(<[u8]>::to_vec));
        match tipo.as_deref() {
            Some(b"Form") => Object::Reference(self.forma_en(doc, id, profundidad + 1)),
            Some(b"Image") => Object::Reference(self.imagen(doc, id)),
            _ => Object::Reference(id),
        }
    }

    fn patron(&mut self, doc: &mut Document, id: ObjectId, profundidad: usize) -> Object {
        if let Some(hecho) = self.hechos.get(&id) {
            return Object::Reference(*hecho);
        }
        let Ok(objeto) = doc.get_object(id).cloned() else { return Object::Reference(id) };
        let tipo = match &objeto {
            Object::Stream(s) => s.dict.get(b"PatternType").ok().and_then(|o| o.as_i64().ok()),
            Object::Dictionary(d) => d.get(b"PatternType").ok().and_then(|o| o.as_i64().ok()),
            _ => None,
        };
        match (tipo, objeto) {
            // Patrón de mosaico: su contenido es como el de una forma.
            (Some(1), Object::Stream(_)) => Object::Reference(self.forma_en(doc, id, profundidad + 1)),
            // Patrón de degradado.
            (Some(2), Object::Dictionary(mut d)) => {
                if let Ok(s) = d.get(b"Shading").cloned() {
                    d.set("Shading", self.sombreado(doc, &s));
                }
                let nuevo = doc.add_object(Object::Dictionary(d));
                self.hechos.insert(id, nuevo);
                Object::Reference(nuevo)
            }
            _ => Object::Reference(id),
        }
    }

    /// Degradado en gris: se pasan a gris los colores de su función (tipos 2
    /// y 3, los que hacen los programas de diseño para degradados).
    fn sombreado(&mut self, doc: &mut Document, o: &Object) -> Object {
        let id = o.as_reference().ok();
        if let Some(hecho) = id.and_then(|i| self.hechos.get(&i)) {
            return Object::Reference(*hecho);
        }
        let mut d = match resolver(doc, o) {
            Object::Dictionary(d) => d.clone(),
            Object::Stream(_) => {
                self.avisar("hay degradados de malla que quedaron en color");
                return o.clone();
            }
            _ => return o.clone(),
        };
        let cs = d.get(b"ColorSpace").map_or(Espacio::Otro, |c| espacio(doc, c, None, 0));
        let Some(f) = d.get(b"Function").ok().cloned() else {
            self.avisar("hay degradados que quedaron en color");
            return o.clone();
        };
        let Some(f) = funcion_gris(doc, &f, &cs) else {
            self.avisar("hay degradados que quedaron en color");
            return o.clone();
        };
        d.set("Function", f);
        d.set("ColorSpace", "DeviceGray");
        if let Ok(fondo) = d.get(b"Background").and_then(|b| b.as_array()).map(|a| numeros(a)) {
            match cs.gris(&fondo) {
                Some(g) => d.set("Background", vec![Object::Real(g as f32)]),
                None => {
                    d.remove(b"Background");
                }
            }
        }
        let nuevo = doc.add_object(Object::Dictionary(d));
        if let Some(i) = id {
            self.hechos.insert(i, nuevo);
        }
        Object::Reference(nuevo)
    }

    /// Imagen en gris de 8 bits (Flate). Las máscaras y las que ya son grises
    /// quedan igual.
    fn imagen(&mut self, doc: &mut Document, id: ObjectId) -> ObjectId {
        if let Some(hecho) = self.hechos.get(&id) {
            return *hecho;
        }
        let Ok(s) = doc.get_object(id).and_then(|o| o.as_stream()).cloned() else { return id };
        let d = &s.dict;
        if d.get(b"ImageMask").ok().and_then(|o| o.as_bool().ok()) == Some(true) {
            return id;
        }
        let cs = d.get(b"ColorSpace").map_or(Espacio::Otro, |c| espacio(doc, c, None, 0));
        if matches!(cs, Espacio::Gris) {
            return id;
        }
        match self.imagen_gris(doc, &s, &cs) {
            Some(gris) => {
                let nuevo = doc.add_object(gris);
                self.hechos.insert(id, nuevo);
                nuevo
            }
            None => {
                self.avisar("hay imágenes que no se pudieron pasar a negro (quedaron en color)");
                id
            }
        }
    }

    fn imagen_gris(&self, doc: &Document, s: &Stream, cs: &Espacio) -> Option<Stream> {
        let d = &s.dict;
        let entero = |k: &[u8]| d.get(k).ok().map(|o| resolver(doc, o)).and_then(|o| o.as_i64().ok());
        let (w, h) = (entero(b"Width")? as usize, entero(b"Height")? as usize);
        let filtros: Vec<Vec<u8>> = match d.get(b"Filter").ok().map(|f| resolver(doc, f)) {
            Some(Object::Name(n)) => vec![n.clone()],
            Some(Object::Array(a)) => a.iter().filter_map(|x| nombre(x).map(<[u8]>::to_vec)).collect(),
            _ => Vec::new(),
        };
        let decode = d.get(b"Decode").ok().and_then(|o| resolver(doc, o).as_array().ok()).map(|a| numeros(a));
        let (muestras, n, bpc) = match filtros.last().map(Vec::as_slice) {
            Some(b"DCTDecode" | b"DCT") => {
                let datos = if filtros.len() > 1 {
                    // p. ej. [/ASCII85Decode /DCTDecode]: se quitan los filtros de antes.
                    let mut previo = s.clone();
                    let antes: Vec<Object> =
                        filtros[..filtros.len() - 1].iter().map(|f| Object::Name(f.clone())).collect();
                    previo.dict.set("Filter", antes);
                    previo.dict.remove(b"DecodeParms");
                    previo.decompressed_content().ok()?
                } else {
                    s.content.clone()
                };
                let mut dec = jpeg_decoder::Decoder::new(&datos[..]);
                let pix = dec.decode().ok()?;
                let info = dec.info()?;
                let n = match info.pixel_format {
                    jpeg_decoder::PixelFormat::L8 => 1,
                    jpeg_decoder::PixelFormat::RGB24 => 3,
                    jpeg_decoder::PixelFormat::CMYK32 => 4,
                    _ => return None,
                };
                let mut pix = pix;
                if n == 4 {
                    // El decodificador entrega los CMYK invertidos (255 − valor); el
                    // PDF aplica /Decode a los valores tal como vienen en el JPEG.
                    let ycck = transformacion_adobe(&datos) == Some(2);
                    for c in pix.chunks_exact_mut(4) {
                        if ycck {
                            c[3] = 255 - c[3];
                        } else {
                            c.iter_mut().for_each(|v| *v = 255 - *v);
                        }
                    }
                }
                (pix, n, 8)
            }
            _ => {
                let datos = s.decompressed_content().ok().or_else(|| filtros.is_empty().then(|| s.content.clone()))?;
                let bpc = entero(b"BitsPerComponent").unwrap_or(8) as usize;
                let n = match cs {
                    Espacio::Indexado(..) | Espacio::Tinta | Espacio::Gris => 1,
                    otro => otro.componentes(),
                };
                (datos, n, bpc)
            }
        };
        if !matches!(bpc, 1 | 2 | 4 | 8) {
            return None;
        }
        // Lector de muestras por fila (las filas empiezan en byte entero).
        let fila = (w * n * bpc).div_ceil(8);
        if muestras.len() < fila * h {
            return None;
        }
        let maximo = ((1u32 << bpc) - 1) as f64;
        let muestra = |y: usize, i: usize| -> f64 {
            let bit = i * bpc;
            let b = muestras[y * fila + bit / 8];
            let v = if bpc == 8 { b } else { (b >> (8 - bpc - bit % 8)) & ((1u16 << bpc) - 1) as u8 };
            f64::from(v)
        };
        let indexado = matches!(cs, Espacio::Indexado(..));
        let mut gris = Vec::with_capacity(w * h);
        let mut v = vec![0.0; n];
        for y in 0..h {
            for x in 0..w {
                for (c, slot) in v.iter_mut().enumerate() {
                    let crudo = muestra(y, x * n + c);
                    *slot = if indexado {
                        crudo
                    } else {
                        let t = crudo / maximo;
                        match &decode {
                            Some(dd) if dd.len() >= 2 * n => dd[2 * c] + t * (dd[2 * c + 1] - dd[2 * c]),
                            _ => t,
                        }
                    };
                }
                let g = cs.gris(&v).unwrap_or(1.0);
                gris.push((g * 255.0).round().clamp(0.0, 255.0) as u8);
            }
        }
        let mut dict = dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => w as i64,
            "Height" => h as i64,
            "BitsPerComponent" => 8,
            "ColorSpace" => "DeviceGray",
        };
        for clave in [&b"SMask"[..], b"Interpolate", b"Intent", b"OC"] {
            if let Ok(o) = d.get(clave) {
                dict.set(clave.to_vec(), o.clone());
            }
        }
        let mut flujo = Stream::new(dict, gris);
        let _ = flujo.compress();
        Some(flujo)
    }

    /// Flujo de contenido con todos los colores en gris.
    fn contenido(
        &mut self,
        doc: &Document,
        datos: &[u8],
        recursos: Option<&Dictionary>,
        _profundidad: usize,
    ) -> Vec<u8> {
        let Ok(c) = Content::decode(datos) else {
            self.avisar("parte del contenido no se pudo leer y quedó en color");
            return datos.to_vec();
        };
        let mut relleno = Espacio::Gris;
        let mut trazo = Espacio::Gris;
        let mut pila: Vec<(Espacio, Espacio)> = Vec::new();
        let mut salida: Vec<Operation> = Vec::with_capacity(c.operations.len());
        let gris = |op: &str, g: f64| Operation::new(op, vec![Object::Real(g as f32)]);
        for op in c.operations {
            let v = numeros(&op.operands);
            match op.operator.as_str() {
                "q" => pila.push((relleno.clone(), trazo.clone())),
                "Q" => {
                    if let Some((r, t)) = pila.pop() {
                        relleno = r;
                        trazo = t;
                    }
                }
                "rg" if v.len() == 3 => {
                    relleno = Espacio::Gris;
                    salida.push(gris("g", luminancia(v[0], v[1], v[2])));
                    continue;
                }
                "RG" if v.len() == 3 => {
                    trazo = Espacio::Gris;
                    salida.push(gris("G", luminancia(v[0], v[1], v[2])));
                    continue;
                }
                "k" if v.len() == 4 => {
                    relleno = Espacio::Gris;
                    salida.push(gris("g", cmyk_a_gris(v[0], v[1], v[2], v[3])));
                    continue;
                }
                "K" if v.len() == 4 => {
                    trazo = Espacio::Gris;
                    salida.push(gris("G", cmyk_a_gris(v[0], v[1], v[2], v[3])));
                    continue;
                }
                "g" => relleno = Espacio::Gris,
                "G" => trazo = Espacio::Gris,
                "cs" | "CS" => {
                    let e = op.operands.first().map_or(Espacio::Otro, |o| espacio(doc, o, recursos, 0));
                    let es_patron = matches!(e, Espacio::Patron);
                    if op.operator == "cs" {
                        relleno = e;
                    } else {
                        trazo = e;
                    }
                    if !es_patron {
                        salida.push(Operation::new(op.operator.as_str(), vec![Object::Name(b"DeviceGray".to_vec())]));
                        continue;
                    }
                }
                "sc" | "scn" | "SC" | "SCN" => {
                    let es_relleno = op.operator.starts_with('s');
                    let e = if es_relleno { &relleno } else { &trazo };
                    if !matches!(e, Espacio::Patron) {
                        let g = e.gris(&v).unwrap_or(0.0);
                        if !matches!(
                            e,
                            Espacio::Gris
                                | Espacio::Rgb
                                | Espacio::Cmyk
                                | Espacio::Tinta
                                | Espacio::Varias
                                | Espacio::Lab
                                | Espacio::Indexado(..)
                        ) {
                            self.avisar("hay colores en un espacio poco común: se pasaron a negro");
                        }
                        salida.push(gris(if es_relleno { "sc" } else { "SC" }, g));
                        continue;
                    }
                }
                "BI" => self.avisar("hay imágenes incrustadas en el contenido que quedaron en color"),
                _ => {}
            }
            salida.push(op);
        }
        Content { operations: salida }.encode().unwrap_or_else(|_| datos.to_vec())
    }
}

/// Transformación de color del marcador Adobe (APP14) de un JPEG: 0 sin
/// transformar, 1 YCbCr, 2 YCCK.
fn transformacion_adobe(jpeg: &[u8]) -> Option<u8> {
    let mut i = 2;
    while i + 4 <= jpeg.len() {
        if jpeg[i] != 0xFF {
            return None;
        }
        let marca = jpeg[i + 1];
        let largo = usize::from(u16::from_be_bytes([jpeg[i + 2], jpeg[i + 3]]));
        if marca == 0xEE && jpeg.get(i + 4..i + 9) == Some(b"Adobe") {
            return jpeg.get(i + 4 + 11).copied();
        }
        if marca == 0xDA {
            return None;
        }
        i += 2 + largo;
    }
    None
}

/// Función de degradado que da gris (tipos 2 y 3; los demás no se convierten).
fn funcion_gris(doc: &Document, f: &Object, cs: &Espacio) -> Option<Object> {
    let d = match resolver(doc, f) {
        Object::Dictionary(d) => d.clone(),
        Object::Array(a) => {
            // Una función por componente: solo si es un único componente.
            return if a.len() == 1 { funcion_gris(doc, &a[0], cs) } else { None };
        }
        _ => return None,
    };
    match d.get(b"FunctionType").ok().and_then(|o| o.as_i64().ok())? {
        2 => {
            let n = cs.componentes();
            let c0 = d.get(b"C0").ok().and_then(|o| resolver(doc, o).as_array().ok()).map(|a| numeros(a));
            let c1 = d.get(b"C1").ok().and_then(|o| resolver(doc, o).as_array().ok()).map(|a| numeros(a));
            let c0 = c0.unwrap_or_else(|| vec![0.0; n]);
            let c1 = c1.unwrap_or_else(|| vec![1.0; n]);
            let mut nueva = d.clone();
            nueva.set("C0", vec![Object::Real(cs.gris(&c0)? as f32)]);
            nueva.set("C1", vec![Object::Real(cs.gris(&c1)? as f32)]);
            nueva.remove(b"Range");
            Some(Object::Dictionary(nueva))
        }
        3 => {
            let partes = d.get(b"Functions").ok().and_then(|o| resolver(doc, o).as_array().ok())?.clone();
            let convertidas: Option<Vec<Object>> = partes.iter().map(|p| funcion_gris(doc, p, cs)).collect();
            let mut nueva = d.clone();
            nueva.set("Functions", convertidas?);
            nueva.remove(b"Range");
            Some(Object::Dictionary(nueva))
        }
        _ => None,
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn pasa_colores_e_imagenes_a_gris() {
        let mut doc = Document::with_version("1.7");
        // Imagen RGB de 2×1: rojo y blanco.
        let img = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Image", "Width" => 2, "Height" => 1,
                "BitsPerComponent" => 8, "ColorSpace" => "DeviceRGB",
            },
            vec![255, 0, 0, 255, 255, 255],
        ));
        let forma = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Form", "BBox" => vec![0.into(), 0.into(), 10.into(), 10.into()],
                "Resources" => dictionary! { "XObject" => dictionary! { "Im" => img } },
            },
            b"1 0 0 rg 0 0 5 5 re f 0 1 0 0 K 0 0 m 5 5 l S /Im Do".to_vec(),
        ));
        let mut conv = Conversor::new();
        let gris = conv.forma(&mut doc, forma);
        let s = doc.get_object(gris).unwrap().as_stream().unwrap();
        let texto = String::from_utf8(s.decompressed_content().unwrap()).unwrap();
        assert!(texto.contains("0.3 g"), "{texto}");
        assert!(!texto.contains(" rg") && !texto.contains(" K"), "{texto}");
        let recursos = s.dict.get(b"Resources").unwrap().as_dict().unwrap();
        let im = recursos.get(b"XObject").unwrap().as_dict().unwrap().get(b"Im").unwrap().as_reference().unwrap();
        let im = doc.get_object(im).unwrap().as_stream().unwrap();
        assert_eq!(im.dict.get(b"ColorSpace").unwrap().as_name().unwrap(), b"DeviceGray");
        assert_eq!(im.decompressed_content().unwrap(), vec![77, 255]);
        assert!(conv.avisos.is_empty());
    }
}
