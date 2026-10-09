//! Capa de fondo de una página, para extender el rebase.
//!
//! Se copia el contenido de la página quedándose solo con lo que hace de
//! fondo: imágenes grandes (la foto), rellenos grandes y sencillos (planos de
//! color, franjas, degradados) y sombreados. Se quitan los textos, los trazos,
//! los rellenos pequeños o muy recortados (letras en curvas, logos, QR) y los
//! rellenos blancos. El recorte de la mesa de trabajo también se quita, así una
//! foto más grande que la página deja ver su continuación real.

use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, dictionary};

use crate::Resultado;
use crate::pdf::{Caja, Fuente, LIMITE_CONTENIDO, PaginaFuente, heredado, resolver};

const PROFUNDIDAD_MAXIMA: usize = 12;
/// Margen (pt) alrededor de la página que puede pintar la capa de fondo (15 mm).
pub const MARGEN_FONDO: f64 = 42.52;
/// Un relleno con más subtrazados que esto es un dibujo (letras, logos), no un fondo.
const SUBTRAZOS_MAXIMOS: usize = 8;

/// Medidas del corte de la página, para decidir qué es «grande».
struct Referencia {
    area: f64,
    ancho: f64,
    alto: f64,
}

#[derive(Clone, Copy)]
struct Limites {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

impl Limites {
    const VACIO: Self = Self { x0: f64::MAX, y0: f64::MAX, x1: f64::MIN, y1: f64::MIN };

    fn sumar(&mut self, m: &[f64; 6], x: f64, y: f64) {
        let (px, py) = (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]);
        self.x0 = self.x0.min(px);
        self.y0 = self.y0.min(py);
        self.x1 = self.x1.max(px);
        self.y1 = self.y1.max(py);
    }

    fn medidas(&self) -> (f64, f64) {
        ((self.x1 - self.x0).max(0.0), (self.y1 - self.y0).max(0.0))
    }

    /// Ocupa una parte grande de la página (o la cruza casi entera).
    fn grande(&self, r: &Referencia) -> bool {
        let (w, h) = self.medidas();
        w > 0.0 && h > 0.0 && (w * h >= 0.15 * r.area || w >= 0.5 * r.ancho || h >= 0.5 * r.alto)
    }

    /// Cubre casi toda la página: el recorte de la mesa de trabajo.
    fn pagina_entera(&self, r: &Referencia) -> bool {
        let (w, h) = self.medidas();
        w * h >= 0.85 * r.area
    }
}

fn multiplicar(a: &[f64; 6], b: &[f64; 6]) -> [f64; 6] {
    [
        a[0] * b[0] + a[1] * b[2],
        a[0] * b[1] + a[1] * b[3],
        a[2] * b[0] + a[3] * b[2],
        a[2] * b[1] + a[3] * b[3],
        a[4] * b[0] + a[5] * b[2] + b[4],
        a[4] * b[1] + a[5] * b[3] + b[5],
    ]
}

fn numeros(ops: &[Object]) -> Vec<f64> {
    ops.iter().filter_map(|o| o.as_float().ok().map(f64::from)).collect()
}

fn matriz(o: Option<&Object>, doc: &Document) -> [f64; 6] {
    o.and_then(|o| resolver(doc, o).as_array().ok())
        .map(|a| a.iter().filter_map(|x| resolver(doc, x).as_float().ok().map(f64::from)).collect::<Vec<_>>())
        .filter(|v| v.len() == 6)
        .map_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0], |v| [v[0], v[1], v[2], v[3], v[4], v[5]])
}

/// Form XObject con solo la capa de fondo de la página, o `None` si la página
/// no tiene nada que sirva de fondo. Su BBox deja pintar hasta
/// [`MARGEN_FONDO`] fuera de la página.
pub fn forma_fondo(doc: &mut Document, p: &PaginaFuente) -> Option<ObjectId> {
    let datos = doc.get_page_content_with_limit(p.id, LIMITE_CONTENIDO).ok()?;
    let recursos = heredado(doc, p.id, b"Resources").and_then(|r| resolver(doc, &r).as_dict().ok().cloned());
    let c = &p.corte;
    let r = Referencia { area: c.ancho() * c.alto(), ancho: c.ancho(), alto: c.alto() };
    let identidad = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let (contenido, nuevos) = filtrar(doc, &datos, recursos.as_ref(), identidad, &r, 0)?;
    let recursos = nuevos.or(recursos).unwrap_or_default();
    let Caja { x0, y0, x1, y1 } = p.media;
    let mut dict = dictionary! {
        "Type" => "XObject",
        "Subtype" => "Form",
        "BBox" => vec![
            Object::Real((x0 - MARGEN_FONDO) as f32),
            Object::Real((y0 - MARGEN_FONDO) as f32),
            Object::Real((x1 + MARGEN_FONDO) as f32),
            Object::Real((y1 + MARGEN_FONDO) as f32),
        ],
        "Resources" => recursos,
    };
    // Como grupo de transparencia: se pinta de una vez y el recorte del rebase
    // suaviza un solo borde (si no, los visores dejan filos entre capas).
    let grupo = doc.get_dictionary(p.id).and_then(|d| d.get(b"Group")).ok().cloned();
    dict.set(
        "Group",
        grupo.unwrap_or_else(|| Object::Dictionary(dictionary! { "Type" => "Group", "S" => "Transparency" })),
    );
    Some(doc.add_object(Stream::new(dict, contenido)))
}

/// Filtra un flujo de contenido. Devuelve el contenido nuevo y, si cambió
/// algún Form XObject anidado, los recursos con esos reemplazos.
fn filtrar(
    doc: &mut Document,
    datos: &[u8],
    recursos: Option<&Dictionary>,
    ctm: [f64; 6],
    r: &Referencia,
    profundidad: usize,
) -> Option<(Vec<u8>, Option<Dictionary>)> {
    if profundidad > PROFUNDIDAD_MAXIMA {
        return None;
    }
    let contenido = Content::decode(datos).ok()?;
    let xobjetos: Option<Dictionary> =
        recursos.and_then(|d| d.get(b"XObject").ok()).and_then(|x| resolver(doc, x).as_dict().ok().cloned());

    let mut ctm = ctm;
    let mut blanco = false;
    let mut pila: Vec<([f64; 6], bool)> = Vec::new();
    let mut en_texto = false;
    let mut camino: Vec<Operation> = Vec::new();
    let mut caja = Limites::VACIO;
    let mut subtrazos = 0usize;
    let mut recorte: Option<Operation> = None;
    let mut salida: Vec<Operation> = Vec::new();
    let mut pinta = false;
    let mut reemplazos: Vec<(Vec<u8>, ObjectId)> = Vec::new();

    for op in contenido.operations {
        let nombre = op.operator.as_str();
        if en_texto {
            en_texto = nombre != "ET";
            continue;
        }
        let v = numeros(&op.operands);
        match nombre {
            "BT" => en_texto = true,
            "q" => {
                pila.push((ctm, blanco));
                salida.push(op);
            }
            "Q" => {
                (ctm, blanco) = pila.pop().unwrap_or((ctm, blanco));
                salida.push(op);
            }
            "cm" if v.len() == 6 => {
                ctm = multiplicar(&[v[0], v[1], v[2], v[3], v[4], v[5]], &ctm);
                salida.push(op);
            }
            "m" | "l" if v.len() == 2 => {
                if nombre == "m" {
                    subtrazos += 1;
                }
                caja.sumar(&ctm, v[0], v[1]);
                camino.push(op);
            }
            "c" | "v" | "y" if v.len() >= 4 => {
                for par in v.chunks(2) {
                    caja.sumar(&ctm, par[0], par[1]);
                }
                camino.push(op);
            }
            "re" if v.len() == 4 => {
                subtrazos += 1;
                for (x, y) in [(v[0], v[1]), (v[0] + v[2], v[1]), (v[0], v[1] + v[3]), (v[0] + v[2], v[1] + v[3])] {
                    caja.sumar(&ctm, x, y);
                }
                camino.push(op);
            }
            "h" => camino.push(op),
            "W" | "W*" => recorte = Some(op),
            "n" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" | "S" | "s" => {
                let relleno = !matches!(nombre, "n" | "S" | "s");
                let con_recorte = recorte.take();
                let rellenar = relleno && !blanco && subtrazos <= SUBTRAZOS_MAXIMOS && caja.grande(r);
                let recortar = con_recorte.is_some() && !caja.pagina_entera(r);
                if rellenar || recortar {
                    salida.append(&mut camino);
                    if recortar && let Some(w) = con_recorte {
                        salida.push(w);
                    }
                    let pintura = match nombre {
                        _ if !rellenar => "n",
                        "f*" | "B*" => "f*",
                        "b" => {
                            salida.push(Operation::new("h", vec![]));
                            "f"
                        }
                        "b*" => {
                            salida.push(Operation::new("h", vec![]));
                            "f*"
                        }
                        _ => "f",
                    };
                    salida.push(Operation::new(pintura, vec![]));
                    pinta |= rellenar;
                }
                camino.clear();
                caja = Limites::VACIO;
                subtrazos = 0;
            }
            "g" => {
                blanco = v.first().is_some_and(|x| *x >= 0.999);
                salida.push(op);
            }
            "rg" => {
                blanco = v.len() == 3 && v.iter().all(|x| *x >= 0.999);
                salida.push(op);
            }
            "k" => {
                blanco = v.len() == 4 && v.iter().all(|x| *x <= 0.001);
                salida.push(op);
            }
            "sc" | "scn" | "cs" => {
                blanco = false;
                salida.push(op);
            }
            "G" | "RG" | "K" | "SC" | "SCN" | "CS" | "gs" | "w" | "J" | "j" | "M" | "d" | "ri" | "i" => {
                salida.push(op);
            }
            "sh" => {
                pinta = true;
                salida.push(op);
            }
            "Do" => {
                let Some(clave) = op.operands.first().and_then(|o| o.as_name().ok()).map(<[u8]>::to_vec) else {
                    continue;
                };
                let Some(objeto) = xobjetos.as_ref().and_then(|x| x.get(&clave).ok()).cloned() else { continue };
                let Object::Stream(s) = resolver(doc, &objeto).clone() else { continue };
                match s.dict.get(b"Subtype").ok().and_then(|o| o.as_name().ok()) {
                    Some(b"Image") => {
                        let mut b = Limites::VACIO;
                        for (x, y) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
                            b.sumar(&ctm, x, y);
                        }
                        if b.grande(r) {
                            salida.push(op);
                            pinta = true;
                        }
                    }
                    Some(b"Form") => {
                        let datos = s.decompressed_content().unwrap_or_else(|_| s.content.clone());
                        let propios =
                            s.dict.get(b"Resources").ok().and_then(|o| resolver(doc, o).as_dict().ok().cloned());
                        let recursos_forma = propios.clone().or_else(|| recursos.cloned());
                        let m = multiplicar(&matriz(s.dict.get(b"Matrix").ok(), doc), &ctm);
                        let Some((nuevo, nuevos_recursos)) =
                            filtrar(doc, &datos, recursos_forma.as_ref(), m, r, profundidad + 1)
                        else {
                            continue;
                        };
                        let mut dict = s.dict.clone();
                        dict.remove(b"Filter");
                        dict.remove(b"DecodeParms");
                        dict.remove(b"Length");
                        if let Some(nr) = nuevos_recursos.or(propios.or_else(|| recursos.cloned())) {
                            dict.set("Resources", nr);
                        }
                        // La BBox de una forma del tamaño de la página es el recorte de la
                        // mesa de trabajo: se abre para que el fondo siga más allá.
                        let caja_forma = dict
                            .get(b"BBox")
                            .ok()
                            .and_then(|o| resolver(doc, o).as_array().ok())
                            .map(|a| numeros(a))
                            .filter(|v| v.len() == 4);
                        if let Some(bb) = caja_forma {
                            let mut lim = Limites::VACIO;
                            for (x, y) in [(bb[0], bb[1]), (bb[2], bb[1]), (bb[0], bb[3]), (bb[2], bb[3])] {
                                lim.sumar(&m, x, y);
                            }
                            if lim.pagina_entera(r) {
                                let (sx, sy) = ((bb[2] - bb[0]).abs(), (bb[3] - bb[1]).abs());
                                let (ax, ay) = (sx.max(sy), sx.max(sy));
                                dict.set(
                                    "BBox",
                                    vec![
                                        Object::Real((bb[0].min(bb[2]) - ax) as f32),
                                        Object::Real((bb[1].min(bb[3]) - ay) as f32),
                                        Object::Real((bb[0].max(bb[2]) + ax) as f32),
                                        Object::Real((bb[1].max(bb[3]) + ay) as f32),
                                    ],
                                );
                            }
                        }
                        let nuevo_id = doc.add_object(Stream::new(dict, nuevo));
                        reemplazos.push((clave, nuevo_id));
                        salida.push(op);
                        pinta = true;
                    }
                    _ => {}
                }
            }
            // Textos sueltos, contenido marcado, imágenes en línea, compatibilidad: fuera.
            _ => {}
        }
    }
    if !pinta {
        return None;
    }
    let bytes = Content { operations: salida }.encode().ok()?;
    let nuevos = (!reemplazos.is_empty()).then(|| {
        let mut d = recursos.cloned().unwrap_or_default();
        let mut x = xobjetos.unwrap_or_default();
        for (clave, id) in reemplazos {
            x.set(clave, Object::Reference(id));
        }
        d.set("XObject", x);
        d
    });
    Some((bytes, nuevos))
}

/// PDF con solo la capa de fondo de cada página, en una hoja que la deja ver
/// hasta [`MARGEN_FONDO`] fuera de la MediaBox (sin girar). Sirve para que la
/// vista previa dibuje el rebase extendido igual que el motor. Devuelve
/// también qué páginas tienen fondo.
pub fn documento_fondos(bytes: &[u8]) -> Resultado<(Vec<u8>, Vec<bool>)> {
    let Fuente { mut doc, paginas } = Fuente::desde_bytes(bytes)?;
    let mut hay = Vec::with_capacity(paginas.len());
    for p in &paginas {
        let forma = forma_fondo(&mut doc, p);
        hay.push(forma.is_some());
        let contenido = if forma.is_some() { b"q /F Do Q".to_vec() } else { Vec::new() };
        let flujo = doc.add_object(Stream::new(Dictionary::new(), contenido));
        let mut xobjetos = Dictionary::new();
        if let Some(id) = forma {
            xobjetos.set("F", Object::Reference(id));
        }
        let Caja { x0, y0, x1, y1 } = p.media;
        let d = doc.get_dictionary_mut(p.id)?;
        for clave in [&b"CropBox"[..], b"TrimBox", b"BleedBox", b"ArtBox", b"Rotate", b"Annots", b"Group"] {
            d.remove(clave);
        }
        let hoja: Vec<Object> = [x0 - MARGEN_FONDO, y0 - MARGEN_FONDO, x1 + MARGEN_FONDO, y1 + MARGEN_FONDO]
            .into_iter()
            .map(|v| Object::Real(v as f32))
            .collect();
        // Explícitos en la página: pueden venir heredados del árbol.
        d.set("MediaBox", hoja.clone());
        d.set("CropBox", hoja);
        d.set("Rotate", 0);
        d.set("Contents", Object::Reference(flujo));
        d.set("Resources", dictionary! { "XObject" => xobjetos });
    }
    let mut salida = Vec::new();
    doc.save_to(&mut salida)?;
    Ok((salida, hay))
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn documento(contenido: &str) -> Document {
        let mut doc = Document::with_version("1.7");
        let paginas = doc.new_object_id();
        let flujo = doc.add_object(Stream::new(Dictionary::new(), contenido.as_bytes().to_vec()));
        let pagina = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => paginas,
            "MediaBox" => vec![0.into(), 0.into(), 300.into(), 400.into()],
            "Contents" => flujo,
            "Resources" => Dictionary::new(),
        });
        doc.objects.insert(
            paginas,
            Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![pagina.into()], "Count" => 1 }),
        );
        let catalogo = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => paginas });
        doc.trailer.set("Root", catalogo);
        doc
    }

    #[test]
    fn deja_solo_el_fondo() {
        let doc = documento(concat!(
            "q 0 0 300 400 re W n 0.2 0.4 0.6 rg 0 0 300 400 re f Q\n", // fondo, con recorte de mesa
            "1 1 1 rg 20 20 260 360 re f\n",                            // relleno blanco
            "BT /F1 12 Tf 10 10 Td (Hola) Tj ET\n",                     // texto
            "0 0 0 rg 280 100 30 30 re f\n",                            // QR que cruza el corte
            "0.9 0.1 0.1 rg 0 0 300 40 re f\n",                         // franja que sangra
            "0 0 0 RG 0 0 m 300 400 l S\n",                             // trazo
        ));
        let Fuente { mut doc, paginas } = Fuente::desde_documento(doc).unwrap();
        let id = forma_fondo(&mut doc, &paginas[0]).expect("hay fondo");
        let Ok(Object::Stream(s)) = doc.get_object(id) else { panic!() };
        let texto = String::from_utf8_lossy(&s.content).to_string();
        assert!(texto.contains("0.2 0.4 0.6 rg"));
        assert!(texto.contains("0 0 300 40 re"));
        assert!(!texto.contains(" W"), "el recorte de la mesa de trabajo se quita: {texto}");
        assert!(!texto.contains("Tj") && !texto.contains("280 100 30 30") && !texto.contains("20 20 260 360"));
        assert!(!texto.contains(" S"));
    }

    #[test]
    fn sin_fondo() {
        let doc = documento("BT /F1 12 Tf 10 10 Td (Hola) Tj ET 0 0 0 rg 10 10 20 20 re f");
        let Fuente { mut doc, paginas } = Fuente::desde_documento(doc).unwrap();
        assert!(forma_fondo(&mut doc, &paginas[0]).is_none());
    }
}
