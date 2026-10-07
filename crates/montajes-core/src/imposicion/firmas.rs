//! Imposición de libros y revistas por firmas (cuadernillos plegados).
//!
//! - **Caballete**: las firmas se anidan una dentro de otra y se engrapan;
//!   lleva desplazamiento (creep) hacia el lomo.
//! - **Al lomo** (PUR, hot-melt): las firmas se alzan una tras otra y se
//!   fresa el lomo; lleva margen de fresado y marcas de alzado.
//! - **Cosido**: firmas alzadas y cosidas por el pliegue, sin fresado.

use serde::{Deserialize, Serialize};

use super::marcas::{self, Linea, OpcionesMarcas};
use super::nup::bloque;
use super::plegado::{Esquema, Lado, Posicion};
use super::{Cara, Margenes, Ubicacion};
use crate::geometria::{Rect, Tamano};
use crate::{Error, Resultado};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Encuadernacion {
    Caballete,
    Lomo,
    Cosido,
}

impl Encuadernacion {
    fn anidada(self) -> bool {
        self == Self::Caballete
    }
}

/// Firmas que se prueban, de mayor a menor.
const TAMANOS_FIRMA: [u32; 5] = [64, 32, 16, 8, 4];
/// Alto de cada escalón de la marca de alzado (mm).
const ESCALON_ALZADO: f64 = 6.0;
const ANCHO_ALZADO: f64 = 3.0;

/// Cuántas firmas se montan en cada pliego.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Aprovechamiento {
    /// Elige lo mejor: tira y retira si caben dos lado a lado, si no repetir.
    #[default]
    Auto,
    /// Una firma por pliego (tiro y retiro con planchas distintas).
    Una,
    /// Todas las copias que quepan, en tiro y retiro.
    Repetir,
    /// Tiro y retiro lado a lado con la misma plancha (work & turn).
    TiraRetira,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParametrosLibro {
    pub pliego: Tamano,
    pub margenes: Margenes,
    /// Formato final de la página.
    pub pagina: Tamano,
    /// Páginas del PDF de entrada.
    pub paginas: u32,
    pub encuadernacion: Encuadernacion,
    /// Páginas por firma; `None` elige la mayor que quepa en el pliego.
    pub firma: Option<u32>,
    pub rebase: f64,
    /// Papel que se come el fresado en el lomo (solo al lomo).
    pub fresado: f64,
    /// Margen de refile en cabeza, pie y frente (nunca menor al rebase).
    pub refile: f64,
    /// Calibre del papel en mm, para el creep (solo caballete).
    pub calibre_mm: Option<f64>,
    /// Libros que se leen de derecha a izquierda (árabe, hebreo, manga).
    pub derecha_a_izquierda: bool,
    pub marcas: OpcionesMarcas,
    #[serde(default)]
    pub aprovechamiento: Aprovechamiento,
    /// Cuadernillos elegidos a mano (p. ej. 16, 16, 8); vacío = automático.
    #[serde(default)]
    pub cuadernillos: Vec<u32>,
    /// Página del PDF (desde 0) para cada página del libro, cuando la tripa
    /// no es el PDF entero (p. ej. trae la carátula). Vacío = en orden.
    #[serde(default)]
    pub mapa: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Firma {
    pub numero: u32,
    pub paginas: u32,
    /// Página del libro (desde 1) para cada página local de la firma.
    pub paginas_libro: Vec<u32>,
    pub girada: bool,
    /// Firmas completas que salen de cada pliego impreso.
    pub copias: u32,
    /// Tiro y retiro en la misma cara: un solo juego de planchas.
    pub tira_retira: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanLibro {
    /// Páginas del libro, ya completadas a múltiplo de 4.
    pub paginas_libro: u32,
    /// Páginas en blanco agregadas al final.
    pub blancas: u32,
    pub firmas: Vec<Firma>,
    /// Dos caras (tiro y retiro) por firma.
    pub caras: Vec<Cara>,
    /// Desplazamiento de la hoja central (caballete).
    pub creep_max: f64,
    /// Pliegos impresos por cada ejemplar del libro.
    pub pliegos_por_ejemplar: f64,
    /// Juegos de planchas (un juego por cara distinta).
    pub juegos_planchas: u32,
    pub avisos: Vec<String>,
}

/// Ancho de cada columna y alto de cada fila del pliego, vistos desde el tiro.
struct Grilla {
    anchos: Vec<f64>,
    altos: Vec<f64>,
}

impl Grilla {
    fn ancho(&self) -> f64 {
        self.anchos.iter().sum()
    }

    fn alto(&self) -> f64 {
        self.altos.iter().sum()
    }
}

/// Márgenes de una página vista desde su lado: (izquierda, derecha, abajo, arriba).
fn margenes_pagina(pos: &Posicion, p: &ParametrosLibro) -> (f64, f64, f64, f64) {
    let lomo = if p.encuadernacion == Encuadernacion::Lomo { p.fresado } else { 0.0 };
    let refile = p.refile.max(p.rebase);
    // Página impar (recto): lomo a la izquierda. Par (verso): a la derecha.
    let (izq, der) = if pos.pagina % 2 == 1 { (lomo, refile) } else { (refile, lomo) };
    if pos.invertida { (der, izq, refile, refile) } else { (izq, der, refile, refile) }
}

fn grilla(e: &Esquema, p: &ParametrosLibro) -> Grilla {
    let mut anchos = vec![0.0_f64; e.columnas as usize];
    let mut altos = vec![0.0_f64; e.filas as usize];
    for pos in &e.posiciones {
        let (izq, der, abajo, arriba) = margenes_pagina(pos, p);
        // Columna física (vista del tiro): el retiro está reflejado.
        let columna = if pos.lado == Lado::Tiro { pos.columna } else { e.columnas - 1 - pos.columna } as usize;
        anchos[columna] = anchos[columna].max(izq + p.pagina.ancho + der);
        altos[pos.fila as usize] = altos[pos.fila as usize].max(abajo + p.pagina.alto + arriba);
    }
    Grilla { anchos, altos }
}

/// `Some(girada)` si la firma cabe en el pliego (prefiere sin girar).
fn cabe(g: &Grilla, p: &ParametrosLibro) -> Option<bool> {
    let area = p.margenes.area_imprimible(p.pliego);
    let borde = 2.0 * p.marcas.espacio_necesario(p.rebase);
    let (w, h) = (g.ancho() + borde, g.alto() + borde);
    if w <= area.ancho + 1e-6 && h <= area.alto + 1e-6 {
        Some(false)
    } else if h <= area.ancho + 1e-6 && w <= area.alto + 1e-6 {
        Some(true)
    } else {
        None
    }
}

/// Área imprimible en el pliego «virtual» (orientado como el libro).
fn area_virtual(p: &ParametrosLibro, girada: bool) -> (f64, f64, Rect) {
    let a = p.margenes.area_imprimible(p.pliego);
    if girada {
        (p.pliego.alto, p.pliego.ancho, Rect::new(p.pliego.alto - a.y - a.alto, a.x, a.alto, a.ancho))
    } else {
        (p.pliego.ancho, p.pliego.alto, a)
    }
}

/// Cómo se monta una firma en el pliego.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Montaje {
    girada: bool,
    columnas: u32,
    filas: u32,
    tira_retira: bool,
}

impl Montaje {
    fn copias(&self) -> u32 {
        self.columnas * self.filas
    }
}

fn montaje(g: &Grilla, p: &ParametrosLibro, girada: bool) -> (Montaje, Option<String>) {
    let (_, _, area) = area_virtual(p, girada);
    let borde = 2.0 * p.marcas.espacio_necesario(p.rebase);
    let columnas = (((area.ancho - borde + 1e-6) / g.ancho()).floor() as u32).max(1);
    let filas = (((area.alto - borde + 1e-6) / g.alto()).floor() as u32).max(1);
    let una = Montaje { girada, columnas: 1, filas: 1, tira_retira: false };
    let repetir = Montaje { girada, columnas, filas, tira_retira: false };
    // Tira y retira: las copias van de a pares lado a lado (el pliego se voltea de lado).
    let pares = Montaje { girada, columnas: columnas - columnas % 2, filas, tira_retira: true };
    let puede_tr = !girada && columnas >= 2;
    match p.aprovechamiento {
        Aprovechamiento::Una => (una, None),
        Aprovechamiento::Repetir => (repetir, None),
        Aprovechamiento::TiraRetira if puede_tr => (pares, None),
        Aprovechamiento::TiraRetira => {
            (repetir, Some("la firma no cabe dos veces lado a lado: se monta repetida en tiro y retiro".into()))
        }
        Aprovechamiento::Auto if puede_tr => (pares, None),
        Aprovechamiento::Auto => (if repetir.copias() > 1 { repetir } else { una }, None),
    }
}

/// Reparte las páginas en firmas: las grandes primero y el resto en firmas
/// menores (al final si son alzadas, al centro si son anidadas).
fn repartir(total: u32, mayor: u32) -> Vec<u32> {
    let mut firmas = vec![mayor; (total / mayor) as usize];
    let mut resto = total % mayor;
    for t in TAMANOS_FIRMA.iter().filter(|t| **t < mayor) {
        while resto >= *t {
            firmas.push(*t);
            resto -= t;
        }
    }
    firmas
}

/// Página del libro para cada página local de cada firma.
fn numerar(firmas: &[u32], total: u32, anidada: bool) -> Vec<Vec<u32>> {
    let mut inicio = 0;
    firmas
        .iter()
        .map(|&n| {
            let paginas = (1..=n)
                .map(|i| {
                    if anidada && i > n / 2 {
                        // Segunda mitad de una firma anidada: cuenta desde el final del libro.
                        total - inicio - (n - i)
                    } else {
                        inicio + i
                    }
                })
                .collect();
            inicio += if anidada { n / 2 } else { n };
            paginas
        })
        .collect()
}

pub fn planificar(p: &ParametrosLibro) -> Resultado<PlanLibro> {
    if p.paginas == 0 {
        return Err(Error::Invalido("el libro no tiene páginas".into()));
    }
    if !p.mapa.is_empty() && p.mapa.len() != p.paginas as usize {
        return Err(Error::Invalido("el mapa de páginas no coincide con las páginas de la tripa".into()));
    }
    let manual: u32 = p.cuadernillos.iter().sum();
    if !p.cuadernillos.is_empty() {
        if let Some(n) = p.cuadernillos.iter().find(|n| !TAMANOS_FIRMA.contains(n)) {
            return Err(Error::Invalido(format!("cuadernillo de {n} páginas: use 4, 8, 16, 32 o 64")));
        }
        if manual < p.paginas {
            return Err(Error::Invalido(format!(
                "los cuadernillos suman {manual} páginas y la tripa tiene {}; faltan {}",
                p.paginas,
                p.paginas - manual
            )));
        }
    }
    let paginas_libro = if p.cuadernillos.is_empty() { p.paginas.div_ceil(4) * 4 } else { manual };
    let blancas = paginas_libro - p.paginas;
    let mut avisos = Vec::new();
    if blancas > 0 {
        avisos.push(format!("se agregan {blancas} páginas en blanco al final para completar los cuadernillos"));
    }

    let mut esquemas = Vec::new();
    for n in TAMANOS_FIRMA {
        let e = Esquema::estandar(n)?;
        if let Some(girada) = cabe(&grilla(&e, p), p) {
            esquemas.push((n, e, girada));
        }
    }
    let mayor =
        match p.firma {
            Some(n) => {
                if !esquemas.iter().any(|(t, _, _)| *t == n) {
                    return Err(Error::NoCabe(format!("firma de {n} páginas de {} en pliego {}", p.pagina, p.pliego)));
                }
                n
            }
            None => esquemas.iter().map(|(t, _, _)| *t).find(|t| *t <= paginas_libro).ok_or_else(|| {
                Error::NoCabe(format!("ni una firma de 4 páginas de {} cabe en {}", p.pagina, p.pliego))
            })?,
        };
    let tamanos = if p.cuadernillos.is_empty() { repartir(paginas_libro, mayor) } else { p.cuadernillos.clone() };
    let numeracion = numerar(&tamanos, paginas_libro, p.encuadernacion.anidada());

    let creep = |pagina: u32| -> f64 {
        match (p.encuadernacion.anidada(), p.calibre_mm) {
            (true, Some(calibre)) => {
                let hoja = pagina.div_ceil(2);
                let hojas = paginas_libro / 2;
                f64::from(hoja.min(hojas + 1 - hoja) - 1) * calibre
            }
            _ => 0.0,
        }
    };
    let creep_max = creep(paginas_libro / 2);
    if p.encuadernacion.anidada() && p.calibre_mm.is_none() {
        avisos.push("sin calibre de papel: no se aplica creep".into());
    }
    if p.encuadernacion.anidada() && tamanos.len() > 1 {
        avisos.push(format!(
            "caballete con {} firmas anidadas; verifique el grosor máximo de la grapadora",
            tamanos.len()
        ));
    }

    let mut firmas = Vec::new();
    let mut caras = Vec::new();
    let mut pliegos_por_ejemplar = 0.0;
    let mut juegos_planchas = 0;
    for (i, (n, paginas_libro_firma)) in tamanos.iter().zip(numeracion).enumerate() {
        let (_, esquema, girada) = esquemas
            .iter()
            .find(|(t, _, _)| t == n)
            .ok_or_else(|| Error::NoCabe(format!("firma de {n} páginas de {} en pliego {}", p.pagina, p.pliego)))?;
        let numero = i as u32 + 1;
        let (m, aviso) = montaje(&grilla(esquema, p), p, *girada);
        if let Some(a) = aviso
            && !avisos.contains(&a)
        {
            avisos.push(a);
        }
        caras.extend(caras_firma(p, esquema, &m, &paginas_libro_firma, numero, &creep));
        pliegos_por_ejemplar += 1.0 / f64::from(m.copias());
        juegos_planchas += if m.tira_retira { 1 } else { 2 };
        firmas.push(Firma {
            numero,
            paginas: *n,
            paginas_libro: paginas_libro_firma,
            girada: *girada,
            copias: m.copias(),
            tira_retira: m.tira_retira,
        });
    }
    Ok(PlanLibro { paginas_libro, blancas, firmas, caras, creep_max, pliegos_por_ejemplar, juegos_planchas, avisos })
}

fn caras_firma(
    p: &ParametrosLibro,
    e: &Esquema,
    m: &Montaje,
    paginas_libro: &[u32],
    numero: u32,
    creep: &dyn Fn(u32) -> f64,
) -> Vec<Cara> {
    let g = grilla(e, p);
    let girada = m.girada;
    // Todo se calcula en un pliego «virtual» orientado como el libro y, si
    // la firma va girada, se lleva al pliego real al final.
    let area_real = p.margenes.area_imprimible(p.pliego);
    let (ancho_v, alto_v, area) = area_virtual(p, girada);
    // Copias centradas en el área; en tira y retira solo la mitad izquierda
    // lleva el tiro: el retiro reflejado cae en la mitad derecha.
    let bx = area.x + (area.ancho - f64::from(m.columnas) * g.ancho()) / 2.0;
    let by = area.y + (area.alto - f64::from(m.filas) * g.alto()) / 2.0;
    let columnas_tiro = if m.tira_retira { m.columnas / 2 } else { m.columnas };
    let origenes: Vec<(f64, f64)> = (0..m.filas)
        .flat_map(|f| (0..columnas_tiro).map(move |c| (c, f)))
        .map(|(c, f)| (bx + f64::from(c) * g.ancho(), by + f64::from(f) * g.alto()))
        .collect();

    struct Pieza {
        lado: Lado,
        celda: Rect,
        corte: Rect,
        ubicacion: Option<Ubicacion>,
    }
    let mut piezas = Vec::with_capacity(e.posiciones.len() * origenes.len());
    let mut alzados = Vec::new();
    for &(x0, y0) in &origenes {
        let celda_fisica = |columna: usize, fila: usize| {
            Rect::new(
                x0 + g.anchos[..columna].iter().sum::<f64>(),
                y0 + g.altos[..fila].iter().sum::<f64>(),
                g.anchos[columna],
                g.altos[fila],
            )
        };

        for pos in &e.posiciones {
            let fisica = if pos.lado == Lado::Tiro { pos.columna } else { e.columnas - 1 - pos.columna } as usize;
            let mut celda = celda_fisica(fisica, pos.fila as usize);
            if pos.lado == Lado::Retiro {
                celda = celda.reflejar_x(ancho_v);
            }
            let (izq, _, abajo, _) = margenes_pagina(pos, p);
            let corte = Rect::new(celda.x + izq, celda.y + abajo, p.pagina.ancho, p.pagina.alto);
            let recto = pos.pagina % 2 == 1;
            let lomo_a_la_izquierda = recto != pos.invertida;
            let pagina = paginas_libro[pos.pagina as usize - 1];

            let ubicacion = (pagina <= p.paginas).then(|| {
                let desplazamiento = creep(pagina);
                let mut corrido = corte;
                corrido.x += if lomo_a_la_izquierda { -desplazamiento } else { desplazamiento };
                Ubicacion {
                    pagina: p.mapa.get(pagina as usize - 1).copied().unwrap_or(pagina as usize - 1),
                    corte: corrido,
                    giro: if pos.invertida { 180 } else { 0 },
                    recorte: corrido.expandir(p.rebase, p.rebase, p.rebase, p.rebase).interseccion(&celda),
                }
            });

            // Marca de alzado: en el pliegue del lomo junto a la primera página
            // de la firma, bajando un escalón por cada firma.
            if pos.pagina == 1 && !p.encuadernacion.anidada() {
                let pasos = ((p.pagina.alto - ESCALON_ALZADO) / ESCALON_ALZADO).floor().max(1.0) as u32;
                let escalon = f64::from((numero - 1) % pasos) * ESCALON_ALZADO;
                let pliegue = if lomo_a_la_izquierda { celda.x } else { celda.derecha() };
                let y = if pos.invertida { corte.y + escalon } else { corte.arriba() - escalon - ESCALON_ALZADO };
                alzados.push((pos.lado, Rect::new(pliegue - ANCHO_ALZADO / 2.0, y, ANCHO_ALZADO, ESCALON_ALZADO)));
            }
            piezas.push(Pieza { lado: pos.lado, celda, corte, ubicacion });
        }
    }

    // Lectura de derecha a izquierda: el mismo pliego reflejado.
    let transformar = |lado: Lado, r: Rect| {
        let r = if p.derecha_a_izquierda { r.reflejar_x(ancho_v) } else { r };
        match (girada, lado) {
            (false, _) => r,
            // Tiro en sentido horario y retiro antihorario: así el retiro
            // sigue cayendo detrás de su tiro al voltear el pliego de lado.
            (true, Lado::Tiro) => r.girar_horario(ancho_v),
            (true, Lado::Retiro) => r.girar_antihorario(alto_v),
        }
    };
    let giro_extra = |lado: Lado| match (girada, lado) {
        (false, _) => 0,
        (true, Lado::Tiro) => 90,
        (true, Lado::Retiro) => 270,
    };

    // Cada cara junta las piezas de los lados que lleva: en tira y retira,
    // tiro y retiro van en la misma cara (y la misma plancha).
    let armar = |lados: &[Lado], nombre: String| {
        let propias: Vec<&Pieza> = piezas.iter().filter(|x| lados.contains(&x.lado)).collect();
        let cortes: Vec<Rect> = propias.iter().map(|x| transformar(x.lado, x.corte)).collect();
        let celdas: Vec<Rect> = propias.iter().map(|x| transformar(x.lado, x.celda)).collect();
        let ubicaciones = propias
            .iter()
            .filter_map(|x| x.ubicacion.as_ref().map(|u| (x.lado, u)))
            .map(|(lado, u)| Ubicacion {
                pagina: u.pagina,
                corte: transformar(lado, u.corte),
                recorte: transformar(lado, u.recorte),
                giro: (u.giro + giro_extra(lado)) % 360,
            })
            .collect();
        let mut marcas = marcas::generar(&cortes, p.pliego, &area_real, p.rebase, &p.marcas);
        if p.marcas.corte {
            marcas.pliegues = marcas_plegado(&celdas, &cortes, p.rebase, &p.marcas);
            marcas.alzado =
                alzados.iter().filter(|(l, _)| lados.contains(l)).map(|(l, r)| transformar(*l, *r)).collect();
        }
        Cara { nombre, pliego: p.pliego, ubicaciones, marcas, cajas: None }
    };
    if m.tira_retira {
        vec![armar(&[Lado::Tiro, Lado::Retiro], format!("Firma {numero} · tira y retira"))]
    } else {
        vec![
            armar(&[Lado::Tiro], format!("Firma {numero} tiro")),
            armar(&[Lado::Retiro], format!("Firma {numero} retiro")),
        ]
    }
}

/// Marcas de plegado: en cada borde interior entre celdas, fuera del bloque.
fn marcas_plegado(celdas: &[Rect], cortes: &[Rect], rebase: f64, op: &OpcionesMarcas) -> Vec<Linea> {
    let b = bloque(celdas);
    let c = bloque(cortes);
    let d = op.distancia(rebase);
    let interior = |v: f64, min: f64, max: f64| v > min + 1e-6 && v < max - 1e-6;
    let mut xs: Vec<f64> =
        celdas.iter().flat_map(|r| [r.x, r.derecha()]).filter(|&x| interior(x, b.x, b.derecha())).collect();
    let mut ys: Vec<f64> =
        celdas.iter().flat_map(|r| [r.y, r.arriba()]).filter(|&y| interior(y, b.y, b.arriba())).collect();
    for v in [&mut xs, &mut ys] {
        v.sort_by(f64::total_cmp);
        v.dedup_by(|a, b| (*a - *b).abs() < 1e-3);
    }
    let mut lineas = Vec::new();
    for x in xs {
        lineas.push(Linea { x1: x, y1: c.y - d - op.largo, x2: x, y2: c.y - d });
        lineas.push(Linea { x1: x, y1: c.arriba() + d, x2: x, y2: c.arriba() + d + op.largo });
    }
    for y in ys {
        lineas.push(Linea { x1: c.x - d - op.largo, y1: y, x2: c.x - d, y2: y });
        lineas.push(Linea { x1: c.derecha() + d, y1: y, x2: c.derecha() + d + op.largo, y2: y });
    }
    lineas
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn libro(paginas: u32, encuadernacion: Encuadernacion) -> ParametrosLibro {
        ParametrosLibro {
            pliego: Tamano::new(720.0, 520.0),
            margenes: Margenes { pinza: 10.0, cola: 6.0, lateral: 5.0 },
            pagina: Tamano::new(148.0, 210.0),
            paginas,
            encuadernacion,
            firma: None,
            rebase: 3.0,
            fresado: 3.0,
            refile: 3.0,
            calibre_mm: Some(0.1),
            derecha_a_izquierda: false,
            marcas: OpcionesMarcas::default(),
            aprovechamiento: Aprovechamiento::Una,
            cuadernillos: vec![],
            mapa: vec![],
        }
    }

    #[test]
    fn reparto_de_firmas() {
        assert_eq!(repartir(48, 16), [16, 16, 16]);
        assert_eq!(repartir(44, 16), [16, 16, 8, 4]);
        assert_eq!(repartir(4, 16), [4]);
    }

    #[test]
    fn numeracion_alzada_y_anidada() {
        let alzada = numerar(&[8, 8], 16, false);
        assert_eq!(alzada[1], [9, 10, 11, 12, 13, 14, 15, 16]);
        let anidada = numerar(&[8, 8], 16, true);
        assert_eq!(anidada[0], [1, 2, 3, 4, 13, 14, 15, 16]);
        assert_eq!(anidada[1], [5, 6, 7, 8, 9, 10, 11, 12]);
    }

    #[test]
    fn a5_al_lomo_en_70x50_usa_firmas_de_16() {
        // A5 + 3 mm fresado + 3 mm refile → celdas de 154 × 216 mm: 4 × 2 caben en 710 × 504.
        let plan = planificar(&libro(160, Encuadernacion::Lomo)).unwrap();
        assert_eq!(plan.firmas.len(), 10);
        assert!(plan.firmas.iter().all(|f| f.paginas == 16 && !f.girada));
        assert_eq!(plan.caras.len(), 20);
        assert!(plan.caras.iter().all(|c| c.ubicaciones.len() == 8));
        // Una marca de alzado por firma, bajando un escalón cada vez.
        let alzados: Vec<Rect> = plan.caras.iter().flat_map(|c| c.marcas.alzado.clone()).collect();
        assert_eq!(alzados.len(), 10);
        assert!((alzados[0].y - alzados[1].y - ESCALON_ALZADO).abs() < 1e-9);
    }

    #[test]
    fn paginas_en_blanco_y_creep_de_caballete() {
        let plan = planificar(&libro(30, Encuadernacion::Caballete)).unwrap();
        assert_eq!((plan.paginas_libro, plan.blancas), (32, 2));
        // 32 pp = 16 hojas a caballete: la central se corre 7 × 0,1 mm.
        assert!((plan.creep_max - 0.7).abs() < 1e-9);
        let ubicadas: usize = plan.caras.iter().map(|c| c.ubicaciones.len()).sum();
        assert_eq!(ubicadas, 30);
        assert!(plan.caras.iter().all(|c| c.marcas.alzado.is_empty()));
    }

    #[test]
    fn cada_pagina_del_libro_aparece_una_vez() {
        for enc in [Encuadernacion::Caballete, Encuadernacion::Lomo, Encuadernacion::Cosido] {
            let plan = planificar(&libro(88, enc)).unwrap();
            let mut paginas: Vec<usize> =
                plan.caras.iter().flat_map(|c| c.ubicaciones.iter().map(|u| u.pagina)).collect();
            paginas.sort_unstable();
            assert_eq!(paginas, (0..88).collect::<Vec<_>>(), "{enc:?}");
        }
    }

    #[test]
    fn el_retiro_queda_detras_de_su_tiro() {
        // Las dos páginas de una hoja ocupan la misma celda física.
        for girar in [false, true] {
            let mut p = libro(16, Encuadernacion::Lomo);
            if girar {
                p.pliego = Tamano::new(520.0, 720.0);
                p.margenes = Margenes { pinza: 5.0, cola: 5.0, lateral: 5.0 };
            }
            let plan = planificar(&p).unwrap();
            assert_eq!(plan.firmas[0].girada, girar);
            let (tiro, retiro) = (&plan.caras[0], &plan.caras[1]);
            for u in &tiro.ubicaciones {
                let pareja = if u.pagina % 2 == 0 { u.pagina + 1 } else { u.pagina - 1 };
                let v = retiro.ubicaciones.iter().find(|v| v.pagina == pareja).unwrap();
                let centro_tiro = u.corte.x + u.corte.ancho / 2.0;
                let centro_retiro = v.corte.x + v.corte.ancho / 2.0;
                // Las páginas recto y verso comparten celda; el fresado las corre
                // en sentidos opuestos, así que se compara contra la celda.
                assert!((centro_tiro + centro_retiro - p.pliego.ancho).abs() < 2.0 * p.fresado + 1e-6);
                assert!(
                    (u.corte.y + u.corte.alto / 2.0 - (v.corte.y + v.corte.alto / 2.0)).abs() < 2.0 * p.fresado + 1e-6
                );
            }
        }
    }

    #[test]
    fn tira_y_retira_con_firmas_pequenas() {
        // A5 de 8 pp (bloque 308 × 432) cabe dos veces lado a lado en 720 × 520.
        let mut p = libro(16, Encuadernacion::Lomo);
        p.firma = Some(8);
        p.aprovechamiento = Aprovechamiento::Auto;
        let plan = planificar(&p).unwrap();
        assert_eq!(plan.firmas.len(), 2);
        assert!(plan.firmas.iter().all(|f| f.tira_retira && f.copias == 2));
        // Una sola cara por firma, con las 8 páginas (tiro a la izquierda, retiro a la derecha).
        assert_eq!(plan.caras.len(), 2);
        let cara = &plan.caras[0];
        let mut paginas: Vec<usize> = cara.ubicaciones.iter().map(|u| u.pagina).collect();
        paginas.sort_unstable();
        assert_eq!(paginas, (0..8).collect::<Vec<_>>());
        let centro = p.pliego.ancho / 2.0;
        // La página 1 (retiro exterior) y la 2 (tiro interior) quedan en mitades opuestas.
        let x = |pag: usize| cara.ubicaciones.iter().find(|u| u.pagina == pag).unwrap().corte.x;
        assert!((x(0) - centro) * (x(1) - centro) < 0.0);
        assert_eq!(plan.juegos_planchas, 2);
        assert!((plan.pliegos_por_ejemplar - 1.0).abs() < 1e-9);
    }

    #[test]
    fn repetir_firmas_en_tiro_y_retiro() {
        let mut p = libro(8, Encuadernacion::Caballete);
        p.firma = Some(4);
        p.aprovechamiento = Aprovechamiento::Repetir;
        let plan = planificar(&p).unwrap();
        // 4 pp A5 = bloque 302 × 216: 2 columnas × 2 filas.
        assert!(plan.firmas.iter().all(|f| f.copias == 4 && !f.tira_retira));
        assert_eq!(plan.caras.len(), 4);
        assert!(plan.caras.iter().all(|c| c.ubicaciones.len() == 8));
        assert_eq!(plan.juegos_planchas, 4);
    }

    #[test]
    fn cuadernillos_a_mano_y_mapa_de_paginas() {
        // PDF de 44 páginas: 1–2 y 43–44 son la carátula; la tripa es 3–42 (40 páginas).
        let mut p = libro(40, Encuadernacion::Lomo);
        p.cuadernillos = vec![16, 16, 8];
        p.mapa = (2..42).collect();
        let plan = planificar(&p).unwrap();
        assert_eq!(plan.firmas.iter().map(|f| f.paginas).collect::<Vec<_>>(), [16, 16, 8]);
        let mut usadas: Vec<usize> = plan.caras.iter().flat_map(|c| c.ubicaciones.iter().map(|u| u.pagina)).collect();
        usadas.sort_unstable();
        assert_eq!(usadas, (2..42).collect::<Vec<_>>());
        // Si los cuadernillos no alcanzan, error claro; si sobran, páginas en blanco.
        p.cuadernillos = vec![16, 16];
        assert!(planificar(&p).is_err());
        p.cuadernillos = vec![16, 16, 16];
        assert_eq!(planificar(&p).unwrap().blancas, 8);
        p.cuadernillos = vec![16, 12, 12];
        assert!(planificar(&p).is_err());
    }

    #[test]
    fn firma_forzada_que_no_cabe() {
        let mut p = libro(64, Encuadernacion::Lomo);
        p.firma = Some(64);
        assert!(matches!(planificar(&p), Err(Error::NoCabe(_))));
    }
}
