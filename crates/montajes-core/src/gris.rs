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
                .and_then(|s| s.dict.get(b"N").ok().and_then(|n| resolver(doc, n).as_i64().ok()));
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
        let (mut nuevos, sombras) = match &recursos {
            Some(r) => self.recursos(doc, r, profundidad),
            None => (Dictionary::new(), Vec::new()),
        };
        let (contenido, extra) = self.contenido(doc, &datos, recursos.as_ref(), &sombras);
        // Degradados e imágenes incrustadas que van con máscara: nuevos XObjects.
        if !sombras.is_empty() || !extra.is_empty() {
            let mut x =
                nuevos.get(b"XObject").ok().and_then(|t| resolver(doc, t).as_dict().ok()).cloned().unwrap_or_default();
            for (n, id) in sombras.iter().map(|(n, id)| (nombre_sombra(n), *id)).chain(extra) {
                x.set(n, Object::Reference(id));
            }
            nuevos.set("XObject", x);
        }
        let mut dict = s.dict.clone();
        dict.remove(b"Filter");
        dict.remove(b"DecodeParms");
        dict.remove(b"Length");
        if recursos.is_some() || !nuevos.is_empty() {
            dict.set("Resources", nuevos);
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

    /// Recursos con sus imágenes, formas, degradados y patrones en gris. Los
    /// degradados que no se pueden convertir se devuelven aparte, con la forma
    /// que los pinta en negro por máscara (para cambiar sus `sh`).
    fn recursos(
        &mut self,
        doc: &mut Document,
        r: &Dictionary,
        profundidad: usize,
    ) -> (Dictionary, Vec<(Vec<u8>, ObjectId)>) {
        let mut sombras = Vec::new();
        let mut salida = r.clone();
        for clave in [&b"XObject"[..], b"Shading", b"Pattern"] {
            let Some(tabla) = r.get(clave).ok().and_then(|t| resolver(doc, t).as_dict().ok()).cloned() else {
                continue;
            };
            let mut nueva = Dictionary::new();
            for (n, v) in tabla.iter() {
                let convertido = match (clave, v) {
                    (b"XObject", Object::Reference(id)) => self.xobjeto(doc, *id, profundidad),
                    (b"Shading", v) => match self.sombreado_gris(doc, v) {
                        Some(o) => o,
                        None => {
                            let grande = [-30000.0, -30000.0, 30000.0, 30000.0];
                            let forma = self.negro_por_mascara(
                                doc,
                                b"/S sh".to_vec(),
                                dictionary! { "Shading" => dictionary! { "S" => v.clone() } },
                                grande,
                            );
                            sombras.push((n.clone(), forma));
                            v.clone()
                        }
                    },
                    (b"Pattern", Object::Reference(id)) => self.patron(doc, *id, profundidad),
                    _ => v.clone(),
                };
                nueva.set(n.clone(), convertido);
            }
            salida.set(clave.to_vec(), nueva);
        }
        (salida, sombras)
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
        self.sombreado_gris(doc, o).unwrap_or_else(|| {
            self.avisar("hay degradados dentro de patrones que quedaron en color");
            o.clone()
        })
    }

    /// Degradado convertido a gris, o `None` si no se puede (malla, funciones raras).
    fn sombreado_gris(&mut self, doc: &mut Document, o: &Object) -> Option<Object> {
        let id = o.as_reference().ok();
        if let Some(hecho) = id.and_then(|i| self.hechos.get(&i)) {
            return Some(Object::Reference(*hecho));
        }
        let mut d = match resolver(doc, o) {
            Object::Dictionary(d) => d.clone(),
            _ => return None,
        };
        let cs = d.get(b"ColorSpace").map_or(Espacio::Otro, |c| espacio(doc, c, None, 0));
        let f = d.get(b"Function").ok().cloned()?;
        let f = funcion_gris(doc, &f, &cs)?;
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
        Some(Object::Reference(nuevo))
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
        let directa = if matches!(cs, Espacio::Otro | Espacio::Lab) { None } else { self.imagen_gris(doc, &s, &cs) };
        let nuevo = match directa {
            Some(gris) => doc.add_object(gris),
            // JPEG 2000, 16 bits, espacios raros…: negro por máscara de su luminosidad.
            None => self.negro_por_mascara(
                doc,
                b"q /I Do Q".to_vec(),
                dictionary! { "XObject" => dictionary! { "I" => id } },
                [0.0, 0.0, 1.0, 1.0],
            ),
        };
        self.hechos.insert(id, nuevo);
        nuevo
    }

    /// Una sola tinta para lo que no se puede convertir directamente: un
    /// relleno de negro 100 % (K) con una máscara de luminosidad del dibujo
    /// original (invertida: donde el dibujo es oscuro hay más negro). El RIP o
    /// el visor leen el dibujo, sea del formato que sea, y solo sale negro.
    /// Necesita transparencia (PDF/X-4 o un RIP moderno).
    fn negro_por_mascara(
        &mut self,
        doc: &mut Document,
        dibujo: Vec<u8>,
        recursos: Dictionary,
        caja: [f64; 4],
    ) -> ObjectId {
        self.avisar(
            "algunas imágenes o degradados se pasaron a negro con una máscara de luminosidad (necesita un RIP con transparencia, p. ej. PDF/X-4)",
        );
        let bbox: Vec<Object> = caja.iter().map(|v| Object::Real(*v as f32)).collect();
        let grupo = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => bbox.clone(),
                "Group" => dictionary! { "Type" => "Group", "S" => "Transparency", "CS" => "DeviceGray" },
                "Resources" => recursos,
            },
            dibujo,
        ));
        let inversa = doc.add_object(Stream::new(
            dictionary! {
                "FunctionType" => 4,
                "Domain" => vec![0.into(), 1.into()],
                "Range" => vec![0.into(), 1.into()],
            },
            b"{1 exch sub}".to_vec(),
        ));
        let estado = dictionary! {
            "Type" => "ExtGState",
            "SMask" => dictionary! { "Type" => "Mask", "S" => "Luminosity", "G" => grupo, "BC" => vec![1.into()], "TR" => inversa },
        };
        let [x0, y0, x1, y1] = caja;
        let contenido = format!("q /M gs 0 0 0 1 k {x0} {y0} {} {} re f Q", x1 - x0, y1 - y0);
        doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => bbox,
                "Resources" => dictionary! { "ExtGState" => dictionary! { "M" => estado } },
            },
            contenido.into_bytes(),
        ))
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
    /// Devuelve también las imágenes incrustadas (BI … EI) que pasaron a ser
    /// XObjects (nombre y objeto) para agregarlas a los recursos.
    fn contenido(
        &mut self,
        doc: &mut Document,
        datos: &[u8],
        recursos: Option<&Dictionary>,
        sombras: &[(Vec<u8>, ObjectId)],
    ) -> (Vec<u8>, Vec<(Vec<u8>, ObjectId)>) {
        let mut extra = Vec::new();
        let Ok(c) = Content::decode(datos) else {
            self.avisar("parte del contenido no se pudo leer y quedó en color");
            return (datos.to_vec(), extra);
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
                // Degradado que no se pudo convertir: su forma en negro por máscara.
                "sh" => {
                    if let Some(n) = op.operands.first().and_then(nombre)
                        && sombras.iter().any(|(s, _)| s.as_slice() == n)
                    {
                        salida.push(Operation::new("Do", vec![Object::Name(nombre_sombra(n))]));
                        continue;
                    }
                }
                // Imagen incrustada: pasa a ser una imagen XObject y se convierte como las demás.
                "BI" => {
                    if let Some(Object::Stream(st)) = op.operands.first()
                        && let Some(img) = imagen_incrustada(doc, st, recursos)
                    {
                        let mascara = img.dict.get(b"ImageMask").ok().and_then(|o| o.as_bool().ok()) == Some(true);
                        if !mascara {
                            let id = doc.add_object(img);
                            let convertida = self.imagen(doc, id);
                            let n = format!("KIn{}", extra.len() + 1).into_bytes();
                            extra.push((n.clone(), convertida));
                            salida.push(Operation::new("Do", vec![Object::Name(n)]));
                            continue;
                        }
                    }
                }
                _ => {}
            }
            salida.push(op);
        }
        (Content { operations: salida }.encode().unwrap_or_else(|_| datos.to_vec()), extra)
    }
}

/// Nombre del XObject que reemplaza a un degradado pintado por máscara.
fn nombre_sombra(n: &[u8]) -> Vec<u8> {
    [b"KSh".as_slice(), n].concat()
}

/// Imagen incrustada (BI … ID … EI) como imagen XObject: claves y nombres
/// abreviados a su forma completa.
fn imagen_incrustada(doc: &Document, st: &Stream, recursos: Option<&Dictionary>) -> Option<Stream> {
    let largo = |k: &[u8]| -> Vec<u8> {
        match k {
            b"W" => b"Width".to_vec(),
            b"H" => b"Height".to_vec(),
            b"BPC" => b"BitsPerComponent".to_vec(),
            b"CS" => b"ColorSpace".to_vec(),
            b"F" => b"Filter".to_vec(),
            b"DP" => b"DecodeParms".to_vec(),
            b"D" => b"Decode".to_vec(),
            b"IM" => b"ImageMask".to_vec(),
            b"I" => b"Interpolate".to_vec(),
            otra => otra.to_vec(),
        }
    };
    let valor = |o: &Object| -> Object {
        match o {
            Object::Name(n) => Object::Name(
                match n.as_slice() {
                    b"G" => b"DeviceGray".as_slice(),
                    b"RGB" => b"DeviceRGB",
                    b"CMYK" => b"DeviceCMYK",
                    b"I" => b"Indexed",
                    b"AHx" => b"ASCIIHexDecode",
                    b"A85" => b"ASCII85Decode",
                    b"LZW" => b"LZWDecode",
                    b"Fl" => b"FlateDecode",
                    b"RL" => b"RunLengthDecode",
                    b"CCF" => b"CCITTFaxDecode",
                    b"DCT" => b"DCTDecode",
                    otro => otro,
                }
                .to_vec(),
            ),
            Object::Array(a) => Object::Array(
                a.iter().map(|x| if let Object::Name(_) = x { valor_simple(x) } else { x.clone() }).collect(),
            ),
            otro => otro.clone(),
        }
    };
    let mut dict = dictionary! { "Type" => "XObject", "Subtype" => "Image" };
    for (k, v) in st.dict.iter() {
        let clave = largo(k);
        let mut v = valor(v);
        // Espacio de color con nombre de recurso (/CS /Cs1).
        if clave == b"ColorSpace"
            && let Object::Name(n) = &v
            && !matches!(n.as_slice(), b"DeviceGray" | b"DeviceRGB" | b"DeviceCMYK")
            && let Some(cs) = recursos
                .and_then(|r| r.get(b"ColorSpace").ok())
                .and_then(|t| resolver(doc, t).as_dict().ok())
                .and_then(|t| t.get(n).ok())
        {
            v = cs.clone();
        }
        dict.set(clave, v);
    }
    dict.has(b"Width").then(|| Stream::new(dict, st.content.clone()))
}

/// Nombre abreviado de filtro o espacio dentro de un arreglo (imagen incrustada).
fn valor_simple(o: &Object) -> Object {
    match o.as_name() {
        Ok(b"AHx") => Object::Name(b"ASCIIHexDecode".to_vec()),
        Ok(b"A85") => Object::Name(b"ASCII85Decode".to_vec()),
        Ok(b"LZW") => Object::Name(b"LZWDecode".to_vec()),
        Ok(b"Fl") => Object::Name(b"FlateDecode".to_vec()),
        Ok(b"RL") => Object::Name(b"RunLengthDecode".to_vec()),
        Ok(b"CCF") => Object::Name(b"CCITTFaxDecode".to_vec()),
        Ok(b"DCT") => Object::Name(b"DCTDecode".to_vec()),
        Ok(b"I") => Object::Name(b"Indexed".to_vec()),
        Ok(b"RGB") => Object::Name(b"DeviceRGB".to_vec()),
        Ok(b"CMYK") => Object::Name(b"DeviceCMYK".to_vec()),
        Ok(b"G") => Object::Name(b"DeviceGray".to_vec()),
        _ => o.clone(),
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

    #[test]
    fn lo_que_no_se_puede_leer_va_en_negro_por_mascara() {
        let mut doc = Document::with_version("1.7");
        // Imagen de 16 bits (no se convierte directo) y una imagen incrustada RGB.
        let img = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Image", "Width" => 1, "Height" => 1,
                "BitsPerComponent" => 16, "ColorSpace" => "DeviceRGB",
            },
            vec![255, 255, 0, 0, 0, 0],
        ));
        let forma = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Form", "BBox" => vec![0.into(), 0.into(), 10.into(), 10.into()],
                "Resources" => dictionary! { "XObject" => dictionary! { "Im" => img } },
            },
            b"/Im Do BI /W 1 /H 1 /BPC 8 /CS /RGB ID \xff\x00\x00 EI".to_vec(),
        ));
        let mut conv = Conversor::new();
        let gris = conv.forma(&mut doc, forma);
        let s = doc.get_object(gris).unwrap().as_stream().unwrap();
        let texto = String::from_utf8_lossy(&s.decompressed_content().unwrap()).to_string();
        assert!(!texto.contains("BI"), "la imagen incrustada pasa a XObject: {texto}");
        let x = s.dict.get(b"Resources").unwrap().as_dict().unwrap().get(b"XObject").unwrap().as_dict().unwrap();
        let im = doc.get_object(x.get(b"Im").unwrap().as_reference().unwrap()).unwrap().as_stream().unwrap();
        // La de 16 bits queda como un relleno de negro con máscara de luminosidad.
        assert_eq!(im.dict.get(b"Subtype").unwrap().as_name().unwrap(), b"Form");
        assert!(String::from_utf8_lossy(&im.content).contains("0 0 0 1 k"));
        assert!(conv.avisos.iter().any(|a| a.contains("máscara")));
    }
}
