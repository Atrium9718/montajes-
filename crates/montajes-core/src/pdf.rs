//! Lectura del PDF del cliente y escritura del PDF impuesto.
//!
//! Las páginas originales nunca se rasterizan: cada una se convierte en un
//! *Form XObject* y se dibuja en el pliego con una matriz de transformación,
//! así la salida conserva vectores, fuentes, transparencias y perfiles.

use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use lopdf::{Dictionary, Document, Object, ObjectId, Stream, StringFormat, dictionary};

use crate::catalogo::VersionPdfx;
use crate::codigo_barras::CodigoBarras;
use crate::correcciones::{
    Conteo, Correcciones, FormaPendiente, ModoRebase, corregir_flujo, recursos_con_sobreimpresion,
};
use crate::geometria::{Rect, Tamano};
use crate::imposicion::Cara;
use crate::imposicion::marcas::Marcas;
use crate::portada::{Portada, TipoPanel};
use crate::unidades::{mm_a_pt, pt_a_mm};
use crate::{Error, Resultado};

/// Conversión simple RGB → CMYK (con generación de negro) para el color del
/// rebase de fondo.
fn rgb_a_cmyk(r: f64, g: f64, b: f64) -> (f64, f64, f64, f64) {
    let k = 1.0 - r.max(g).max(b);
    if k >= 0.999 {
        return (0.0, 0.0, 0.0, 1.0);
    }
    ((1.0 - r - k) / (1.0 - k), (1.0 - g - k) / (1.0 - k), (1.0 - b - k) / (1.0 - k), k)
}

/// Solape (mm) de las franjas de rebase generado bajo la página.
const SOLAPE: f64 = 0.2;
/// Cuánto (mm) se deja pasar la página más allá del corte en los lados con
/// rebase generado. Los visores en pantalla suavizan el borde del recorte y,
/// si la página trae un fondo blanco debajo, dejan una línea clara de un
/// píxel justo en el borde; así esa línea cae en el rebase y no sobre la imagen.
const PASE: f64 = 0.25;
/// Orilla de la página (mm) que se estira para generar el rebase.
const ORILLA: f64 = 0.3;

/// Límite de descompresión por página (protege de PDFs maliciosos).
pub(crate) const LIMITE_CONTENIDO: usize = 512 * 1024 * 1024;

/// Caja PDF normalizada en puntos.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Caja {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl Caja {
    pub fn ancho(&self) -> f64 {
        self.x1 - self.x0
    }

    pub fn alto(&self) -> f64 {
        self.y1 - self.y0
    }

    fn interseccion(&self, otra: &Caja) -> Caja {
        Caja { x0: self.x0.max(otra.x0), y0: self.y0.max(otra.y0), x1: self.x1.min(otra.x1), y1: self.y1.min(otra.y1) }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PaginaFuente {
    pub id: ObjectId,
    pub media: Caja,
    /// CropBox (lo que muestra un visor); por defecto la MediaBox.
    pub vista: Caja,
    /// TrimBox (o CropBox/MediaBox si no está definida).
    pub corte: Caja,
    /// Hasta dónde llega el contenido utilizable como rebase.
    pub sangrado: Caja,
    /// /Rotate normalizado a 0, 90, 180 o 270.
    pub giro: u16,
    /// Si la página declaraba TrimBox explícita.
    pub tiene_trimbox: bool,
}

impl PaginaFuente {
    /// Formato final tal como se ve la página (con /Rotate aplicado), en mm.
    pub fn tamano_corte(&self) -> Tamano {
        let t = Tamano::new(pt_a_mm(self.corte.ancho()), pt_a_mm(self.corte.alto())).redondeado();
        if self.giro % 180 == 90 { t.girado() } else { t }
    }

    /// Rebase disponible (el menor de los cuatro lados), en mm.
    pub fn rebase_disponible(&self) -> f64 {
        let c = &self.corte;
        let s = &self.sangrado;
        pt_a_mm((c.x0 - s.x0).min(c.y0 - s.y0).min(s.x1 - c.x1).min(s.y1 - c.y1).max(0.0))
    }
}

/// PDF de entrada ya analizado.
pub struct Fuente {
    pub(crate) doc: Document,
    pub paginas: Vec<PaginaFuente>,
}

impl Fuente {
    pub fn abrir(ruta: &Path) -> Resultado<Self> {
        Self::desde_documento(Document::load(ruta)?)
    }

    pub fn desde_bytes(bytes: &[u8]) -> Resultado<Self> {
        Self::desde_documento(Document::load_mem(bytes)?)
    }

    pub fn desde_documento(doc: Document) -> Resultado<Self> {
        if doc.is_encrypted() {
            return Err(Error::Invalido("el PDF está protegido con contraseña".into()));
        }
        let mut paginas = Vec::new();
        for (_, id) in doc.get_pages() {
            let media = heredado(&doc, id, b"MediaBox")
                .and_then(|o| caja(&doc, &o))
                .ok_or_else(|| Error::Invalido("página sin MediaBox".into()))?;
            let recorte =
                heredado(&doc, id, b"CropBox").and_then(|o| caja(&doc, &o)).map_or(media, |c| c.interseccion(&media));
            let propia = |clave: &[u8]| {
                doc.get_dictionary(id).ok()?.get(clave).ok().and_then(|o| caja(&doc, o)).map(|c| c.interseccion(&media))
            };
            let trim = propia(b"TrimBox");
            let corte = trim.or_else(|| propia(b"ArtBox")).unwrap_or(recorte);
            let sangrado = propia(b"BleedBox").unwrap_or(recorte);
            let giro = heredado(&doc, id, b"Rotate")
                .and_then(|o| resolver(&doc, &o).as_i64().ok())
                .map_or(0, |r| r.rem_euclid(360) as u16 / 90 * 90);
            paginas.push(PaginaFuente {
                id,
                media,
                vista: recorte,
                corte,
                sangrado,
                giro,
                tiene_trimbox: trim.is_some(),
            });
        }
        if paginas.is_empty() {
            return Err(Error::Invalido("el PDF no tiene páginas".into()));
        }
        Ok(Self { doc, paginas })
    }
}

pub(crate) fn resolver<'a>(doc: &'a Document, o: &'a Object) -> &'a Object {
    match o {
        Object::Reference(id) => doc.get_object(*id).unwrap_or(o),
        _ => o,
    }
}

impl Fuente {
    /// Junta varios PDF (p. ej. uno por cliente) en una sola fuente, en orden.
    pub fn unir(bytes: &[&[u8]]) -> Resultado<Self> {
        let docs = bytes.iter().map(|b| Document::load_mem(b)).collect::<Result<Vec<_>, _>>()?;
        Self::desde_documento(unir_documentos(docs)?)
    }

    /// Documento original (solo lectura), para revisiones como el preflight.
    pub fn documento(&self) -> &Document {
        &self.doc
    }

    /// Como [`Fuente::formato_comun`], pero solo con las páginas indicadas
    /// (p. ej. la tripa, cuando el PDF también trae la carátula).
    pub fn formato_de(
        &self,
        indices: &[usize],
        formato: Option<Tamano>,
        rebase: f64,
    ) -> Resultado<(Tamano, Vec<String>)> {
        let primera = indices.first().ok_or_else(|| Error::Invalido("no hay páginas seleccionadas".into()))?;
        let pagina =
            |i: usize| self.paginas.get(i).ok_or_else(|| Error::Invalido(format!("el PDF no tiene página {}", i + 1)));
        let formato = match formato {
            Some(f) => f,
            None => pagina(*primera)?.tamano_corte(),
        };
        let mut avisos = Vec::new();
        for &i in indices {
            let p = pagina(i)?;
            let t = p.tamano_corte();
            if (t.ancho - formato.ancho).abs() > 0.5 || (t.alto - formato.alto).abs() > 0.5 {
                return Err(Error::Invalido(format!(
                    "la página {} mide {t} y el formato es {formato} (¿es de la carátula?)",
                    i + 1
                )));
            }
            if p.rebase_disponible() + 0.05 < rebase {
                avisos.push(format!(
                    "página {}: tiene {:.1} mm de rebase y se pidieron {rebase} mm",
                    i + 1,
                    p.rebase_disponible()
                ));
            }
        }
        Ok((formato, avisos))
    }

    /// Formato común de todas las páginas (o el pedido) y avisos de rebase.
    /// Falla si alguna página mide distinto.
    pub fn formato_comun(&self, formato: Option<Tamano>, rebase: f64) -> Resultado<(Tamano, Vec<String>)> {
        let formato = formato.unwrap_or_else(|| self.paginas[0].tamano_corte());
        let mut avisos = Vec::new();
        for (i, p) in self.paginas.iter().enumerate() {
            let t = p.tamano_corte();
            if (t.ancho - formato.ancho).abs() > 0.5 || (t.alto - formato.alto).abs() > 0.5 {
                return Err(Error::Invalido(format!(
                    "la página {} mide {t} y el formato es {formato} (¿falta TrimBox o el giro?)",
                    i + 1
                )));
            }
            if p.rebase_disponible() + 0.05 < rebase {
                avisos.push(format!(
                    "página {}: tiene {:.1} mm de rebase y se pidieron {rebase} mm",
                    i + 1,
                    p.rebase_disponible()
                ));
            }
        }
        Ok((formato, avisos))
    }
}

/// Declara TrimBox y BleedBox en un PDF cuyas páginas se compusieron con el
/// rebase incluido en la MediaBox (el interior que sale de la diagramación):
/// el corte queda `rebase_mm` hacia adentro de cada borde.
pub fn fijar_cajas(bytes: &[u8], rebase_mm: f64) -> Resultado<Vec<u8>> {
    let mut doc = Document::load_mem(bytes)?;
    if doc.is_encrypted() {
        return Err(Error::Invalido("el PDF está protegido con contraseña".into()));
    }
    let r = mm_a_pt(rebase_mm.max(0.0));
    let numero = |v: f64| Object::Real(v as f32);
    let arreglo = |c: Caja| Object::Array(vec![numero(c.x0), numero(c.y0), numero(c.x1), numero(c.y1)]);
    for (n, id) in doc.get_pages() {
        let media = heredado(&doc, id, b"MediaBox")
            .and_then(|o| caja(&doc, &o))
            .ok_or_else(|| Error::Invalido("página sin MediaBox".into()))?;
        if media.ancho() <= 2.0 * r || media.alto() <= 2.0 * r {
            return Err(Error::Invalido(format!("la página {n} es más pequeña que el rebase")));
        }
        let corte = Caja { x0: media.x0 + r, y0: media.y0 + r, x1: media.x1 - r, y1: media.y1 - r };
        let pagina = doc.get_dictionary_mut(id)?;
        pagina.set("MediaBox", arreglo(media));
        pagina.set("TrimBox", arreglo(corte));
        pagina.set("BleedBox", arreglo(media));
        pagina.remove(b"CropBox");
    }
    let mut salida = Vec::new();
    doc.save_to(&mut salida)?;
    Ok(salida)
}

/// Une un plegable que viene por cuerpos (una página por cuerpo) en piezas
/// abiertas: cada `cuerpos` páginas seguidas forman una pieza, de izquierda a
/// derecha (primero el exterior, luego el interior). Los cuerpos se juntan
/// por el corte; el rebase queda solo en los bordes de afuera.
pub fn unir_cuerpos(bytes: &[u8], cuerpos: usize) -> Resultado<Vec<u8>> {
    let Fuente { mut doc, paginas } = Fuente::desde_bytes(bytes)?;
    if cuerpos < 2 || paginas.is_empty() || !paginas.len().is_multiple_of(cuerpos) {
        return Err(Error::Invalido(format!(
            "el PDF tiene {} páginas: para {cuerpos} cuerpos debe tener un múltiplo de {cuerpos} (exterior e interior)",
            paginas.len()
        )));
    }
    if let Some(i) = paginas.iter().position(|p| p.giro != 0) {
        return Err(Error::Invalido(format!("la página {} está girada; no se puede unir por cuerpos", i + 1)));
    }
    let raiz = doc.catalog()?.get(b"Pages")?.as_reference()?;
    let mut conteo = Conteo::default();
    let mut pendientes = Vec::new();
    let mut nuevas = Vec::new();
    for grupo in paginas.chunks(cuerpos) {
        let alto = grupo[0].corte.alto();
        if grupo.iter().any(|p| (p.corte.alto() - alto).abs() > 1.0) {
            return Err(Error::Invalido("los cuerpos de un plegable deben tener la misma altura".into()));
        }
        let r = grupo.iter().map(PaginaFuente::rebase_disponible).fold(f64::INFINITY, f64::min);
        let r = mm_a_pt(r);
        let ancho: f64 = grupo.iter().map(|p| p.corte.ancho()).sum();
        let mut xobjetos = Dictionary::new();
        let mut contenido = String::new();
        let mut x = r;
        for (i, p) in grupo.iter().enumerate() {
            let forma = crear_forma(&mut doc, p, &Correcciones::ninguna(), &mut conteo, &mut pendientes)?;
            let nombre = format!("C{}", i + 1);
            xobjetos.set(nombre.as_bytes(), Object::Reference(forma));
            let izq = if i == 0 { r } else { 0.0 };
            let der = if i + 1 == grupo.len() { r } else { 0.0 };
            let w = p.corte.ancho();
            contenido += &format!(
                "q {} 0 {} {} re W n 1 0 0 1 {} {} cm /{nombre} Do Q\n",
                n(x - izq),
                n(w + izq + der),
                n(alto + 2.0 * r),
                n(x - p.corte.x0),
                n(r - p.corte.y0)
            );
            x += w;
        }
        let flujo = doc.add_object(Stream::new(Dictionary::new(), contenido.into_bytes()));
        let caja = |v: [f64; 4]| -> Object { v.iter().map(|x| Object::Real(*x as f32)).collect::<Vec<_>>().into() };
        let hoja = caja([0.0, 0.0, ancho + 2.0 * r, alto + 2.0 * r]);
        let pagina = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => raiz,
            "MediaBox" => hoja.clone(),
            "CropBox" => hoja.clone(),
            "BleedBox" => hoja,
            "TrimBox" => caja([r, r, ancho + r, alto + r]),
            "Rotate" => 0,
            "Contents" => flujo,
            "Resources" => dictionary! { "XObject" => xobjetos },
        });
        nuevas.push(Object::Reference(pagina));
    }
    let cantidad = nuevas.len() as i64;
    let arbol = doc.get_dictionary_mut(raiz)?;
    arbol.set("Kids", nuevas);
    arbol.set("Count", cantidad);
    doc.prune_objects();
    let mut salida = Vec::new();
    doc.save_to(&mut salida)?;
    Ok(salida)
}

/// Separa las páginas dobles (pliegos de lectura: dos páginas seguidas en una
/// hoja) en dos páginas sencillas, en orden: primero la izquierda. Las demás
/// páginas quedan igual. El contenido no se toca: cada mitad es la misma
/// página con sus cajas (TrimBox, BleedBox y CropBox) recortadas a su lado;
/// en el lomo, el rebase es la continuación de la página vecina.
pub fn separar_dobles(bytes: &[u8], dobles: &[usize]) -> Resultado<Vec<u8>> {
    let mut doc = Document::load_mem(bytes)?;
    if doc.is_encrypted() {
        return Err(Error::Invalido("el PDF está protegido con contraseña".into()));
    }
    let paginas: Vec<ObjectId> = doc.get_pages().into_values().collect();
    let raiz = doc.catalog()?.get(b"Pages")?.as_reference()?;
    let arreglo = |c: Caja| Object::Array(vec![c.x0.into(), c.y0.into(), c.x1.into(), c.y1.into()]);
    let mut nuevas = Vec::with_capacity(paginas.len() + dobles.len());
    for (i, &id) in paginas.iter().enumerate() {
        // Atributos heredados copiados a la página, para poder moverla de árbol.
        for clave in [&b"MediaBox"[..], b"CropBox", b"Rotate", b"Resources"] {
            let falta = doc.get_dictionary(id).map(|p| !p.has(clave)).unwrap_or(false);
            if falta && let Some(valor) = heredado(&doc, id, clave) {
                doc.get_dictionary_mut(id)?.set(clave.to_vec(), valor);
            }
        }
        if !dobles.contains(&i) {
            doc.get_dictionary_mut(id)?.set("Parent", raiz);
            nuevas.push(id);
            continue;
        }
        let giro = heredado(&doc, id, b"Rotate").and_then(|o| resolver(&doc, &o).as_i64().ok()).unwrap_or(0);
        if giro.rem_euclid(360) != 0 {
            return Err(Error::Invalido(format!("la página {} está girada; no se puede separar en dos", i + 1)));
        }
        let media = heredado(&doc, id, b"MediaBox")
            .and_then(|o| caja(&doc, &o))
            .ok_or_else(|| Error::Invalido("página sin MediaBox".into()))?;
        let propia = |clave: &[u8]| {
            doc.get_dictionary(id).ok()?.get(clave).ok().and_then(|o| caja(&doc, o)).map(|c| c.interseccion(&media))
        };
        let recorte = propia(b"CropBox").unwrap_or(media);
        let corte = propia(b"TrimBox").or_else(|| propia(b"ArtBox")).unwrap_or(recorte);
        let sangrado = propia(b"BleedBox").unwrap_or(recorte);
        // Rebase disponible por fuera (el menor de los cuatro lados).
        let r = [corte.x0 - sangrado.x0, corte.y0 - sangrado.y0, sangrado.x1 - corte.x1, sangrado.y1 - corte.y1]
            .into_iter()
            .fold(f64::INFINITY, f64::min)
            .max(0.0);
        let medio = (corte.x0 + corte.x1) / 2.0;
        let original = doc.get_dictionary(id)?.clone();
        for izquierda in [true, false] {
            let (c, b) = if izquierda {
                (
                    Caja { x1: medio, ..corte },
                    Caja { x0: sangrado.x0, y0: sangrado.y0, x1: (medio + r).min(media.x1), y1: sangrado.y1 },
                )
            } else {
                (
                    Caja { x0: medio, ..corte },
                    Caja { x0: (medio - r).max(media.x0), y0: sangrado.y0, x1: sangrado.x1, y1: sangrado.y1 },
                )
            };
            let mut d = original.clone();
            d.set("Parent", raiz);
            d.set("TrimBox", arreglo(c));
            d.set("BleedBox", arreglo(b));
            d.set("CropBox", arreglo(b));
            d.remove(b"ArtBox");
            nuevas.push(doc.add_object(Object::Dictionary(d)));
        }
    }
    let raiz_dic = doc.get_dictionary_mut(raiz)?;
    raiz_dic.set("Kids", Object::Array(nuevas.iter().map(|&k| Object::Reference(k)).collect()));
    raiz_dic.set("Count", nuevas.len() as i64);
    for k in [b"MediaBox".as_slice(), b"CropBox", b"Rotate", b"Resources"] {
        raiz_dic.remove(k);
    }
    doc.prune_objects();
    let mut salida = Vec::new();
    doc.save_to(&mut salida)?;
    Ok(salida)
}

/// Atributo de página heredable (MediaBox, CropBox, Rotate, Resources).
/// Une documentos en uno: renumera los objetos de cada uno, copia en cada
/// página los atributos que heredaba (cajas, giro, recursos) y arma un árbol
/// de páginas nuevo con todas, en orden.
pub fn unir_documentos(docs: Vec<Document>) -> Resultado<Document> {
    let mut base = Document::with_version("1.6");
    let mut paginas = Vec::new();
    let mut siguiente = 1;
    for mut d in docs {
        if d.is_encrypted() {
            return Err(Error::Invalido("uno de los PDF está protegido con contraseña".into()));
        }
        for (_, id) in d.get_pages() {
            for clave in [&b"MediaBox"[..], b"CropBox", b"Rotate", b"Resources"] {
                let falta = d.get_dictionary(id).map(|p| !p.has(clave)).unwrap_or(false);
                if falta && let Some(valor) = heredado(&d, id, clave) {
                    d.get_dictionary_mut(id)?.set(clave.to_vec(), valor);
                }
            }
        }
        d.renumber_objects_with(siguiente);
        siguiente = d.objects.keys().map(|k| k.0).max().unwrap_or(siguiente) + 1;
        paginas.extend(d.get_pages().into_values());
        base.objects.extend(d.objects);
    }
    base.max_id = siguiente;
    let arbol = base.new_object_id();
    for id in &paginas {
        base.get_dictionary_mut(*id)?.set("Parent", arbol);
    }
    let hijos: Vec<Object> = paginas.iter().map(|id| Object::Reference(*id)).collect();
    let cantidad = hijos.len() as i64;
    base.objects
        .insert(arbol, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => hijos, "Count" => cantidad }));
    let catalogo = base.add_object(dictionary! { "Type" => "Catalog", "Pages" => arbol });
    base.trailer.set("Root", catalogo);
    Ok(base)
}

pub(crate) fn heredado(doc: &Document, pagina: ObjectId, clave: &[u8]) -> Option<Object> {
    let mut actual = doc.get_dictionary(pagina).ok()?;
    for _ in 0..64 {
        if let Ok(valor) = actual.get(clave) {
            return Some(valor.clone());
        }
        let padre = actual.get(b"Parent").ok()?.as_reference().ok()?;
        actual = doc.get_dictionary(padre).ok()?;
    }
    None
}

fn caja(doc: &Document, o: &Object) -> Option<Caja> {
    let v = resolver(doc, o).as_array().ok()?;
    if v.len() != 4 {
        return None;
    }
    let n: Vec<f64> = v.iter().map(|x| resolver(doc, x).as_float().map(f64::from)).collect::<Result<_, _>>().ok()?;
    Some(Caja { x0: n[0].min(n[2]), y0: n[1].min(n[3]), x1: n[0].max(n[2]), y1: n[1].max(n[3]) })
}

/// Matriz `cm` que lleva la caja `b` de la página, girada `giro` grados en
/// sentido horario, a la esquina inferior izquierda `(tx, ty)` del pliego.
pub fn matriz_colocacion(b: &Caja, giro: u16, tx: f64, ty: f64) -> [f64; 6] {
    match giro % 360 {
        0 => [1.0, 0.0, 0.0, 1.0, tx - b.x0, ty - b.y0],
        90 => [0.0, -1.0, 1.0, 0.0, tx - b.y0, ty + b.x1],
        180 => [-1.0, 0.0, 0.0, -1.0, tx + b.x1, ty + b.y1],
        270 => [0.0, 1.0, -1.0, 0.0, tx + b.y1, ty - b.x0],
        otro => panic!("giro no válido: {otro}"),
    }
}

#[derive(Debug, Clone)]
pub struct OpcionesSalida {
    pub titulo: String,
    pub pdfx: VersionPdfx,
    /// Perfil ICC de salida (CMYK) para el OutputIntent.
    pub icc: Option<Vec<u8>>,
    /// Identificador de la condición, p. ej. `FOGRA39`.
    pub condicion: Option<String>,
    /// Segundos desde 1970 (UTC) para las fechas del PDF. En el navegador
    /// no hay reloj del sistema y se debe indicar; si falta, se usa el reloj.
    pub fecha: Option<u64>,
    /// Correcciones automáticas de preflight.
    pub correcciones: Correcciones,
}

#[derive(Debug, Clone, Default)]
pub struct InformeSalida {
    /// Se escribió la identificación PDF/X (hay OutputIntent).
    pub pdfx_identificado: bool,
    pub pliegos: usize,
    pub avisos: Vec<String>,
}

/// Número con pocos decimales para el flujo de contenido.
fn n(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

/// Escribe el PDF impuesto en `destino`.
pub fn escribir(fuente: Fuente, caras: &[Cara], opciones: &OpcionesSalida, destino: &Path) -> Resultado<InformeSalida> {
    let (mut doc, informe) = componer(fuente, caras, opciones)?;
    doc.save(destino)?;
    Ok(informe)
}

/// Arma el documento impuesto en memoria.
pub fn componer(fuente: Fuente, caras: &[Cara], opciones: &OpcionesSalida) -> Resultado<(Document, InformeSalida)> {
    let Fuente { mut doc, paginas } = fuente;
    let mut informe = InformeSalida { pliegos: caras.len(), ..Default::default() };
    if opciones.pdfx == VersionPdfx::X1a {
        informe.avisos.push("PDF/X-1a: aún no se aplana la transparencia; se escribe como PDF 1.6".into());
    }

    // 1. Un Form XObject por cada página de entrada que se usa.
    let mut formas: BTreeMap<usize, ObjectId> = BTreeMap::new();
    // Capa de fondo de cada página, para el rebase extendido.
    let mut formas_fondo: BTreeMap<usize, Option<ObjectId>> = BTreeMap::new();
    let mut conteo = Conteo::default();
    let mut pendientes = Vec::new();
    let mut espejos = 0;
    for cara in caras {
        for u in &cara.ubicaciones {
            if formas.contains_key(&u.pagina) {
                continue;
            }
            let p = paginas
                .get(u.pagina)
                .ok_or_else(|| Error::Invalido(format!("la página {} no existe en la entrada", u.pagina + 1)))?;
            let id = crear_forma(&mut doc, p, &opciones.correcciones, &mut conteo, &mut pendientes)?;
            formas.insert(u.pagina, id);
            if opciones.correcciones.rebase_extendido {
                let fondo = crate::fondo::forma_fondo(&mut doc, p);
                formas_fondo.insert(u.pagina, fondo);
            }
        }
    }
    corregir_formas_anidadas(&mut doc, &opciones.correcciones, &mut conteo, pendientes);

    // 2. Espacio de color de registro: separación «All» (sale en todas las planchas).
    let funcion = doc.add_object(dictionary! {
        "FunctionType" => 2,
        "Domain" => vec![0.into(), 1.into()],
        "C0" => vec![0.into(), 0.into(), 0.into(), 0.into()],
        "C1" => vec![1.into(), 1.into(), 1.into(), 1.into()],
        "N" => 1,
    });
    let registro = doc.add_object(vec![
        Object::Name(b"Separation".to_vec()),
        Object::Name(b"All".to_vec()),
        Object::Name(b"DeviceCMYK".to_vec()),
        Object::Reference(funcion),
    ]);

    // 3. Un pliego por cara.
    let arbol = doc.new_object_id();
    let mut hijos = Vec::with_capacity(caras.len());
    let corr = &opciones.correcciones;
    let lados = lados_de(caras);
    let mut gris = crate::gris::Conversor::new();
    let mut paginas_en_negro = std::collections::BTreeSet::new();
    for (cara, (etiqueta, lado)) in caras.iter().zip(&lados) {
        let mut xobjetos = Dictionary::new();
        let mut contenido = String::new();
        // A una tinta: la página (y su capa de fondo) en su copia en gris.
        let tintas = match lado {
            Lado::Tiro => corr.tintas_tiro,
            Lado::Retiro => corr.tintas_retiro,
            Lado::TiraRetira => corr.tintas_tiro.max(corr.tintas_retiro),
        };
        let negro = tintas == 1;
        for u in &cara.ubicaciones {
            let p = &paginas[u.pagina];
            let (nombre, forma) = if negro {
                paginas_en_negro.insert(u.pagina);
                (format!("PN{}", u.pagina + 1), gris.forma(&mut doc, formas[&u.pagina]))
            } else {
                (format!("P{}", u.pagina + 1), formas[&u.pagina])
            };
            xobjetos.set(nombre.as_bytes(), Object::Reference(forma));
            let giro = (p.giro + u.giro) % 360;
            let mut m = matriz_colocacion(&p.corte, giro, mm_a_pt(u.corte.x), mm_a_pt(u.corte.y));
            let s = if u.escala > 0.0 { u.escala } else { 1.0 };
            if (s - 1.0).abs() > 1e-9 {
                // Arte reducido o ampliado: ya colocado, se escala alrededor de la
                // esquina de corte en el pliego (vale para cualquier giro).
                let (tx, ty) = (mm_a_pt(u.corte.x), mm_a_pt(u.corte.y));
                m = multiplicar(&m, &[s, 0.0, 0.0, s, tx * (1.0 - s), ty * (1.0 - s)]);
            }
            let (c, r) = (u.corte, u.recorte);
            // Cuánto rebase pide cada lado: izquierda, abajo, derecha, arriba.
            let pide = [c.x - r.x, c.y - r.y, r.derecha() - c.derecha(), r.arriba() - c.arriba()];
            let corr = &opciones.correcciones;
            // «Solo fondo» nunca usa lo que el PDF trae fuera del corte: si no hay
            // color medido para la página, se estira la orilla en todos los lados.
            let tiene = if corr.rebase_fondo { 0.0 } else { p.rebase_disponible() * s };
            let falta = pide.map(|v| v > tiene + 0.01);
            let estirado = corr.rebase_estirado || corr.rebase_fondo;
            let fondo = if corr.rebase_fondo { corr.fondos.get(u.pagina).copied().flatten() } else { None };
            // Lados del pliego (izquierda, abajo, derecha, arriba) → lado de la
            // página, que va girada `giro` grados.
            let lado_pagina = |lado_pliego: usize| {
                let ciclo = [0usize, 3, 2, 1]; // horario: izquierda, arriba, derecha, abajo
                let pos = ciclo.iter().position(|l| *l == lado_pliego).unwrap_or(0);
                ciclo[(pos + 4 - (giro / 90) as usize % 4) % 4]
            };
            let fondo_pagina = formas_fondo.get(&u.pagina).copied().flatten();
            if let Some(fid) = fondo_pagina.filter(|_| pide.iter().any(|v| *v > 0.01)) {
                // Rebase con la capa de fondo: la foto o el fondo siguen más allá del corte.
                espejos += 1;
                let (nombre_fondo, fid) = if negro {
                    (format!("FN{}", u.pagina + 1), gris.forma(&mut doc, fid))
                } else {
                    (format!("F{}", u.pagina + 1), fid)
                };
                xobjetos.set(nombre_fondo.as_bytes(), Object::Reference(fid));
                colocar_extendido(&mut contenido, c, r, pide, &m, &nombre, &nombre_fondo);
            } else if let Some(modos) =
                corr.modos.get(u.pagina).copied().flatten().filter(|_| pide.iter().any(|v| *v > 0.01))
            {
                // Rebase decidido por borde (lo elige la app mirando la página).
                espejos += 1;
                let colores = corr.fondos.get(u.pagina).copied().flatten();
                let lados: [usize; 4] = std::array::from_fn(lado_pagina);
                let modo = lados.map(|l| modos[l]);
                let color = lados.map(|l| colores.map(|c| c[l]));
                let tiene = p.rebase_disponible() * s;
                colocar_por_lados(&mut contenido, c, r, pide, tiene, modo, color, &m, &nombre, opciones.icc.is_some());
            } else if let Some(colores) = fondo {
                // Rebase solo con el fondo: franjas del color medido junto a cada
                // borde (girado como la página) y la página recortada al corte.
                espejos += 1;
                let color_lado = |lado_pliego: usize| {
                    // Lados en sentido horario: izquierda, arriba, derecha, abajo.
                    let ciclo = [0usize, 3, 2, 1];
                    let pos = ciclo.iter().position(|l| *l == lado_pliego).unwrap_or(0);
                    let pasos = (giro / 90) as usize % 4;
                    colores[ciclo[(pos + 4 - pasos) % 4]]
                };
                let [pi, pb, pd, pa] = pide;
                // La página primero, pasada un poco del corte; las franjas de color
                // van encima, desde el borde de corte hacia afuera (tapan ese pase).
                let pase = pide.map(|v| v.min(PASE));
                colocar(&mut contenido, c.expandir(pase[0], pase[1], pase[2], pase[3]), &m, &nombre);
                let bandas = [
                    (0, Rect::new(r.x, r.y, pi, r.alto)),
                    (2, Rect::new(c.derecha(), r.y, pd, r.alto)),
                    (1, Rect::new(c.x, r.y, c.ancho, pb)),
                    (3, Rect::new(c.x, c.arriba(), c.ancho, pa)),
                ];
                for (lado, zona) in bandas {
                    if zona.ancho > 1e-6 && zona.alto > 1e-6 && pide[lado] > 0.01 {
                        let [rr, gg, bb] = color_lado(lado);
                        let (rr, gg, bb) = (f64::from(rr), f64::from(gg), f64::from(bb));
                        // Con perfil de salida (PDF/X) el color va en CMYK; si no, en RGB,
                        // tal como se midió, para que coincida con el fondo.
                        let color = if opciones.icc.is_some() {
                            let (cc, mm_, yy, kk) = rgb_a_cmyk(rr, gg, bb);
                            format!("{} {} {} {} k", n(cc), n(mm_), n(yy), n(kk))
                        } else {
                            format!("{} {} {} rg", n(rr), n(gg), n(bb))
                        };
                        contenido += &format!(
                            "q {color} {} {} {} {} re f Q\n",
                            n(mm_a_pt(zona.x)),
                            n(mm_a_pt(zona.y)),
                            n(mm_a_pt(zona.ancho)),
                            n(mm_a_pt(zona.alto))
                        );
                    }
                }
            } else if (corr.rebase_espejo || estirado) && falta.iter().any(|f| *f) {
                // Rebase generado sobre cada borde de corte que no tiene rebase
                // suficiente (y sobre las esquinas): estirando la orilla de la
                // página (solo sigue el fondo) o reflejándola en espejo.
                espejos += 1;
                let (x0, y0, x1, y1) = (mm_a_pt(c.x), mm_a_pt(c.y), mm_a_pt(c.derecha()), mm_a_pt(c.arriba()));
                let [pi, pb, pd, pa] = pide;
                // Orilla que se estira: 0,3 mm de la página hacia adentro del corte.
                let e = mm_a_pt(ORILLA);
                let estirar = |desde: f64, hacia_afuera: f64, signo: f64| {
                    // Lleva la franja [desde, desde + signo·e] a [desde − signo·b, desde + signo·e].
                    let fijo = desde + signo * e;
                    let k = (e + mm_a_pt(hacia_afuera)) / e;
                    (k, fijo * (1.0 - k))
                };
                let tx = |a: f64, b: f64, signo: f64| {
                    if estirado {
                        let (k, t) = estirar(a, b, signo);
                        [k, 0.0, 0.0, 1.0, t, 0.0]
                    } else {
                        [-1.0, 0.0, 0.0, 1.0, 2.0 * a, 0.0]
                    }
                };
                let ty = |a: f64, b: f64, signo: f64| {
                    if estirado {
                        let (k, t) = estirar(a, b, signo);
                        [1.0, 0.0, 0.0, k, 0.0, t]
                    } else {
                        [1.0, 0.0, 0.0, -1.0, 0.0, 2.0 * a]
                    }
                };
                let [fi, fb, fd, fa] = falta;
                let (izq, der) = (tx(x0, pi, 1.0), tx(x1, pd, -1.0));
                let (abajo, arriba) = (ty(y0, pb, 1.0), ty(y1, pa, -1.0));
                let franjas: [(bool, Rect, Vec<[f64; 6]>); 8] = [
                    // Cada franja se mete 0,2 mm bajo la página (que va encima) para
                    // que no quede un filo blanco por el antialias del visor o del RIP.
                    (fi, Rect::new(r.x, c.y, pi + SOLAPE, c.alto), vec![izq]),
                    (fd, Rect::new(c.derecha() - SOLAPE, c.y, pd + SOLAPE, c.alto), vec![der]),
                    (fb, Rect::new(c.x, r.y, c.ancho, pb + SOLAPE), vec![abajo]),
                    (fa, Rect::new(c.x, c.arriba() - SOLAPE, c.ancho, pa + SOLAPE), vec![arriba]),
                    (fi && fb, Rect::new(r.x, r.y, pi, pb), vec![izq, abajo]),
                    (fd && fb, Rect::new(c.derecha(), r.y, pd, pb), vec![der, abajo]),
                    (fi && fa, Rect::new(r.x, c.arriba(), pi, pa), vec![izq, arriba]),
                    (fd && fa, Rect::new(c.derecha(), c.arriba(), pd, pa), vec![der, arriba]),
                ];
                for (aplica, zona, reflejos) in franjas {
                    if aplica && zona.ancho > 0.0 && zona.alto > 0.0 {
                        let mt = reflejos.iter().fold(m, |acc, refl| multiplicar(&acc, refl));
                        colocar(&mut contenido, zona, &mt, &nombre);
                    }
                }
                // La página encima, con el rebase real que sí trae.
                let propio = pide.map(|v| v.min(tiene.max(PASE)));
                colocar(&mut contenido, c.expandir(propio[0], propio[1], propio[2], propio[3]), &m, &nombre);
            } else {
                colocar(&mut contenido, r, &m, &nombre);
            }
        }
        dibujar_marcas(&cara.marcas, &mut contenido, tintas);
        if let Some(r) = cara.marcas.rotulo {
            dibujar_rotulo(&mut contenido, &r, tintas, &opciones.titulo, etiqueta);
        }
        informe.avisos.extend(cara.marcas.avisos.iter().map(|a| format!("{}: {a}", cara.nombre)));

        let flujo = doc.add_object(Stream::new(Dictionary::new(), contenido.into_bytes()));
        let caja = |r: Rect| -> Object {
            vec![mm_a_pt(r.x).into(), mm_a_pt(r.y).into(), mm_a_pt(r.derecha()).into(), mm_a_pt(r.arriba()).into()]
                .into()
        };
        let todo = Rect::new(0.0, 0.0, cara.pliego.ancho, cara.pliego.alto);
        let (corte, sangrado) = cara.cajas.map_or((todo, todo), |c| (c.corte, c.sangrado));
        let pagina = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => arbol,
            "MediaBox" => caja(todo),
            "TrimBox" => caja(corte),
            "BleedBox" => caja(sangrado),
            "Contents" => flujo,
            "Resources" => dictionary! {
                "XObject" => xobjetos,
                "ColorSpace" => dictionary! { "Registro" => registro },
            },
        });
        hijos.push(Object::Reference(pagina));
    }
    informe.avisos.extend(conteo.avisos());
    if !paginas_en_negro.is_empty() {
        informe.avisos.push(format!(
            "corregido: {} {} a una tinta (todo en negro)",
            paginas_en_negro.len(),
            if paginas_en_negro.len() == 1 { "página pasada" } else { "páginas pasadas" }
        ));
        informe.avisos.extend(gris.avisos.iter().map(|a| format!("a una tinta: {a}")));
    }
    if espejos > 0 {
        informe.avisos.push(format!(
            "corregido: rebase generado {} en {espejos} ubicaciones sin rebase suficiente",
            if opciones.correcciones.rebase_extendido {
                "con la capa de fondo de la página"
            } else if opciones.correcciones.rebase_fondo {
                "solo con el fondo"
            } else if opciones.correcciones.rebase_estirado {
                "estirando el fondo de la orilla"
            } else {
                "en espejo"
            }
        ));
    }
    let cantidad = hijos.len() as i64;
    doc.objects
        .insert(arbol, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => hijos, "Count" => cantidad }));

    // 4. Catálogo nuevo: solo lo que sigue siendo válido tras la imposición.
    let catalogo_viejo = doc.catalog()?.clone();
    let mut catalogo = dictionary! { "Type" => "Catalog", "Pages" => arbol };
    let intencion_previa = catalogo_viejo.get(b"OutputIntents").ok().cloned();
    if let Some(icc) = &opciones.icc {
        let condicion = opciones.condicion.clone().unwrap_or_else(|| "Custom".into());
        let perfil = doc.add_object(Stream::new(dictionary! { "N" => 4 }, icc.clone()));
        let intencion = doc.add_object(dictionary! {
            "Type" => "OutputIntent",
            "S" => "GTS_PDFX",
            "OutputConditionIdentifier" => Object::string_literal(condicion.clone()),
            "Info" => Object::string_literal(condicion),
            "DestOutputProfile" => perfil,
        });
        catalogo.set("OutputIntents", vec![Object::Reference(intencion)]);
        informe.pdfx_identificado = true;
    } else if let Some(previa) = intencion_previa {
        catalogo.set("OutputIntents", previa);
        informe.pdfx_identificado = true;
        informe.avisos.push("se conservó el OutputIntent del PDF de entrada".into());
    } else {
        informe.avisos.push("sin perfil ICC de salida: el PDF no se identifica como PDF/X".into());
    }
    if let Ok(oc) = catalogo_viejo.get(b"OCProperties") {
        catalogo.set("OCProperties", oc.clone());
    }

    let ahora = Fecha::desde(opciones.fecha.unwrap_or_else(segundos_actuales));
    let version_pdfx = match opciones.pdfx {
        VersionPdfx::X4 => "PDF/X-4",
        VersionPdfx::X1a => "PDF/X-1a:2003",
    };
    let id_documento = huella(&(opciones.titulo.as_str(), ahora.segundos, caras.len()));
    let xmp = xmp(&opciones.titulo, &ahora, informe.pdfx_identificado.then_some(version_pdfx), &id_documento);
    let mut flujo_xmp = Stream::new(dictionary! { "Type" => "Metadata", "Subtype" => "XML" }, xmp.into_bytes());
    flujo_xmp.allows_compression = false;
    catalogo.set("Metadata", doc.add_object(flujo_xmp));
    let id_catalogo = doc.add_object(catalogo);

    let mut info = dictionary! {
        "Title" => Object::string_literal(opciones.titulo.clone()),
        "Creator" => Object::string_literal("Macula"),
        "Producer" => Object::string_literal(concat!("montajes-core ", env!("CARGO_PKG_VERSION"))),
        "CreationDate" => Object::string_literal(ahora.pdf()),
        "ModDate" => Object::string_literal(ahora.pdf()),
        "Trapped" => "False",
    };
    if informe.pdfx_identificado {
        info.set("GTS_PDFXVersion", Object::string_literal(version_pdfx));
    }
    let id_info = doc.add_object(info);

    let id_bytes = hex_a_bytes(&id_documento);
    doc.trailer = dictionary! {
        "Root" => id_catalogo,
        "Info" => id_info,
        "ID" => vec![
            Object::String(id_bytes.clone(), StringFormat::Hexadecimal),
            Object::String(id_bytes, StringFormat::Hexadecimal),
        ],
    };
    doc.version = "1.6".into();

    // 5. Eliminar páginas originales, marcadores y todo lo que quedó suelto.
    doc.prune_objects();
    doc.renumber_objects();
    doc.compress();
    Ok((doc, informe))
}

/// Texto para un flujo de contenido con fuente WinAnsi: escapa paréntesis y
/// convierte a Latin-1 (cubre tildes, ñ y ×).
/// Recuadro blanco del código: símbolo más la línea «ISBN …» encima (mm).
pub fn caja_codigo(cod: &CodigoBarras) -> (f64, f64) {
    (cod.ancho, cod.alto + cod.cuerpo * 2.2 + 1.0)
}

/// Operadores PDF del código de barras con su recuadro blanco, con la esquina
/// inferior izquierda del recuadro en (x, y) mm. Usa la fuente /F1 (Helvetica).
pub fn dibujar_codigo(cod: &CodigoBarras, x: f64, y: f64) -> Vec<u8> {
    let (ancho, alto) = caja_codigo(cod);
    let p = |v: f64| n(mm_a_pt(v));
    let mut s = format!("q 0 0 0 0 k {} {} {} {} re f 0 0 0 1 k\n", p(x), p(y), p(ancho), p(alto));
    let base_barras = y + 0.5;
    for b in &cod.barras {
        s += &format!("{} {} {} {} re ", p(x + b.x), p(base_barras + b.y), p(b.ancho), p(b.alto));
    }
    s += "f\n";
    let tam = mm_a_pt(cod.cuerpo) / 0.72; // altura de mayúscula ≈ 0,72 em
    let mut texto = |t: &str, cx: f64, linea: f64, tam: f64| {
        let largo = 0.556 * tam * t.chars().count() as f64; // dígitos de Helvetica: 556/1000 em
        s += &format!("BT /F1 {} Tf 1 0 0 1 {} {} Tm ", n(tam), n(mm_a_pt(cx) - largo / 2.0), n(mm_a_pt(linea)));
        s += &String::from_utf8_lossy(&texto_pdf(t));
        s += " Tj ET\n";
    };
    for d in &cod.digitos {
        texto(&d.texto, x + d.x, base_barras + d.y, tam);
    }
    texto(&format!("ISBN {}", cod.isbn), x + ancho / 2.0, base_barras + cod.alto + cod.cuerpo * 0.6, tam * 0.85);
    s += "Q\n";
    s.into_bytes()
}

fn texto_pdf(s: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(s.len() + 2);
    v.push(b'(');
    for c in s.chars() {
        match c {
            '(' | ')' | '\\' => {
                v.push(b'\\');
                v.push(c as u8);
            }
            c if (c as u32) < 256 => v.push(c as u32 as u8),
            '—' => v.push(0x97),
            _ => v.push(b'?'),
        }
    }
    v.push(b')');
    v
}

/// Milímetros con coma decimal y sin ceros sobrantes: 12,8 / 148.
pub fn mm_es(v: f64) -> String {
    n((v * 100.0).round() / 100.0).replace('.', ",")
}

/// Plantilla de portada para el diseñador: página del tamaño final con
/// rebase y guías (corte, rebase, pliegues, zona segura, rótulos) en una capa
/// que se ve en pantalla pero no se imprime.
pub fn plantilla_portada(
    c: &Portada,
    titulo: &str,
    nota: &str,
    isbn: Option<&CodigoBarras>,
    destino: &Path,
) -> Resultado<()> {
    documento_plantilla_portada(c, titulo, nota, isbn).save(destino)?;
    Ok(())
}

/// Igual que [`plantilla_portada`], pero en memoria.
pub fn documento_plantilla_portada(c: &Portada, titulo: &str, nota: &str, isbn: Option<&CodigoBarras>) -> Document {
    const MARGEN: f64 = 20.0;
    let r = c.rebase;
    let ancho = c.tamano.ancho + 2.0 * (r + MARGEN);
    let alto = c.tamano.alto + 2.0 * (r + MARGEN);
    let corte = Rect::new(MARGEN + r, MARGEN + r, c.tamano.ancho, c.tamano.alto);
    let sangrado = corte.expandir(r, r, r, r);
    let re =
        |r: Rect| format!("{} {} {} {} re", n(mm_a_pt(r.x)), n(mm_a_pt(r.y)), n(mm_a_pt(r.ancho)), n(mm_a_pt(r.alto)));
    let linea = |x1: f64, y1: f64, x2: f64, y2: f64| {
        format!("{} {} m {} {} l S\n", n(mm_a_pt(x1)), n(mm_a_pt(y1)), n(mm_a_pt(x2)), n(mm_a_pt(y2)))
    };

    let mut s: Vec<u8> = Vec::new();
    let texto = |s: &mut Vec<u8>, tam: f64, x: f64, y: f64, girado: bool, t: &str| {
        // Centrado aproximado: Helvetica mide ~0,55 em por carácter.
        let largo = 0.55 * tam * t.chars().count() as f64;
        let (x, y) = (mm_a_pt(x), mm_a_pt(y));
        let matriz = if girado {
            format!("0 1 -1 0 {} {} Tm", n(x + tam / 3.0), n(y - largo / 2.0))
        } else {
            format!("1 0 0 1 {} {} Tm", n(x - largo / 2.0), n(y))
        };
        s.extend_from_slice(format!("BT /F1 {} Tf {matriz} ", n(tam)).as_bytes());
        s.extend(texto_pdf(t));
        s.extend_from_slice(b" Tj ET\n");
    };
    s.extend_from_slice(b"/OC /Guias BDC q\n");
    // Zona de rebase en gris claro y bordes de rebase (rojo) y corte (cian).
    s.extend_from_slice(format!("0.93 g {} {} f*\n", re(sangrado), re(corte)).as_bytes());
    s.extend_from_slice(format!("1 0 0 RG 0.5 w {} S\n", re(sangrado)).as_bytes());
    s.extend_from_slice(format!("0 0.6 1 RG 0.75 w {} S\n", re(corte)).as_bytes());
    // Pliegues (magenta discontinuo).
    s.extend_from_slice(b"1 0 1 RG 0.5 w [4 2] 0 d\n");
    for &x in &c.pliegues {
        s.extend_from_slice(linea(corte.x + x, sangrado.y, corte.x + x, sangrado.arriba()).as_bytes());
    }
    for &y in &c.pliegues_horizontales {
        s.extend_from_slice(linea(sangrado.x, corte.y + y, sangrado.derecha(), corte.y + y).as_bytes());
    }
    // Zona segura (verde punteado) en los paneles que llevan diseño.
    s.extend_from_slice(b"0 0.6 0 RG [1 2] 0 d\n");
    for p in &c.paneles {
        let seguridad = match p.tipo {
            TipoPanel::Lomo => 1.5_f64.min(p.rect.ancho / 4.0),
            TipoPanel::Bisagra | TipoPanel::Vuelta => continue,
            _ => c.seguridad,
        };
        let interior = Rect::new(corte.x + p.rect.x, corte.y + p.rect.y, p.rect.ancho, p.rect.alto).expandir(
            -seguridad,
            -c.seguridad,
            -seguridad,
            -c.seguridad,
        );
        if interior.ancho > 0.0 && interior.alto > 0.0 {
            s.extend_from_slice(format!("{} S\n", re(interior)).as_bytes());
        }
    }
    s.extend_from_slice(b"[] 0 d 0 g\n");
    // Rótulos de cada panel con su medida.
    for p in &c.paneles {
        // Las vueltas de cabeza y pie se entienden por los pliegues; no se rotulan.
        if p.tipo == TipoPanel::Vuelta && p.rect.alto < c.tamano.alto - 1e-6 {
            continue;
        }
        let cx = corte.x + p.rect.x + p.rect.ancho / 2.0;
        let cy = corte.y + p.rect.y + p.rect.alto / 2.0;
        let medida = format!("{} mm", mm_es(p.rect.ancho));
        if p.rect.ancho < 30.0 {
            texto(&mut s, 7.0, cx, cy, true, &format!("{} · {medida}", p.tipo.nombre()));
        } else {
            texto(&mut s, 10.0, cx, cy + 2.0, false, p.tipo.nombre());
            texto(&mut s, 8.0, cx, cy - 4.0, false, &format!("{medida} × {} mm", mm_es(p.rect.alto)));
        }
    }
    // Encabezado y leyenda fuera del área de impresión.
    texto(&mut s, 11.0, ancho / 2.0, alto - 9.0, false, titulo);
    texto(&mut s, 8.0, ancho / 2.0, alto - 15.0, false, nota);
    texto(
        &mut s,
        7.0,
        ancho / 2.0,
        6.0,
        false,
        "Cian: corte · Rojo: rebase · Magenta: pliegues · Verde: zona segura. Las guías no se imprimen.",
    );
    s.extend_from_slice(b"Q EMC\n");
    // Código de barras del ISBN: sí se imprime (K 100 % sobre recuadro blanco).
    if let Some(cod) = isbn {
        let (ancho_caja, alto_caja) = caja_codigo(cod);
        if let Some((x, y)) = crate::portada::posicion_codigo(c, ancho_caja, alto_caja) {
            s.extend(dibujar_codigo(cod, corte.x + x, corte.y + y));
        }
    }

    let mut doc = Document::with_version("1.6");
    let fuente = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let capa = doc.add_object(dictionary! {
        "Type" => "OCG",
        "Name" => Object::string_literal("Guías de portada (no imprime)"),
        "Usage" => dictionary! {
            "Print" => dictionary! { "PrintState" => "OFF" },
            "View" => dictionary! { "ViewState" => "ON" },
            "Export" => dictionary! { "ExportState" => "OFF" },
        },
    });
    let contenido = doc.add_object(Stream::new(dictionary! {}, s));
    let arbol = doc.new_object_id();
    let caja = |r: Rect| -> Object {
        vec![mm_a_pt(r.x).into(), mm_a_pt(r.y).into(), mm_a_pt(r.derecha()).into(), mm_a_pt(r.arriba()).into()].into()
    };
    let pagina = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => arbol,
        "MediaBox" => caja(Rect::new(0.0, 0.0, ancho, alto)),
        "TrimBox" => caja(corte),
        "BleedBox" => caja(sangrado),
        "Contents" => contenido,
        "Resources" => dictionary! {
            "Font" => dictionary! { "F1" => fuente },
            "Properties" => dictionary! { "Guias" => capa },
        },
    });
    doc.objects.insert(
        arbol,
        Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![pagina.into()], "Count" => 1 }),
    );
    let catalogo = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => arbol,
        "OCProperties" => dictionary! {
            "OCGs" => vec![capa.into()],
            "D" => dictionary! {
                "ON" => vec![capa.into()],
                "AS" => vec![
                    dictionary! { "Event" => "Print", "OCGs" => vec![capa.into()], "Category" => vec!["Print".into()] }.into(),
                    dictionary! { "Event" => "View", "OCGs" => vec![capa.into()], "Category" => vec!["View".into()] }.into(),
                ],
            },
        },
    });
    let info = doc.add_object(dictionary! {
        "Title" => Object::string_literal(titulo),
        "Creator" => Object::string_literal("Macula"),
    });
    doc.trailer.set("Root", catalogo);
    doc.trailer.set("Info", info);
    doc.compress();
    doc
}

/// Dibuja la página `nombre` recortada a `zona` (mm) con la matriz `m` (pt).
/// Coloca una página con el rebase hecho con su capa de fondo (`fondo`, ver
/// [`crate::fondo`]):
/// 1. en cada franja de rebase, la capa de fondo reflejada sobre el borde
///    (rellena lo que el fondo no alcanza, sin textos ni logos);
/// 2. encima, en todo el anillo de rebase, la capa de fondo tal cual: donde la
///    foto o el fondo siguen más allá del corte, se ve su continuación real;
/// 3. la página encima, recortada al corte con un pase mínimo hacia el rebase.
fn colocar_extendido(
    contenido: &mut String,
    c: Rect,
    r: Rect,
    pide: [f64; 4],
    m: &[f64; 6],
    pagina: &str,
    fondo: &str,
) {
    let hace = pide.map(|v| v > 0.01);
    let (x0, y0, x1, y1) = (mm_a_pt(c.x), mm_a_pt(c.y), mm_a_pt(c.derecha()), mm_a_pt(c.arriba()));
    let espejo = |l: usize| -> [f64; 6] {
        match l {
            0 => [-1.0, 0.0, 0.0, 1.0, 2.0 * x0, 0.0],
            2 => [-1.0, 0.0, 0.0, 1.0, 2.0 * x1, 0.0],
            1 => [1.0, 0.0, 0.0, -1.0, 0.0, 2.0 * y0],
            _ => [1.0, 0.0, 0.0, -1.0, 0.0, 2.0 * y1],
        }
    };
    let [pi, pb, pd, pa] = pide;
    let franjas = [
        (0, Rect::new(r.x, c.y, pi, c.alto)),
        (2, Rect::new(c.derecha(), c.y, pd, c.alto)),
        (1, Rect::new(c.x, r.y, c.ancho, pb)),
        (3, Rect::new(c.x, c.arriba(), c.ancho, pa)),
    ];
    for (l, zona) in franjas {
        if hace[l] {
            colocar(contenido, zona, &multiplicar(m, &espejo(l)), fondo);
        }
    }
    for (v, h, zona) in [
        (0, 1, Rect::new(r.x, r.y, pi, pb)),
        (2, 1, Rect::new(c.derecha(), r.y, pd, pb)),
        (0, 3, Rect::new(r.x, c.arriba(), pi, pa)),
        (2, 3, Rect::new(c.derecha(), c.arriba(), pd, pa)),
    ] {
        if hace[v] && hace[h] {
            colocar(contenido, zona, &multiplicar(&multiplicar(m, &espejo(v)), &espejo(h)), fondo);
        }
    }
    // El anillo de rebase: el rectángulo de rebase menos el de corte (par-impar).
    let caja =
        |z: Rect| format!("{} {} {} {} re ", n(mm_a_pt(z.x)), n(mm_a_pt(z.y)), n(mm_a_pt(z.ancho)), n(mm_a_pt(z.alto)));
    contenido.push_str(&format!(
        "q {}{}W* n {} {} {} {} {} {} cm /{} Do Q\n",
        caja(r),
        caja(c),
        n(m[0]),
        n(m[1]),
        n(m[2]),
        n(m[3]),
        n(m[4]),
        n(m[5]),
        fondo
    ));
    // La página encima, pasada un poco del corte: el filo que suavizan los
    // visores queda en el rebase, no sobre la pieza.
    let pase: [f64; 4] = std::array::from_fn(|l| if hace[l] { pide[l].min(PASE) } else { 0.0 });
    colocar(contenido, c.expandir(pase[0], pase[1], pase[2], pase[3]), m, pagina);
}

/// Coloca una página con el rebase de cada borde hecho a su manera (lados en
/// el pliego: izquierda, abajo, derecha, arriba):
/// 1. franjas en espejo o estiradas, debajo de la página;
/// 2. la página, con el rebase original donde se usa y un pase mínimo donde no;
/// 3. franjas de color plano encima, desde el corte hacia afuera.
#[allow(clippy::too_many_arguments)]
fn colocar_por_lados(
    contenido: &mut String,
    c: Rect,
    r: Rect,
    pide: [f64; 4],
    tiene: f64,
    modo: [ModoRebase; 4],
    color: [Option<[f32; 3]>; 4],
    m: &[f64; 6],
    nombre: &str,
    cmyk: bool,
) {
    let hace = pide.map(|v| v > 0.01);
    // Sin rebase suficiente en el PDF, «original» pasa a espejo; sin color, a estirar.
    let modo: [ModoRebase; 4] = std::array::from_fn(|l| match modo[l] {
        ModoRebase::Original if pide[l] > tiene + 0.05 => ModoRebase::Espejo,
        ModoRebase::Color if color[l].is_none() => ModoRebase::Estirar,
        otro => otro,
    });
    let original = std::array::from_fn::<bool, 4, _>(|l| hace[l] && modo[l] == ModoRebase::Original);
    let reflejo =
        std::array::from_fn::<bool, 4, _>(|l| hace[l] && matches!(modo[l], ModoRebase::Espejo | ModoRebase::Estirar));
    let plano = std::array::from_fn::<bool, 4, _>(|l| hace[l] && modo[l] == ModoRebase::Color);

    // Transformación (en el pliego) que lleva la página a la franja de un lado.
    let (x0, y0, x1, y1) = (mm_a_pt(c.x), mm_a_pt(c.y), mm_a_pt(c.derecha()), mm_a_pt(c.arriba()));
    let e = mm_a_pt(ORILLA);
    let transformar = |lado: usize| -> [f64; 6] {
        let (borde, signo, horizontal) = match lado {
            0 => (x0, 1.0, true),
            2 => (x1, -1.0, true),
            1 => (y0, 1.0, false),
            _ => (y1, -1.0, false),
        };
        let (k, t) = if modo[lado] == ModoRebase::Estirar {
            // La orilla [borde, borde + signo·e] se estira hasta cubrir el rebase.
            let fijo = borde + signo * e;
            let k = (e + mm_a_pt(pide[lado])) / e;
            (k, fijo * (1.0 - k))
        } else {
            (-1.0, 2.0 * borde)
        };
        if horizontal { [k, 0.0, 0.0, 1.0, t, 0.0] } else { [1.0, 0.0, 0.0, k, 0.0, t] }
    };

    // 1. Franjas reflejadas o estiradas (debajo). A lo largo del borde se
    //    alargan sobre los lados vecinos que usan el rebase original.
    let [pi, pb, pd, pa] = pide;
    let alto_y = |l_abajo: bool, l_arriba: bool| {
        let y = if l_abajo { r.y } else { c.y };
        let y2 = if l_arriba { r.arriba() } else { c.arriba() };
        (y, y2 - y)
    };
    let ancho_x = |l_izq: bool, l_der: bool| {
        let x = if l_izq { r.x } else { c.x };
        let x2 = if l_der { r.derecha() } else { c.derecha() };
        (x, x2 - x)
    };
    let (vy, valto) = alto_y(original[1], original[3]);
    let (hx, hancho) = ancho_x(original[0], original[2]);
    let franjas = [
        (0, Rect::new(r.x, vy, pi + SOLAPE, valto)),
        (2, Rect::new(c.derecha() - SOLAPE, vy, pd + SOLAPE, valto)),
        (1, Rect::new(hx, r.y, hancho, pb + SOLAPE)),
        (3, Rect::new(hx, c.arriba() - SOLAPE, hancho, pa + SOLAPE)),
    ];
    for (lado, zona) in franjas {
        if reflejo[lado] {
            colocar(contenido, zona, &multiplicar(m, &transformar(lado)), nombre);
        }
    }
    // Esquinas entre dos franjas reflejadas: las dos transformaciones.
    for (v, h, zona) in [
        (0, 1, Rect::new(r.x, r.y, pi, pb)),
        (2, 1, Rect::new(c.derecha(), r.y, pd, pb)),
        (0, 3, Rect::new(r.x, c.arriba(), pi, pa)),
        (2, 3, Rect::new(c.derecha(), c.arriba(), pd, pa)),
    ] {
        if reflejo[v] && reflejo[h] {
            let mt = multiplicar(&multiplicar(m, &transformar(v)), &transformar(h));
            colocar(contenido, zona, &mt, nombre);
        }
    }

    // 2. La página.
    let pase: [f64; 4] = std::array::from_fn(|l| {
        if original[l] {
            pide[l]
        } else if hace[l] {
            pide[l].min(PASE)
        } else {
            0.0
        }
    });
    colocar(contenido, c.expandir(pase[0], pase[1], pase[2], pase[3]), m, nombre);

    // 3. Color plano encima (también tapa las esquinas que le tocan).
    let bandas = [
        (0, Rect::new(r.x, r.y, pi, r.alto)),
        (2, Rect::new(c.derecha(), r.y, pd, r.alto)),
        (1, Rect::new(r.x, r.y, r.ancho, pb)),
        (3, Rect::new(r.x, c.arriba(), r.ancho, pa)),
    ];
    for (lado, zona) in bandas {
        let Some([rr, gg, bb]) = color[lado].filter(|_| plano[lado]) else { continue };
        if zona.ancho <= 1e-6 || zona.alto <= 1e-6 {
            continue;
        }
        let (rr, gg, bb) = (f64::from(rr), f64::from(gg), f64::from(bb));
        let pintura = if cmyk {
            let (cc, mm_, yy, kk) = rgb_a_cmyk(rr, gg, bb);
            format!("{} {} {} {} k", n(cc), n(mm_), n(yy), n(kk))
        } else {
            format!("{} {} {} rg", n(rr), n(gg), n(bb))
        };
        // En las esquinas, la franja horizontal solo cubre lo que no es de
        // una franja de color vertical (que ya va completa).
        let zona = if lado == 1 || lado == 3 {
            let x = if plano[0] { c.x } else { r.x };
            let x2 = if plano[2] { c.derecha() } else { r.derecha() };
            Rect::new(x, zona.y, x2 - x, zona.alto)
        } else {
            zona
        };
        contenido.push_str(&format!(
            "q {pintura} {} {} {} {} re f Q\n",
            n(mm_a_pt(zona.x)),
            n(mm_a_pt(zona.y)),
            n(mm_a_pt(zona.ancho)),
            n(mm_a_pt(zona.alto))
        ));
    }
}

fn colocar(s: &mut String, zona: Rect, m: &[f64; 6], nombre: &str) {
    s.push_str(&format!(
        "q {} {} {} {} re W n {} {} {} {} {} {} cm /{} Do Q\n",
        n(mm_a_pt(zona.x)),
        n(mm_a_pt(zona.y)),
        n(mm_a_pt(zona.ancho)),
        n(mm_a_pt(zona.alto)),
        n(m[0]),
        n(m[1]),
        n(m[2]),
        n(m[3]),
        n(m[4]),
        n(m[5]),
        nombre
    ));
}

/// `a × b`: primero se aplica `a`, luego `b`.
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

fn crear_forma(
    doc: &mut Document,
    p: &PaginaFuente,
    corr: &Correcciones,
    conteo: &mut Conteo,
    pendientes: &mut Vec<FormaPendiente>,
) -> Resultado<ObjectId> {
    let mut contenido = doc.get_page_content_with_limit(p.id, LIMITE_CONTENIDO)?;
    let mut recursos = heredado(doc, p.id, b"Resources").unwrap_or_else(|| Object::Dictionary(Dictionary::new()));
    if corr.reescribe_contenido() {
        let dict = resolver(doc, &recursos).as_dict().ok();
        let identidad = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
        if let Some(nuevo) = corregir_flujo(doc, &contenido, dict, identidad, corr, conteo, pendientes) {
            contenido = nuevo;
            recursos = Object::Dictionary(recursos_con_sobreimpresion(doc, Some(&recursos)));
        }
    }
    let mut dict = dictionary! {
        "Type" => "XObject",
        "Subtype" => "Form",
        "BBox" => vec![p.media.x0.into(), p.media.y0.into(), p.media.x1.into(), p.media.y1.into()],
        "Resources" => recursos,
    };
    // El grupo de transparencia de la página debe acompañar a la forma.
    if let Ok(grupo) = doc.get_dictionary(p.id).and_then(|d| d.get(b"Group")) {
        dict.set("Group", grupo.clone());
    }
    Ok(doc.add_object(Stream::new(dict, contenido)))
}

/// Corrige los Form XObjects que usan las páginas (y los que estos usan).
fn corregir_formas_anidadas(
    doc: &mut Document,
    corr: &Correcciones,
    conteo: &mut Conteo,
    mut pendientes: Vec<FormaPendiente>,
) {
    let mut hechas = std::collections::HashSet::new();
    while let Some(f) = pendientes.pop() {
        if !hechas.insert(f.id) || hechas.len() > 10_000 {
            continue;
        }
        let Ok(Object::Stream(s)) = doc.get_object(f.id) else { continue };
        let Ok(datos) = s.decompressed_content().or_else(|_| Ok::<_, ()>(s.content.clone())) else { continue };
        let recursos = s.dict.get(b"Resources").ok().cloned();
        let dict = recursos.as_ref().and_then(|r| resolver(doc, r).as_dict().ok());
        let mut nuevas = Vec::new();
        let Some(nuevo) = corregir_flujo(doc, &datos, dict, f.ctm, corr, conteo, &mut nuevas) else {
            pendientes.extend(nuevas);
            continue;
        };
        let recursos = recursos_con_sobreimpresion(doc, recursos.as_ref());
        pendientes.extend(nuevas);
        if let Ok(Object::Stream(s)) = doc.get_object_mut(f.id) {
            s.dict.remove(b"Filter");
            s.dict.remove(b"DecodeParms");
            s.dict.set("Resources", recursos);
            s.set_plain_content(nuevo);
        }
    }
}

/// Lado de un pliego según su nombre («… tiro», «… retiro», «tira y retira»).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lado {
    Tiro,
    Retiro,
    TiraRetira,
}

/// Rótulo de lado de cada cara: «TIRO 1», «RETIRO 1», «TIRO 2»… (cada retiro
/// lleva el número del tiro anterior).
fn lados_de(caras: &[Cara]) -> Vec<(String, Lado)> {
    let mut n = 0;
    caras
        .iter()
        .map(|c| {
            let nombre = c.nombre.to_lowercase();
            if nombre.contains("tira y retira") {
                n += 1;
                (format!("TIRA Y RETIRA {n}"), Lado::TiraRetira)
            } else if nombre.trim_end().ends_with("retiro") {
                (format!("RETIRO {}", n.max(1)), Lado::Retiro)
            } else {
                n += 1;
                (format!("TIRO {n}"), Lado::Tiro)
            }
        })
        .collect()
}

/// Rótulo de la plancha: el nombre de cada tinta escrito en esa misma tinta
/// (así cada plancha dice cuál es) y, en color de registro, el archivo y el
/// lado: «CYAN / MAGENTA / AMARILLO / NEGRO  VOLANTES  TIRO 1».
fn dibujar_rotulo(s: &mut String, r: &crate::imposicion::marcas::Rotulo, tintas: u8, titulo: &str, lado: &str) {
    use crate::letras;
    const REGISTRO: &str = "/Registro CS 1 SCN";
    let tintas: &[(&str, &str)] = if tintas == 1 {
        &[("NEGRO", "0 0 0 1 K")]
    } else {
        &[("CYAN", "1 0 0 0 K"), ("MAGENTA", "0 1 0 0 K"), ("AMARILLO", "0 0 1 0 K"), ("NEGRO", "0 0 0 1 K")]
    };
    let mut partes: Vec<(String, &str)> = Vec::new();
    for (i, (nombre, color)) in tintas.iter().enumerate() {
        if i > 0 {
            partes.push((" / ".into(), REGISTRO));
        }
        partes.push(((*nombre).into(), color));
    }
    let fijo = letras::largo(&partes.iter().map(|(t, _)| t.as_str()).collect::<String>(), r.alto);
    let cola = format!("   {}   {lado}", titulo.trim());
    let cola = letras::ajustar(&cola, r.alto, (r.largo - fijo).max(0.0));
    partes.push((cola, REGISTRO));

    let u = mm_a_pt(r.alto / 6.0);
    let (x, y) = (mm_a_pt(r.x), mm_a_pt(r.y));
    let matriz = if r.vertical { format!("0 {} {} 0", n(u), n(-u)) } else { format!("{} 0 0 {}", n(u), n(u)) };
    // Trazo de 0,6 pt, en unidades de letra.
    s.push_str(&format!("q {matriz} {} {} cm {} w 1 J 1 j\n", n(x), n(y), n(0.6 / u)));
    let mut avance = 0.0;
    for (texto, color) in partes {
        let (segmentos, ancho) = letras::segmentos(&texto);
        if !segmentos.is_empty() {
            s.push_str(color);
            s.push('\n');
            for seg in segmentos {
                for (i, (a, b)) in seg.iter().enumerate() {
                    s.push_str(&format!("{} {} {} ", n(a + avance), n(*b), if i == 0 { "m" } else { "l" }));
                }
                s.push_str("S\n");
            }
        }
        avance += ancho;
    }
    s.push_str("Q\n");
}

/// Marcas de la cara. A una tinta, la tira de control lleva solo negro (cada
/// parche con el tono de gris que le corresponde).
fn dibujar_marcas(m: &Marcas, s: &mut String, tintas: u8) {
    if !m.tira_color.is_empty() {
        s.push_str("q\n");
        for p in &m.tira_color {
            let mut p = *p;
            if tintas == 1 {
                let [c, mg, y, k] = p.cmyk;
                let gris = 0.30 * (1.0 - c) * (1.0 - k) + 0.59 * (1.0 - mg) * (1.0 - k) + 0.11 * (1.0 - y) * (1.0 - k);
                p.cmyk = [0.0, 0.0, 0.0, 1.0 - gris];
            }
            s.push_str(&format!(
                "{} {} {} {} k {} {} {} {} re f\n",
                n(p.cmyk[0]),
                n(p.cmyk[1]),
                n(p.cmyk[2]),
                n(p.cmyk[3]),
                n(mm_a_pt(p.rect.x)),
                n(mm_a_pt(p.rect.y)),
                n(mm_a_pt(p.rect.ancho)),
                n(mm_a_pt(p.rect.alto))
            ));
        }
        s.push_str("Q\n");
    }
    if m.corte.is_empty() && m.registro.is_empty() && m.pliegues.is_empty() && m.alzado.is_empty() {
        return;
    }
    // Trazo de 0,25 pt en color de registro.
    s.push_str("q /Registro CS 1 SCN /Registro cs 1 scn 0.25 w 0 J\n");
    for l in &m.corte {
        s.push_str(&format!(
            "{} {} m {} {} l S\n",
            n(mm_a_pt(l.x1)),
            n(mm_a_pt(l.y1)),
            n(mm_a_pt(l.x2)),
            n(mm_a_pt(l.y2))
        ));
    }
    // Plegado: línea discontinua.
    if !m.pliegues.is_empty() {
        s.push_str("[2 2] 0 d\n");
        for l in &m.pliegues {
            s.push_str(&format!(
                "{} {} m {} {} l S\n",
                n(mm_a_pt(l.x1)),
                n(mm_a_pt(l.y1)),
                n(mm_a_pt(l.x2)),
                n(mm_a_pt(l.y2))
            ));
        }
        s.push_str("[] 0 d\n");
    }
    // Marcas de alzado (escalera en el lomo de cada firma).
    for r in &m.alzado {
        s.push_str(&format!(
            "{} {} {} {} re f\n",
            n(mm_a_pt(r.x)),
            n(mm_a_pt(r.y)),
            n(mm_a_pt(r.ancho)),
            n(mm_a_pt(r.alto))
        ));
    }
    for r in &m.registro {
        let (x, y, radio) = (mm_a_pt(r.x), mm_a_pt(r.y), mm_a_pt(r.radio));
        // Cruz que sobresale del círculo, círculo y punto central.
        s.push_str(&format!("{} {} m {} {} l S\n", n(x - radio), n(y), n(x + radio), n(y)));
        s.push_str(&format!("{} {} m {} {} l S\n", n(x), n(y - radio), n(x), n(y + radio)));
        circulo(s, x, y, radio * 0.6);
        s.push_str("S\n");
        circulo(s, x, y, radio * 0.25);
        s.push_str("f\n");
    }
    s.push_str("Q\n");
}

/// Círculo con cuatro curvas de Bézier.
fn circulo(s: &mut String, x: f64, y: f64, r: f64) {
    let k = 0.552_284_75 * r;
    s.push_str(&format!("{} {} m\n", n(x + r), n(y)));
    s.push_str(&format!("{} {} {} {} {} {} c\n", n(x + r), n(y + k), n(x + k), n(y + r), n(x), n(y + r)));
    s.push_str(&format!("{} {} {} {} {} {} c\n", n(x - k), n(y + r), n(x - r), n(y + k), n(x - r), n(y)));
    s.push_str(&format!("{} {} {} {} {} {} c\n", n(x - r), n(y - k), n(x - k), n(y - r), n(x), n(y - r)));
    s.push_str(&format!("{} {} {} {} {} {} c\n", n(x + k), n(y - r), n(x + r), n(y - k), n(x + r), n(y)));
}

fn segundos_actuales() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Fecha UTC sin dependencias externas.
struct Fecha {
    segundos: u64,
    anio: i64,
    mes: u32,
    dia: u32,
    hora: u32,
    minuto: u32,
    segundo: u32,
}

impl Fecha {
    fn desde(segundos: u64) -> Self {
        // Algoritmo de días civiles de Howard Hinnant.
        let dias = (segundos / 86_400) as i64 + 719_468;
        let era = dias.div_euclid(146_097);
        let doe = dias - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let dia = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let mes = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        let anio = yoe + era * 400 + i64::from(mes <= 2);
        let resto = segundos % 86_400;
        Self {
            segundos,
            anio,
            mes,
            dia,
            hora: (resto / 3600) as u32,
            minuto: (resto % 3600 / 60) as u32,
            segundo: (resto % 60) as u32,
        }
    }

    fn pdf(&self) -> String {
        format!(
            "D:{:04}{:02}{:02}{:02}{:02}{:02}Z",
            self.anio, self.mes, self.dia, self.hora, self.minuto, self.segundo
        )
    }

    fn iso(&self) -> String {
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
            self.anio, self.mes, self.dia, self.hora, self.minuto, self.segundo
        )
    }
}

fn huella<T: Hash>(valor: &T) -> String {
    let mut a = DefaultHasher::new();
    valor.hash(&mut a);
    let mut b = DefaultHasher::new();
    (valor, "montajes").hash(&mut b);
    format!("{:016x}{:016x}", a.finish(), b.finish())
}

fn hex_a_bytes(hex: &str) -> Vec<u8> {
    (0..hex.len()).step_by(2).filter_map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok()).collect()
}

fn escapar_xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn xmp(titulo: &str, fecha: &Fecha, pdfx: Option<&str>, id: &str) -> String {
    let uuid = format!("{}-{}-{}-{}-{}", &id[0..8], &id[8..12], &id[12..16], &id[16..20], &id[20..32]);
    let pdfx = pdfx.map(|v| format!("\n   <pdfxid:GTS_PDFXVersion>{v}</pdfxid:GTS_PDFXVersion>")).unwrap_or_default();
    format!(
        r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:dc="http://purl.org/dc/elements/1.1/"
    xmlns:xmp="http://ns.adobe.com/xap/1.0/"
    xmlns:pdf="http://ns.adobe.com/pdf/1.3/"
    xmlns:xmpMM="http://ns.adobe.com/xap/1.0/mm/"
    xmlns:pdfxid="http://www.npes.org/pdfx/ns/id/">
   <dc:format>application/pdf</dc:format>
   <dc:title><rdf:Alt><rdf:li xml:lang="x-default">{titulo}</rdf:li></rdf:Alt></dc:title>
   <xmp:CreatorTool>Macula</xmp:CreatorTool>
   <xmp:CreateDate>{fecha}</xmp:CreateDate>
   <xmp:ModifyDate>{fecha}</xmp:ModifyDate>
   <xmp:MetadataDate>{fecha}</xmp:MetadataDate>
   <pdf:Producer>montajes-core {version}</pdf:Producer>
   <pdf:Trapped>False</pdf:Trapped>
   <xmpMM:DocumentID>uuid:{uuid}</xmpMM:DocumentID>
   <xmpMM:InstanceID>uuid:{uuid}</xmpMM:InstanceID>
   <xmpMM:VersionID>1</xmpMM:VersionID>
   <xmpMM:RenditionClass>default</xmpMM:RenditionClass>{pdfx}
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>"#,
        titulo = escapar_xml(titulo),
        fecha = fecha.iso(),
        version = env!("CARGO_PKG_VERSION"),
    )
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn tiros_y_retiros() {
        let cara = |nombre: &str| Cara {
            nombre: nombre.into(),
            pliego: Tamano::new(100.0, 100.0),
            ubicaciones: vec![],
            marcas: Marcas::default(),
            cajas: None,
        };
        let caras = [
            cara("Firma 1 tiro"),
            cara("Firma 1 retiro"),
            cara("Firma 2 tiro"),
            cara("Firma 2 retiro"),
            cara("Firma 3 · tira y retira"),
        ];
        let lados: Vec<String> = lados_de(&caras).into_iter().map(|(t, _)| t).collect();
        assert_eq!(lados, ["TIRO 1", "RETIRO 1", "TIRO 2", "RETIRO 2", "TIRA Y RETIRA 3"]);
    }

    #[test]
    fn rebase_por_lados() {
        // Pieza de 100×50 con 3 mm de rebase pedido en los cuatro lados.
        let c = Rect::new(10.0, 10.0, 100.0, 50.0);
        let r = c.expandir(3.0, 3.0, 3.0, 3.0);
        let m = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
        let gris = Some([0.5f32, 0.5, 0.5]);
        let mut s = String::new();
        let modo = [ModoRebase::Espejo, ModoRebase::Original, ModoRebase::Color, ModoRebase::Estirar];
        colocar_por_lados(&mut s, c, r, [3.0; 4], 3.0, modo, [gris; 4], &m, "P1", false);
        let lineas: Vec<&str> = s.lines().collect();
        // Espejo a la izquierda (escala −1 en x) y estirado arriba (escala y > 1).
        assert!(lineas.iter().any(|l| l.contains(" -1 0 0 1 ")));
        assert!(lineas.iter().any(|l| l.contains(" 1 0 0 11 ")));
        // La página usa el rebase original abajo (3 mm) y un pase mínimo en el resto.
        let pagina = format!("{} {} ", n(mm_a_pt(c.x - PASE)), n(mm_a_pt(c.y - 3.0)));
        assert!(lineas.iter().any(|l| l.starts_with(&format!("q {pagina}")) && l.ends_with("/P1 Do Q")));
        // El color va al final, encima, solo a la derecha.
        let ultima = lineas.last().unwrap();
        assert!(ultima.contains(" rg ") && ultima.contains(&n(mm_a_pt(c.derecha()))));
        assert_eq!(lineas.iter().filter(|l| l.contains(" rg ")).count(), 1);
        // «Original» sin rebase suficiente en el PDF pasa a espejo.
        let mut t = String::new();
        colocar_por_lados(&mut t, c, r, [3.0; 4], 1.0, [ModoRebase::Original; 4], [None; 4], &m, "P1", false);
        assert_eq!(t.lines().filter(|l| l.contains(" -1 ") || l.contains("0 0 -1")).count(), 8);
    }

    /// Aplica la matriz a un punto.
    fn aplicar(m: &[f64; 6], x: f64, y: f64) -> (f64, f64) {
        (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
    }

    #[test]
    fn la_matriz_lleva_la_caja_al_destino_en_cada_giro() {
        let b = Caja { x0: 10.0, y0: 20.0, x1: 110.0, y1: 70.0 }; // 100 × 50
        for giro in [0, 90, 180, 270] {
            let m = matriz_colocacion(&b, giro, 300.0, 400.0);
            let esquinas = [(b.x0, b.y0), (b.x1, b.y0), (b.x0, b.y1), (b.x1, b.y1)].map(|(x, y)| aplicar(&m, x, y));
            let min_x = esquinas.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
            let min_y = esquinas.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
            let max_x = esquinas.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
            let max_y = esquinas.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
            let (w, h) = if giro % 180 == 0 { (100.0, 50.0) } else { (50.0, 100.0) };
            assert_eq!((min_x, min_y, max_x - min_x, max_y - min_y), (300.0, 400.0, w, h), "giro {giro}");
        }
    }

    #[test]
    fn giro_horario() {
        // Con 90° horario, la esquina superior izquierda pasa a la superior derecha.
        let b = Caja { x0: 0.0, y0: 0.0, x1: 100.0, y1: 50.0 };
        let m = matriz_colocacion(&b, 90, 0.0, 0.0);
        assert_eq!(aplicar(&m, 0.0, 50.0), (50.0, 100.0));
    }

    #[test]
    fn fijar_cajas_pone_el_corte_dentro_del_rebase() {
        let mut doc = Document::with_version("1.7");
        let paginas = doc.new_object_id();
        let contenido = doc.add_object(Stream::new(dictionary! {}, Vec::new()));
        let pagina = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => paginas, "Contents" => contenido,
            "MediaBox" => vec![0.into(), 0.into(), Object::Real(mm_a_pt(154.0) as f32), Object::Real(mm_a_pt(216.0) as f32)],
        });
        doc.objects.insert(
            paginas,
            Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![pagina.into()], "Count" => 1 }),
        );
        let catalogo = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => paginas });
        doc.trailer.set("Root", catalogo);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();

        let fuente = Fuente::desde_bytes(&fijar_cajas(&bytes, 3.0).unwrap()).unwrap();
        let p = &fuente.paginas[0];
        assert!(p.tiene_trimbox);
        let t = p.tamano_corte();
        assert!((t.ancho - 148.0).abs() < 0.01 && (t.alto - 210.0).abs() < 0.01, "{t}");
        assert!((p.rebase_disponible() - 3.0).abs() < 0.01);
        assert!(fijar_cajas(&bytes, 200.0).is_err());
    }

    #[test]
    fn separar_dobles_parte_el_pliego_de_lectura_en_dos() {
        let mut doc = Document::with_version("1.7");
        let paginas = doc.new_object_id();
        let mut kids = Vec::new();
        for ancho in [148.0, 296.0, 148.0] {
            let contenido = doc.add_object(Stream::new(dictionary! {}, Vec::new()));
            let caja = |a: f64, b: f64, c: f64, d: f64| {
                vec![
                    Object::Real(mm_a_pt(a) as f32),
                    Object::Real(mm_a_pt(b) as f32),
                    Object::Real(mm_a_pt(c) as f32),
                    Object::Real(mm_a_pt(d) as f32),
                ]
            };
            kids.push(Object::Reference(doc.add_object(dictionary! {
                "Type" => "Page", "Parent" => paginas, "Contents" => contenido,
                "MediaBox" => caja(0.0, 0.0, ancho + 6.0, 216.0),
                "TrimBox" => caja(3.0, 3.0, ancho + 3.0, 213.0),
                "BleedBox" => caja(0.0, 0.0, ancho + 6.0, 216.0),
            })));
        }
        doc.objects
            .insert(paginas, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => 3 }));
        let catalogo = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => paginas });
        doc.trailer.set("Root", catalogo);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();

        let f = Fuente::desde_bytes(&separar_dobles(&bytes, &[1]).unwrap()).unwrap();
        assert_eq!(f.paginas.len(), 4);
        for p in &f.paginas {
            let t = p.tamano_corte();
            assert!((t.ancho - 148.0).abs() < 0.01 && (t.alto - 210.0).abs() < 0.01, "{t}");
            assert!((p.rebase_disponible() - 3.0).abs() < 0.01);
        }
        // La mitad izquierda va antes que la derecha.
        assert!(f.paginas[1].corte.x0 < f.paginas[2].corte.x0);
    }

    #[test]
    fn numeros_compactos() {
        assert_eq!(n(1.0), "1");
        assert_eq!(n(2.83466), "2.8347");
        assert_eq!(n(-0.00001), "0");
    }
}
