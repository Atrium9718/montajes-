//! Geometría de la portada: tapa, lomo, contratapa y solapas (rústica) o
//! forro de tapa dura (cartoné) con bisagras y vueltas.
//!
//! Las medidas se dan sobre la portada extendida, vista desde afuera, con
//! el origen en la esquina inferior izquierda del formato final (sin rebase).

use serde::{Deserialize, Serialize};

use crate::geometria::{Rect, Tamano};
use crate::imposicion::marcas::{self, Linea, OpcionesMarcas};
use crate::imposicion::{Cajas, Cara, Ubicacion};
use crate::{Error, Resultado};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TipoPortada {
    /// Rústica (PUR, hot-melt, cosida o caballete), con solapas opcionales.
    Rustica {
        /// Ancho de cada solapa en mm; 0 = sin solapas.
        solapa: f64,
    },
    /// Tapa dura: el forro envuelve dos cartones y el lomo.
    TapaDura {
        /// Grosor del cartón (mm).
        carton: f64,
        /// Cuánto sobresale el cartón del bloque en cabeza, pie y frente (mm).
        escuadra: f64,
        /// Papel que se dobla hacia adentro del cartón (mm).
        vuelta: f64,
        /// Canal entre el cartón y el lomo (mm).
        bisagra: f64,
    },
}

impl TipoPortada {
    pub fn tapa_dura_estandar() -> Self {
        Self::TapaDura { carton: 2.5, escuadra: 3.0, vuelta: 15.0, bisagra: 8.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TipoPanel {
    SolapaContratapa,
    Contratapa,
    Bisagra,
    Lomo,
    Tapa,
    SolapaTapa,
    /// Papel que se dobla sobre el cartón (tapa dura).
    Vuelta,
}

impl TipoPanel {
    pub fn nombre(&self) -> &'static str {
        match self {
            Self::SolapaContratapa => "SOLAPA CONTRATAPA",
            Self::Contratapa => "CONTRATAPA",
            Self::Bisagra => "BISAGRA",
            Self::Lomo => "LOMO",
            Self::Tapa => "TAPA",
            Self::SolapaTapa => "SOLAPA TAPA",
            Self::Vuelta => "VUELTA",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Panel {
    pub tipo: TipoPanel,
    pub rect: Rect,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParametrosPortada {
    /// Formato final de las páginas de la tripa.
    pub pagina: Tamano,
    /// Grosor del lomo del bloque (mm), ya calculado con el papel.
    pub lomo_bloque: f64,
    pub tipo: TipoPortada,
    pub rebase: f64,
    pub derecha_a_izquierda: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Portada {
    /// Formato final de la portada extendida (en tapa dura incluye las vueltas).
    pub tamano: Tamano,
    pub rebase: f64,
    /// Ancho del panel del lomo.
    pub lomo: f64,
    /// Paneles de izquierda a derecha, vistos desde afuera.
    pub paneles: Vec<Panel>,
    /// Posiciones x de los pliegues o hendidos verticales.
    pub pliegues: Vec<f64>,
    /// Posiciones y de los pliegues horizontales (vueltas de tapa dura).
    #[serde(default)]
    pub pliegues_horizontales: Vec<f64>,
    /// Zona de seguridad para textos dentro de cada panel (mm).
    pub seguridad: f64,
}

impl Portada {
    pub fn panel(&self, tipo: TipoPanel) -> Option<&Panel> {
        self.paneles.iter().find(|p| p.tipo == tipo)
    }

    /// Página completa con rebase.
    pub fn tamano_con_rebase(&self) -> Tamano {
        Tamano::new(self.tamano.ancho + 2.0 * self.rebase, self.tamano.alto + 2.0 * self.rebase)
    }
}

pub fn calcular(p: &ParametrosPortada) -> Resultado<Portada> {
    if p.lomo_bloque < 0.0 || p.rebase < 0.0 {
        return Err(Error::Invalido("lomo y rebase no pueden ser negativos".into()));
    }
    // Se arma de izquierda a derecha en lectura occidental: la contratapa a
    // la izquierda y la tapa a la derecha. En derecha a izquierda se invierte.
    let mut tramos: Vec<(TipoPanel, f64)> = Vec::new();
    let (alto, margen_vertical, lomo, seguridad) = match p.tipo {
        TipoPortada::Rustica { solapa } => {
            if solapa < 0.0 {
                return Err(Error::Invalido("la solapa no puede ser negativa".into()));
            }
            if solapa > 0.0 {
                tramos.push((TipoPanel::SolapaContratapa, solapa));
            }
            tramos.push((TipoPanel::Contratapa, p.pagina.ancho));
            tramos.push((TipoPanel::Lomo, p.lomo_bloque));
            tramos.push((TipoPanel::Tapa, p.pagina.ancho));
            if solapa > 0.0 {
                tramos.push((TipoPanel::SolapaTapa, solapa));
            }
            (p.pagina.alto, 0.0, p.lomo_bloque, 5.0)
        }
        TipoPortada::TapaDura { carton, escuadra, vuelta, bisagra } => {
            if [carton, escuadra, vuelta, bisagra].iter().any(|v| *v < 0.0) {
                return Err(Error::Invalido("las medidas de tapa dura no pueden ser negativas".into()));
            }
            // El cartón cubre la página más la escuadra del frente; el lomo
            // rígido abarca el bloque y el grosor de ambos cartones.
            let ancho_carton = p.pagina.ancho + escuadra;
            let lomo = p.lomo_bloque + 2.0 * carton;
            tramos.push((TipoPanel::Vuelta, vuelta));
            tramos.push((TipoPanel::Contratapa, ancho_carton));
            tramos.push((TipoPanel::Bisagra, bisagra));
            tramos.push((TipoPanel::Lomo, lomo));
            tramos.push((TipoPanel::Bisagra, bisagra));
            tramos.push((TipoPanel::Tapa, ancho_carton));
            tramos.push((TipoPanel::Vuelta, vuelta));
            (p.pagina.alto + 2.0 * escuadra, vuelta, lomo, 5.0 + carton)
        }
    };
    if p.derecha_a_izquierda {
        tramos.reverse();
    }

    let mut paneles = Vec::with_capacity(tramos.len());
    let mut pliegues = Vec::new();
    let mut x = 0.0;
    for (tipo, ancho) in tramos {
        if x > 0.0 {
            pliegues.push(x);
        }
        paneles.push(Panel { tipo, rect: Rect::new(x, margen_vertical, ancho, alto) });
        x += ancho;
    }
    let ancho_total = x;
    let alto_total = alto + 2.0 * margen_vertical;
    // En tapa dura las vueltas también rodean cabeza y pie.
    if margen_vertical > 0.0 {
        for y in [0.0, alto_total - margen_vertical] {
            paneles.push(Panel { tipo: TipoPanel::Vuelta, rect: Rect::new(0.0, y, ancho_total, margen_vertical) });
        }
        for panel in &mut paneles {
            if panel.tipo == TipoPanel::Vuelta && panel.rect.alto == alto {
                panel.rect = Rect::new(panel.rect.x, 0.0, panel.rect.ancho, alto_total);
            }
        }
    }
    Ok(Portada {
        tamano: Tamano::new(ancho_total, alto_total),
        rebase: p.rebase,
        lomo,
        paneles,
        pliegues,
        pliegues_horizontales: if margen_vertical > 0.0 {
            vec![margen_vertical, alto_total - margen_vertical]
        } else {
            vec![]
        },
        seguridad,
    })
}

/// Página de portada con espacio para marcas alrededor del rebase.
fn cara_base(c: &Portada, op: &OpcionesMarcas) -> (Cara, Rect) {
    let mut op = op.clone();
    op.tira_color = false;
    let margen = if op.corte || op.registro { op.espacio_necesario(c.rebase) + 2.0 } else { c.rebase };
    let pliego = Tamano::new(c.tamano.ancho + 2.0 * margen, c.tamano.alto + 2.0 * margen);
    let corte = Rect::new(margen, margen, c.tamano.ancho, c.tamano.alto);
    let todo = Rect::new(0.0, 0.0, pliego.ancho, pliego.alto);
    let mut m = marcas::generar(&[corte], pliego, &todo, c.rebase, &op);
    if op.corte {
        let d = op.distancia(c.rebase);
        for &x in &c.pliegues {
            let x = corte.x + x;
            m.pliegues.push(Linea { x1: x, y1: corte.y - d - op.largo, x2: x, y2: corte.y - d });
            m.pliegues.push(Linea { x1: x, y1: corte.arriba() + d, x2: x, y2: corte.arriba() + d + op.largo });
        }
        for &y in &c.pliegues_horizontales {
            let y = corte.y + y;
            m.pliegues.push(Linea { x1: corte.x - d - op.largo, y1: y, x2: corte.x - d, y2: y });
            m.pliegues.push(Linea { x1: corte.derecha() + d, y1: y, x2: corte.derecha() + d + op.largo, y2: y });
        }
    }
    let r = c.rebase;
    let cara = Cara {
        nombre: "Portada".into(),
        pliego,
        ubicaciones: vec![],
        marcas: m,
        cajas: Some(Cajas { corte, sangrado: corte.expandir(r, r, r, r) }),
    };
    (cara, corte)
}

/// Portada diseñada en una sola página (tapa, lomo y contratapa juntas).
pub fn cara_completa(c: &Portada, pagina: usize, op: &OpcionesMarcas) -> Cara {
    let (mut cara, corte) = cara_base(c, op);
    let r = c.rebase;
    cara.ubicaciones.push(Ubicacion { pagina, corte, giro: 0, recorte: corte.expandir(r, r, r, r) });
    cara
}

/// Portada armada a partir de páginas sueltas, una por panel. Cada página
/// debe medir lo mismo que su panel; el rebase solo se usa en los bordes
/// exteriores de la portada.
pub fn cara_armada(c: &Portada, asignacion: &[(TipoPanel, usize, Tamano)], op: &OpcionesMarcas) -> Resultado<Cara> {
    let (mut cara, corte) = cara_base(c, op);
    let eps = 1e-6;
    for &(tipo, pagina, tamano) in asignacion {
        let panel = c
            .paneles
            .iter()
            .find(|p| p.tipo == tipo)
            .ok_or_else(|| Error::Invalido(format!("esta portada no tiene {}", tipo.nombre())))?;
        let r = panel.rect;
        if (tamano.ancho - r.ancho).abs() > 0.5 || (tamano.alto - r.alto).abs() > 0.5 {
            return Err(Error::Invalido(format!(
                "la página para {} mide {tamano} y el panel {}",
                tipo.nombre(),
                Tamano::new(r.ancho, r.alto).redondeado()
            )));
        }
        let destino = Rect::new(corte.x + r.x, corte.y + r.y, r.ancho, r.alto);
        let lado = |borde: bool| if borde { c.rebase } else { 0.0 };
        let recorte = destino.expandir(
            lado(r.x < eps),
            lado(r.y < eps),
            lado(r.derecha() > c.tamano.ancho - eps),
            lado(r.arriba() > c.tamano.alto - eps),
        );
        cara.ubicaciones.push(Ubicacion { pagina, corte: destino, giro: 0, recorte });
    }
    Ok(cara)
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn rustica(solapa: f64) -> ParametrosPortada {
        ParametrosPortada {
            pagina: Tamano::new(148.0, 210.0),
            lomo_bloque: 12.8,
            tipo: TipoPortada::Rustica { solapa },
            rebase: 3.0,
            derecha_a_izquierda: false,
        }
    }

    #[test]
    fn rustica_sin_solapas() {
        let c = calcular(&rustica(0.0)).unwrap();
        assert!((c.tamano.ancho - (148.0 * 2.0 + 12.8)).abs() < 1e-9);
        assert_eq!(c.tamano.alto, 210.0);
        let lomo = c.panel(TipoPanel::Lomo).unwrap().rect;
        assert_eq!(lomo.x, 148.0);
        assert_eq!(c.pliegues, vec![148.0, 160.8]);
        assert_eq!(c.tamano_con_rebase(), Tamano::new(314.8, 216.0));
    }

    #[test]
    fn rustica_con_solapas_y_derecha_a_izquierda() {
        let mut p = rustica(80.0);
        let c = calcular(&p).unwrap();
        assert!((c.tamano.ancho - (80.0 * 2.0 + 148.0 * 2.0 + 12.8)).abs() < 1e-9);
        assert_eq!(c.pliegues.len(), 4);
        assert_eq!(c.paneles.last().unwrap().tipo, TipoPanel::SolapaTapa);
        p.derecha_a_izquierda = true;
        let c = calcular(&p).unwrap();
        assert_eq!(c.paneles[0].tipo, TipoPanel::SolapaTapa);
        assert_eq!(c.paneles[1].tipo, TipoPanel::Tapa);
    }

    #[test]
    fn tapa_dura() {
        let mut p = rustica(0.0);
        p.tipo = TipoPortada::tapa_dura_estandar();
        let c = calcular(&p).unwrap();
        // Lomo rígido: 12,8 + 2 × 2,5 = 17,8. Cartón: 148 + 3 = 151.
        assert!((c.lomo - 17.8).abs() < 1e-9);
        let ancho = 15.0 * 2.0 + 151.0 * 2.0 + 8.0 * 2.0 + 17.8;
        assert!((c.tamano.ancho - ancho).abs() < 1e-9);
        assert_eq!(c.tamano.alto, 210.0 + 6.0 + 30.0);
        let tapa = c.panel(TipoPanel::Tapa).unwrap().rect;
        assert_eq!((tapa.y, tapa.alto), (15.0, 216.0));
        // Pliegues: bordes de cartón, bisagras y lomo (6 líneas).
        assert_eq!(c.pliegues.len(), 6);
        assert_eq!(c.pliegues_horizontales, vec![15.0, 231.0]);
    }

    #[test]
    fn armado_con_paginas_sueltas() {
        let c = calcular(&rustica(80.0)).unwrap();
        let a5 = Tamano::new(148.0, 210.0);
        let asignacion = [
            (TipoPanel::Tapa, 0, a5),
            (TipoPanel::Contratapa, 1, a5),
            (TipoPanel::SolapaTapa, 2, Tamano::new(80.0, 210.0)),
        ];
        let cara = cara_armada(&c, &asignacion, &OpcionesMarcas::default()).unwrap();
        let cajas = cara.cajas.unwrap();
        assert!((cajas.corte.ancho - c.tamano.ancho).abs() < 1e-9);
        // La tapa lleva rebase en cabeza y pie, pero no hacia el lomo ni la solapa.
        let tapa = &cara.ubicaciones[0];
        assert_eq!((tapa.recorte.x, tapa.recorte.ancho), (tapa.corte.x, tapa.corte.ancho));
        assert_eq!(tapa.recorte.alto, tapa.corte.alto + 6.0);
        let solapa = &cara.ubicaciones[2];
        assert!((solapa.recorte.derecha() - solapa.corte.derecha() - 3.0).abs() < 1e-9);
        assert_eq!(cara.marcas.pliegues.len(), 2 * c.pliegues.len());
        // Página de otro tamaño: error claro.
        let mal = [(TipoPanel::Tapa, 0, Tamano::new(150.0, 210.0))];
        assert!(cara_armada(&c, &mal, &OpcionesMarcas::default()).is_err());
    }
}
