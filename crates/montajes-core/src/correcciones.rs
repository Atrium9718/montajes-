//! Correcciones automáticas de preflight sobre el contenido vectorial.
//!
//! Se reescriben los flujos de contenido de las páginas (y de sus Form
//! XObjects) sin rasterizar nada:
//! - sobreimprimir el negro 100 % (texto y vectores en `0 0 0 1 k`);
//! - quitar la sobreimpresión de objetos blancos, que desaparecerían;
//! - engrosar las líneas más finas que el mínimo imprimible.
//!
//! El rebase en espejo se resuelve al colocar la página (ver [`crate::pdf`]).

use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document, Object, ObjectId};
use serde::{Deserialize, Serialize};

use crate::pdf::resolver;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Correcciones {
    /// Sobreimprimir el negro 100 % (evita filetes blancos por registro).
    #[serde(default)]
    pub sobreimprimir_negro: bool,
    /// Quitar la sobreimpresión de objetos blancos.
    #[serde(default)]
    pub quitar_sobreimpresion_blanco: bool,
    /// Grosor mínimo de línea en pt (p. ej. 0,25).
    #[serde(default)]
    pub linea_minima: Option<f64>,
    /// Generar el rebase reflejando la página cuando el PDF no lo trae.
    #[serde(default)]
    pub rebase_espejo: bool,
    /// Generar el rebase estirando solo la orilla de la página (el fondo
    /// sigue y los elementos no se repiten). Si está activo, gana al espejo.
    #[serde(default)]
    pub rebase_estirado: bool,
    /// Rebase solo con el color del fondo: se ignora lo que el PDF trae
    /// fuera del corte y cada franja de rebase se rellena con el color que
    /// tiene la página junto a ese borde (ver `fondos`).
    #[serde(default)]
    pub rebase_fondo: bool,
    /// Color del fondo junto a cada borde de cada página del PDF (RGB 0–1),
    /// en orden izquierda, abajo, derecha, arriba. Lo mide la app.
    #[serde(default)]
    pub fondos: Vec<Option<[[f32; 3]; 4]>>,
    /// Cómo se hace el rebase en cada borde de cada página del PDF (mismo
    /// orden que `fondos`). Lo decide la app mirando la página; si hay modos
    /// para una página, mandan sobre las opciones de rebase de arriba.
    #[serde(default)]
    pub modos: Vec<Option<[ModoRebase; 4]>>,
    /// Rebase con la capa de fondo de la página (ver [`crate::fondo`]): la
    /// foto o el fondo siguen más allá del corte, sin textos ni logos. Manda
    /// sobre las demás opciones de rebase en las páginas que tienen fondo.
    #[serde(default)]
    pub rebase_extendido: bool,
    /// Tintas del tiro y del retiro: 4 (CMYK) o 1 (todo se pasa a negro,
    /// ver [`crate::gris`]). Salen en el rótulo de cada plancha.
    #[serde(default = "cuatro")]
    pub tintas_tiro: u8,
    #[serde(default = "cuatro")]
    pub tintas_retiro: u8,
}

fn cuatro() -> u8 {
    4
}

/// Rebase de un borde.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModoRebase {
    /// El que trae el PDF (si alcanza; si no, espejo).
    Original,
    /// Reflejo de la orilla: sigue degradados y texturas.
    Espejo,
    /// Estirar la última orilla de la página.
    Estirar,
    /// Color plano del fondo junto a ese borde (ver `fondos`; sin color, estirar).
    Color,
}

impl Correcciones {
    pub fn ninguna() -> Self {
        Self {
            sobreimprimir_negro: false,
            quitar_sobreimpresion_blanco: false,
            linea_minima: None,
            rebase_espejo: false,
            rebase_estirado: false,
            rebase_fondo: false,
            fondos: Vec::new(),
            modos: Vec::new(),
            rebase_extendido: false,
            tintas_tiro: 4,
            tintas_retiro: 4,
        }
    }

    /// Las correcciones seguras recomendadas por defecto.
    pub fn recomendadas() -> Self {
        Self {
            sobreimprimir_negro: true,
            quitar_sobreimpresion_blanco: true,
            linea_minima: Some(0.25),
            rebase_espejo: false,
            rebase_estirado: true,
            rebase_fondo: false,
            fondos: Vec::new(),
            modos: Vec::new(),
            rebase_extendido: false,
            tintas_tiro: 4,
            tintas_retiro: 4,
        }
    }

    pub fn reescribe_contenido(&self) -> bool {
        self.sobreimprimir_negro || self.quitar_sobreimpresion_blanco || self.linea_minima.is_some()
    }
}

impl Default for Correcciones {
    fn default() -> Self {
        Self::ninguna()
    }
}

/// Cuántos cambios se hicieron, para el informe.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conteo {
    pub negro_sobreimpreso: usize,
    pub blancos_corregidos: usize,
    pub lineas_engrosadas: usize,
}

impl Conteo {
    pub fn avisos(&self) -> Vec<String> {
        let mut v = Vec::new();
        if self.negro_sobreimpreso > 0 {
            v.push(format!(
                "corregido: {} rellenos/trazos en negro 100 % ahora sobreimprimen",
                self.negro_sobreimpreso
            ));
        }
        if self.blancos_corregidos > 0 {
            v.push(format!("corregido: {} objetos blancos dejaron de sobreimprimir", self.blancos_corregidos));
        }
        if self.lineas_engrosadas > 0 {
            v.push(format!("corregido: {} grosores de línea llevados al mínimo imprimible", self.lineas_engrosadas));
        }
        v
    }
}

/// Nombres de los estados gráficos que se agregan a los recursos.
pub const GS_RELLENO_SI: &str = "MjOPf1";
pub const GS_RELLENO_NO: &str = "MjOPf0";
pub const GS_TRAZO_SI: &str = "MjOPs1";
pub const GS_TRAZO_NO: &str = "MjOPs0";

/// Diccionario ExtGState con los cuatro estados de sobreimpresión.
pub fn estados_sobreimpresion() -> Dictionary {
    let mut d = Dictionary::new();
    let gs = |clave: &str, valor: bool| {
        let mut g = Dictionary::new();
        g.set("Type", "ExtGState");
        g.set(clave, valor);
        if valor {
            // OPM 1: un 0 % en un canal no borra lo que hay debajo.
            g.set("OPM", 1);
        }
        Object::Dictionary(g)
    };
    d.set(GS_RELLENO_SI, gs("op", true));
    d.set(GS_RELLENO_NO, gs("op", false));
    d.set(GS_TRAZO_SI, gs("OP", true));
    d.set(GS_TRAZO_NO, gs("OP", false));
    d
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Clase {
    NegroPuro,
    Blanco,
    Otro,
}

#[derive(Debug, Clone, Copy)]
struct Estado {
    ctm: [f64; 6],
    relleno: Clase,
    trazo: Clase,
    /// Sobreimpresión activa según el documento o lo que se insertó.
    op_relleno: bool,
    op_trazo: bool,
    /// La sobreimpresión de negro fue puesta por nosotros (hay que apagarla al cambiar de color).
    nuestro_relleno: bool,
    nuestro_trazo: bool,
}

fn numeros(ops: &[Object]) -> Vec<f64> {
    ops.iter().filter_map(|o| o.as_float().ok().map(f64::from)).collect()
}

fn clase_cmyk(v: &[f64]) -> Clase {
    match v {
        [c, m, y, k] if c.abs() < 1e-4 && m.abs() < 1e-4 && y.abs() < 1e-4 && (k - 1.0).abs() < 1e-4 => {
            Clase::NegroPuro
        }
        [c, m, y, k] if [c, m, y, k].iter().all(|x| x.abs() < 1e-4) => Clase::Blanco,
        _ => Clase::Otro,
    }
}

fn clase_de(op: &str, v: &[f64]) -> Clase {
    match op {
        "k" | "K" => clase_cmyk(v),
        "g" | "G" if v.first().is_some_and(|g| (g - 1.0).abs() < 1e-4) => Clase::Blanco,
        "rg" | "RG" if v.len() == 3 && v.iter().all(|x| (x - 1.0).abs() < 1e-4) => Clase::Blanco,
        _ => Clase::Otro,
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

fn gs(nombre: &str) -> Operation {
    Operation::new("gs", vec![Object::Name(nombre.as_bytes().to_vec())])
}

/// Form XObject anidado que también hay que corregir, con la CTM con que se usa.
pub struct FormaPendiente {
    pub id: ObjectId,
    pub ctm: [f64; 6],
}

/// Reescribe un flujo de contenido. Devuelve el nuevo contenido (o `None` si
/// no hubo cambios) y anota en `pendientes` los Form XObjects que usa.
pub fn corregir_flujo(
    doc: &Document,
    datos: &[u8],
    recursos: Option<&Dictionary>,
    ctm: [f64; 6],
    corr: &Correcciones,
    conteo: &mut Conteo,
    pendientes: &mut Vec<FormaPendiente>,
) -> Option<Vec<u8>> {
    let contenido = Content::decode(datos).ok()?;
    let extgstate = |nombre: &Object| -> Option<&Dictionary> {
        let tabla = recursos?.get(b"ExtGState").ok().map(|t| resolver(doc, t))?.as_dict().ok()?;
        resolver(doc, tabla.get(nombre.as_name().ok()?).ok()?).as_dict().ok()
    };
    let mut e = Estado {
        ctm,
        relleno: Clase::Otro,
        trazo: Clase::Otro,
        op_relleno: false,
        op_trazo: false,
        nuestro_relleno: false,
        nuestro_trazo: false,
    };
    let mut pila: Vec<Estado> = Vec::new();
    let mut salida: Vec<Operation> = Vec::with_capacity(contenido.operations.len() + 16);
    let mut cambios = false;

    for mut op in contenido.operations {
        let nombre = op.operator.clone();
        match nombre.as_str() {
            "q" => pila.push(e),
            "Q" => e = pila.pop().unwrap_or(e),
            "cm" => {
                let v = numeros(&op.operands);
                if v.len() == 6 {
                    e.ctm = multiplicar(&[v[0], v[1], v[2], v[3], v[4], v[5]], &e.ctm);
                }
            }
            "gs" => {
                if let Some(g) = op.operands.first().and_then(extgstate) {
                    let b = |k: &[u8]| g.get(k).ok().and_then(|o| o.as_bool().ok());
                    if let Some(v) = b(b"OP") {
                        e.op_trazo = v;
                        e.nuestro_trazo = false;
                        // Si el documento no define «op», hereda de «OP».
                        if b(b"op").is_none() {
                            e.op_relleno = v;
                            e.nuestro_relleno = false;
                        }
                    }
                    if let Some(v) = b(b"op") {
                        e.op_relleno = v;
                        e.nuestro_relleno = false;
                    }
                }
            }
            "w" => {
                if let Some(minimo) = corr.linea_minima {
                    let escala = (e.ctm[0] * e.ctm[3] - e.ctm[1] * e.ctm[2]).abs().sqrt();
                    let w = op.operands.first().and_then(|o| o.as_float().ok()).map(f64::from).unwrap_or(1.0);
                    if escala > 1e-9 && w * escala < minimo - 1e-6 {
                        op.operands = vec![Object::Real((minimo / escala) as f32)];
                        conteo.lineas_engrosadas += 1;
                        cambios = true;
                    }
                }
            }
            "Do" => {
                let xobjeto = op.operands.first().and_then(|n| {
                    let tabla = resolver(doc, recursos?.get(b"XObject").ok()?).as_dict().ok()?;
                    match tabla.get(n.as_name().ok()?).ok()? {
                        Object::Reference(id) => Some(*id),
                        _ => None,
                    }
                });
                if let Some(id) = xobjeto
                    && let Ok(Object::Stream(s)) = doc.get_object(id)
                    && s.dict.get(b"Subtype").ok().and_then(|o| o.as_name().ok()) == Some(b"Form")
                {
                    let m = s
                        .dict
                        .get(b"Matrix")
                        .ok()
                        .and_then(|o| resolver(doc, o).as_array().ok())
                        .map(|a| numeros(a))
                        .filter(|v| v.len() == 6)
                        .map_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0], |v| [v[0], v[1], v[2], v[3], v[4], v[5]]);
                    pendientes.push(FormaPendiente { id, ctm: multiplicar(&m, &e.ctm) });
                }
            }
            _ => {}
        }

        // Cambio de color de relleno o de trazo.
        let es_relleno = matches!(nombre.as_str(), "k" | "g" | "rg" | "sc" | "scn" | "cs");
        let es_trazo = matches!(nombre.as_str(), "K" | "G" | "RG" | "SC" | "SCN" | "CS");
        salida.push(op.clone());
        if es_relleno || es_trazo {
            let clase = clase_de(&nombre, &numeros(&op.operands));
            let (actual_op, nuestro, si, no) = if es_relleno {
                (&mut e.op_relleno, &mut e.nuestro_relleno, GS_RELLENO_SI, GS_RELLENO_NO)
            } else {
                (&mut e.op_trazo, &mut e.nuestro_trazo, GS_TRAZO_SI, GS_TRAZO_NO)
            };
            match clase {
                Clase::NegroPuro if corr.sobreimprimir_negro && !*actual_op => {
                    salida.push(gs(si));
                    *actual_op = true;
                    *nuestro = true;
                    conteo.negro_sobreimpreso += 1;
                    cambios = true;
                }
                Clase::Blanco if corr.quitar_sobreimpresion_blanco && *actual_op => {
                    salida.push(gs(no));
                    *actual_op = false;
                    *nuestro = false;
                    conteo.blancos_corregidos += 1;
                    cambios = true;
                }
                Clase::Otro | Clase::Blanco if *nuestro => {
                    // Se apaga la sobreimpresión que pusimos para el negro.
                    salida.push(gs(no));
                    *actual_op = false;
                    *nuestro = false;
                }
                _ => {}
            }
            if es_relleno {
                e.relleno = clase;
            } else {
                e.trazo = clase;
            }
        }
    }
    if !cambios {
        return None;
    }
    Content { operations: salida }.encode().ok()
}

/// Recursos con los estados de sobreimpresión agregados (copia resuelta).
pub fn recursos_con_sobreimpresion(doc: &Document, recursos: Option<&Object>) -> Dictionary {
    let mut d = recursos.and_then(|r| resolver(doc, r).as_dict().ok()).cloned().unwrap_or_default();
    let mut tabla = d.get(b"ExtGState").ok().and_then(|t| resolver(doc, t).as_dict().ok()).cloned().unwrap_or_default();
    for (k, v) in estados_sobreimpresion().iter() {
        tabla.set(k.clone(), v.clone());
    }
    d.set("ExtGState", tabla);
    d
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn corregir(datos: &[u8], corr: &Correcciones) -> (String, Conteo) {
        let doc = Document::with_version("1.6");
        let mut conteo = Conteo::default();
        let mut pendientes = Vec::new();
        let r = corregir_flujo(&doc, datos, None, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], corr, &mut conteo, &mut pendientes);
        (r.map(|b| String::from_utf8(b).unwrap()).unwrap_or_default(), conteo)
    }

    #[test]
    fn sobreimprime_el_negro_y_lo_apaga_al_cambiar_de_color() {
        let (s, c) = corregir(
            b"0 0 0 1 k BT (Hola) Tj ET 1 0 0 0 k 0 0 10 10 re f",
            &Correcciones { sobreimprimir_negro: true, ..Correcciones::ninguna() },
        );
        let si = s.find("/MjOPf1 gs").expect("enciende");
        let no = s.find("/MjOPf0 gs").expect("apaga");
        assert!(si < s.find("Tj").unwrap() && no > s.find("Tj").unwrap());
        assert_eq!(c.negro_sobreimpreso, 1);
    }

    #[test]
    fn no_toca_negro_enriquecido_ni_contenido_sin_problemas() {
        let (s, _) = corregir(b"0.6 0.4 0.4 1 k 0 0 10 10 re f", &Correcciones::recomendadas());
        assert!(s.is_empty(), "sin cambios no se reescribe");
    }

    #[test]
    fn engrosa_lineas_teniendo_en_cuenta_la_escala() {
        // A escala 0,5 una línea de 0,2 queda en 0,1 pt: se lleva a 0,25 / 0,5 = 0,5.
        let (s, c) = corregir(b"0.5 0 0 0.5 0 0 cm 0.2 w 0 0 m 10 10 l S", &Correcciones::recomendadas());
        assert!(s.contains("0.5 w"), "{s}");
        assert_eq!(c.lineas_engrosadas, 1);
        let (s, _) = corregir(b"0 w 0 0 m 10 10 l S", &Correcciones::recomendadas());
        assert!(s.contains("0.25 w"), "{s}");
    }

    #[test]
    fn el_estado_se_restaura_con_q_y_q() {
        let (s, _) = corregir(b"q 0 0 0 1 k 0 0 5 5 re f Q 1 0 0 0 k 0 0 5 5 re f", &Correcciones::recomendadas());
        // Tras Q el gráfico ya no sobreimprime: no hace falta apagar nada.
        assert_eq!(s.matches("/MjOPf0 gs").count(), 0, "{s}");
    }
}
