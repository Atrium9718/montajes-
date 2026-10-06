//! Montaje de piezas sueltas repetidas (step & repeat): volantes, tarjetas,
//! etiquetas, postales.

use serde::{Deserialize, Serialize};

use super::marcas::{self, OpcionesMarcas};
use super::{Cara, Margenes, Ubicacion, Volteo};
use crate::geometria::{Rect, Tamano};
use crate::{Error, Resultado};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Orientacion {
    /// Probar ambas y quedarse con la que más piezas aloja.
    Auto,
    Normal,
    Girada,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParametrosNup {
    pub pliego: Tamano,
    pub margenes: Margenes,
    /// Formato final de la pieza, tal como se ve (ya aplicado su /Rotate).
    pub pieza: Tamano,
    pub rebase: f64,
    /// Separación entre cortes de piezas vecinas; 0 = corte compartido.
    pub calle: f64,
    pub orientacion: Orientacion,
    pub marcas: OpcionesMarcas,
}

/// Resultado del cálculo, antes de asignar páginas.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Distribucion {
    pub columnas: u32,
    pub filas: u32,
    pub girada: bool,
    /// Rectángulos de corte, de abajo a arriba y de izquierda a derecha.
    pub cortes: Vec<Rect>,
    pub area_imprimible: Rect,
    /// Porcentaje del pliego que queda como producto final.
    pub aprovechamiento: f64,
}

impl Distribucion {
    pub fn piezas(&self) -> u32 {
        self.columnas * self.filas
    }
}

/// Cuántas piezas de `largo` caben en `disponible` con `calle` entre ellas.
fn caben(disponible: f64, largo: f64, calle: f64) -> u32 {
    if disponible < largo {
        return 0;
    }
    // Pequeña tolerancia para que 3 × 90 = 270 quepa en 270 exactos.
    ((disponible - largo + 1e-6) / (largo + calle)).floor() as u32 + 1
}

pub fn calcular(p: &ParametrosNup) -> Resultado<Distribucion> {
    if p.rebase < 0.0 || p.calle < 0.0 {
        return Err(Error::Invalido("rebase y calle no pueden ser negativos".into()));
    }
    let area = p.margenes.area_imprimible(p.pliego);
    // Espacio que necesitan rebase y marcas alrededor del bloque de piezas.
    let borde = p.marcas.espacio_necesario(p.rebase);
    let util_ancho = area.ancho - 2.0 * borde;
    let util_alto = area.alto - 2.0 * borde;

    let probar = |girada: bool| {
        let pieza = if girada { p.pieza.girado() } else { p.pieza };
        let col = caben(util_ancho, pieza.ancho, p.calle);
        let fil = caben(util_alto, pieza.alto, p.calle);
        (col, fil, girada, pieza)
    };
    let opciones = match p.orientacion {
        Orientacion::Normal => vec![probar(false)],
        Orientacion::Girada => vec![probar(true)],
        Orientacion::Auto => vec![probar(false), probar(true)],
    };
    // A igualdad de piezas se prefiere sin girar (max_by_key devuelve el último máximo).
    let (columnas, filas, girada, pieza) =
        opciones.into_iter().rev().max_by_key(|(c, f, _, _)| c * f).expect("hay al menos una opción");

    if columnas * filas == 0 {
        return Err(Error::NoCabe(format!(
            "pieza {} con rebase {} mm y marcas en pliego {} (área útil {:.1}×{:.1} mm)",
            p.pieza, p.rebase, p.pliego, util_ancho, util_alto
        )));
    }

    let bloque_ancho = f64::from(columnas) * pieza.ancho + f64::from(columnas - 1) * p.calle;
    let bloque_alto = f64::from(filas) * pieza.alto + f64::from(filas - 1) * p.calle;
    // Centrado en el área imprimible.
    let x0 = area.x + (area.ancho - bloque_ancho) / 2.0;
    let y0 = area.y + (area.alto - bloque_alto) / 2.0;

    let mut cortes = Vec::with_capacity((columnas * filas) as usize);
    for f in 0..filas {
        for c in 0..columnas {
            cortes.push(Rect::new(
                x0 + f64::from(c) * (pieza.ancho + p.calle),
                y0 + f64::from(f) * (pieza.alto + p.calle),
                pieza.ancho,
                pieza.alto,
            ));
        }
    }
    let aprovechamiento =
        100.0 * f64::from(columnas * filas) * pieza.ancho * pieza.alto / (p.pliego.ancho * p.pliego.alto);
    Ok(Distribucion { columnas, filas, girada, cortes, area_imprimible: area, aprovechamiento })
}

/// Recorte de cada pieza: el rebase completo hacia afuera del bloque y, entre
/// piezas, solo hasta la mitad de la calle para no pisar a la vecina.
fn recorte(corte: Rect, bloque: Rect, rebase: f64, calle: f64) -> Rect {
    let interior = rebase.min(calle / 2.0);
    let lado = |en_borde: bool| if en_borde { rebase } else { interior };
    let eps = 1e-6;
    corte.expandir(
        lado((corte.x - bloque.x).abs() < eps),
        lado((corte.y - bloque.y).abs() < eps),
        lado((corte.derecha() - bloque.derecha()).abs() < eps),
        lado((corte.arriba() - bloque.arriba()).abs() < eps),
    )
}

/// Arma la cara de tiro con la página `pagina` repetida en todas las posiciones.
pub fn cara_tiro(p: &ParametrosNup, d: &Distribucion, pagina: usize) -> Cara {
    let bloque = bloque(&d.cortes);
    let giro = if d.girada { 90 } else { 0 };
    let ubicaciones = d
        .cortes
        .iter()
        .map(|&corte| Ubicacion { pagina, corte, giro, recorte: recorte(corte, bloque, p.rebase, p.calle) })
        .collect();
    Cara {
        nombre: "Tiro".into(),
        pliego: p.pliego,
        ubicaciones,
        marcas: marcas::generar(&d.cortes, p.pliego, &d.area_imprimible, p.rebase, &p.marcas),
        cajas: None,
    }
}

/// Arma el retiro: cada pieza queda exactamente detrás de su frente una vez
/// volteado el pliego.
pub fn cara_retiro(p: &ParametrosNup, d: &Distribucion, pagina: usize, volteo: Volteo) -> Cara {
    let (w, h) = (p.pliego.ancho, p.pliego.alto);
    let espejo = |r: Rect| match volteo {
        Volteo::Lateral => Rect::new(w - r.derecha(), r.y, r.ancho, r.alto),
        Volteo::Cabeza => Rect::new(r.x, h - r.arriba(), r.ancho, r.alto),
    };
    let tiro = cara_tiro(p, d, pagina);
    let ubicaciones = tiro
        .ubicaciones
        .into_iter()
        .map(|u| Ubicacion {
            pagina,
            corte: espejo(u.corte),
            recorte: espejo(u.recorte),
            giro: match volteo {
                Volteo::Lateral => u.giro,
                Volteo::Cabeza => (u.giro + 180) % 360,
            },
        })
        .collect::<Vec<_>>();
    let cortes: Vec<Rect> = ubicaciones.iter().map(|u| u.corte).collect();
    let area = p.margenes.area_imprimible(p.pliego);
    Cara {
        nombre: "Retiro".into(),
        pliego: p.pliego,
        marcas: marcas::generar(&cortes, p.pliego, &area, p.rebase, &p.marcas),
        cajas: None,
        ubicaciones,
    }
}

pub(crate) fn bloque(cortes: &[Rect]) -> Rect {
    let x0 = cortes.iter().map(|r| r.x).fold(f64::INFINITY, f64::min);
    let y0 = cortes.iter().map(|r| r.y).fold(f64::INFINITY, f64::min);
    let x1 = cortes.iter().map(Rect::derecha).fold(f64::NEG_INFINITY, f64::max);
    let y1 = cortes.iter().map(Rect::arriba).fold(f64::NEG_INFINITY, f64::max);
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn tarjetas(calle: f64, orientacion: Orientacion) -> ParametrosNup {
        ParametrosNup {
            pliego: Tamano::new(480.0, 330.0),
            margenes: Margenes { pinza: 10.0, cola: 5.0, lateral: 5.0 },
            pieza: Tamano::new(90.0, 50.0),
            rebase: 3.0,
            calle,
            orientacion,
            marcas: OpcionesMarcas::default(),
        }
    }

    #[test]
    fn tarjetas_en_tabloide_extra() {
        // Área imprimible 470×315; con marcas (3 + 5 mm por lado) quedan 454×299.
        let d = calcular(&tarjetas(0.0, Orientacion::Auto)).unwrap();
        // Normal: 5 × 5 = 25. Girada (50×90): 9 × 3 = 27 → gana girada.
        assert_eq!((d.columnas, d.filas, d.girada), (9, 3, true));
        assert_eq!(d.piezas(), 27);
    }

    #[test]
    fn orientacion_fija_y_calle() {
        let d = calcular(&tarjetas(0.0, Orientacion::Normal)).unwrap();
        assert_eq!((d.columnas, d.filas), (5, 5));
        // Con 6 mm de calle: 454 → 4 columnas (4×90 + 3×6 = 378; 5 serían 474).
        let d = calcular(&tarjetas(6.0, Orientacion::Normal)).unwrap();
        assert_eq!(d.columnas, 4);
    }

    #[test]
    fn bloque_centrado_en_area_imprimible() {
        let p = tarjetas(0.0, Orientacion::Normal);
        let d = calcular(&p).unwrap();
        let b = bloque(&d.cortes);
        let a = d.area_imprimible;
        assert!(((b.x - a.x) - (a.derecha() - b.derecha())).abs() < 1e-9);
        assert!(((b.y - a.y) - (a.arriba() - b.arriba())).abs() < 1e-9);
    }

    #[test]
    fn corte_compartido_no_deja_rebase_interior() {
        let p = tarjetas(0.0, Orientacion::Normal);
        let d = calcular(&p).unwrap();
        let cara = cara_tiro(&p, &d, 0);
        // La pieza de la esquina inferior izquierda lleva rebase solo afuera.
        let u = &cara.ubicaciones[0];
        assert!((u.corte.x - u.recorte.x - 3.0).abs() < 1e-9);
        assert!((u.recorte.derecha() - u.corte.derecha()).abs() < 1e-9);
    }

    #[test]
    fn retiro_queda_detras_del_tiro() {
        let p = tarjetas(4.0, Orientacion::Normal);
        let d = calcular(&p).unwrap();
        let tiro = cara_tiro(&p, &d, 0);
        let retiro = cara_retiro(&p, &d, 1, Volteo::Lateral);
        for (t, r) in tiro.ubicaciones.iter().zip(&retiro.ubicaciones) {
            // Mismo centro reflejado sobre el eje vertical del pliego.
            let ct = t.corte.x + t.corte.ancho / 2.0;
            let cr = r.corte.x + r.corte.ancho / 2.0;
            assert!((ct + cr - p.pliego.ancho).abs() < 1e-9);
            assert_eq!(t.corte.y, r.corte.y);
        }
        let cabeza = cara_retiro(&p, &d, 1, Volteo::Cabeza);
        assert!(cabeza.ubicaciones.iter().all(|u| u.giro == 180));
    }

    #[test]
    fn pieza_que_no_cabe() {
        let mut p = tarjetas(0.0, Orientacion::Auto);
        p.pieza = Tamano::new(500.0, 400.0);
        assert!(matches!(calcular(&p), Err(Error::NoCabe(_))));
    }
}
