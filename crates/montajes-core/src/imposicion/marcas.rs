//! Marcas de impresión: corte, registro y tira de control de color.
//!
//! Se dibujan en color de registro (separación `All`), así aparecen en todas
//! las planchas, excepto la tira de color, que lleva sus tintas reales.

use serde::{Deserialize, Serialize};

use super::nup::bloque;
use crate::geometria::{Rect, Tamano};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpcionesMarcas {
    pub corte: bool,
    /// Largo de cada marca de corte (mm).
    pub largo: f64,
    /// Distancia del corte al inicio de la marca; nunca menor al rebase.
    pub desfase: f64,
    pub registro: bool,
    pub tira_color: bool,
}

impl Default for OpcionesMarcas {
    fn default() -> Self {
        Self { corte: true, largo: 5.0, desfase: 3.0, registro: true, tira_color: true }
    }
}

impl OpcionesMarcas {
    pub fn ninguna() -> Self {
        Self { corte: false, registro: false, tira_color: false, ..Self::default() }
    }

    /// Distancia entre el corte y el inicio de la marca.
    pub fn distancia(&self, rebase: f64) -> f64 {
        self.desfase.max(rebase)
    }

    /// Franja que hay que reservar alrededor del bloque de piezas.
    pub fn espacio_necesario(&self, rebase: f64) -> f64 {
        if self.corte || self.registro { self.distancia(rebase) + self.largo } else { rebase }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Linea {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Registro {
    pub x: f64,
    pub y: f64,
    pub radio: f64,
}

/// Parche de la tira de control, en porcentajes CMYK (0–1).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Parche {
    pub rect: Rect,
    pub cmyk: [f64; 4],
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Marcas {
    pub corte: Vec<Linea>,
    pub registro: Vec<Registro>,
    pub tira_color: Vec<Parche>,
    /// Marcas de plegado (se dibujan discontinuas).
    #[serde(default)]
    pub pliegues: Vec<Linea>,
    /// Marcas de alzado en el lomo de cada firma.
    #[serde(default)]
    pub alzado: Vec<Rect>,
    /// Dónde va el rótulo de la plancha (tintas, archivo, tiro o retiro).
    #[serde(default)]
    pub rotulo: Option<Rotulo>,
    /// Lo que no se pudo dibujar por falta de espacio.
    pub avisos: Vec<String>,
}

/// Lugar del rótulo: el texto empieza en (x, y) con letras de `alto` mm y
/// cabe en `largo` mm. Vertical: se lee de abajo hacia arriba (girado 90°)
/// y las letras quedan a la izquierda de x.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rotulo {
    pub x: f64,
    pub y: f64,
    pub alto: f64,
    pub largo: f64,
    pub vertical: bool,
}

/// Alto de las letras del rótulo (mm).
pub const ALTO_ROTULO: f64 = 2.5;

/// Secuencia de la tira: sólidos, sobreimpresiones, tramas al 50 % y gris.
const PARCHES: [[f64; 4]; 12] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
    [1.0, 1.0, 0.0, 0.0],
    [1.0, 0.0, 1.0, 0.0],
    [0.0, 1.0, 1.0, 0.0],
    [0.5, 0.0, 0.0, 0.0],
    [0.0, 0.5, 0.0, 0.0],
    [0.0, 0.0, 0.5, 0.0],
    [0.0, 0.0, 0.0, 0.5],
    [0.5, 0.4, 0.4, 0.0],
];
const LADO_PARCHE: f64 = 5.0;

/// Valores únicos ordenados (con tolerancia de 1 µm).
fn unicos(mut v: Vec<f64>) -> Vec<f64> {
    v.sort_by(f64::total_cmp);
    v.dedup_by(|a, b| (*a - *b).abs() < 1e-3);
    v
}

/// Centro del tramo entre cortes más cercano a la mitad, con espacio para
/// una marca de registro (así no queda encima de una marca de corte).
fn hueco_central(cortes: &[f64], centro: f64, ancho: f64) -> Option<f64> {
    cortes
        .windows(2)
        .filter(|w| w[1] - w[0] >= ancho)
        .map(|w| (w[0] + w[1]) / 2.0)
        .min_by(|a, b| (a - centro).abs().total_cmp(&(b - centro).abs()))
}

pub fn generar(cortes: &[Rect], pliego: Tamano, area: &Rect, rebase: f64, op: &OpcionesMarcas) -> Marcas {
    let mut m = Marcas::default();
    if cortes.is_empty() {
        return m;
    }
    let b = bloque(cortes);
    let d = op.distancia(rebase);
    let xs = unicos(cortes.iter().flat_map(|r| [r.x, r.derecha()]).collect());
    let ys = unicos(cortes.iter().flat_map(|r| [r.y, r.arriba()]).collect());

    if op.corte {
        for &x in &xs {
            m.corte.push(Linea { x1: x, y1: b.y - d - op.largo, x2: x, y2: b.y - d });
            m.corte.push(Linea { x1: x, y1: b.arriba() + d, x2: x, y2: b.arriba() + d + op.largo });
        }
        for &y in &ys {
            m.corte.push(Linea { x1: b.x - d - op.largo, y1: y, x2: b.x - d, y2: y });
            m.corte.push(Linea { x1: b.derecha() + d, y1: y, x2: b.derecha() + d + op.largo, y2: y });
        }
    }

    if op.registro {
        let radio = op.largo / 2.0;
        let banda = d + op.largo / 2.0;
        match hueco_central(&ys, b.y + b.alto / 2.0, 2.0 * radio + 2.0) {
            Some(y) => {
                m.registro.push(Registro { x: b.x - banda, y, radio });
                m.registro.push(Registro { x: b.derecha() + banda, y, radio });
            }
            None => m.avisos.push("sin espacio para registro a los lados".into()),
        }
        match hueco_central(&xs, b.x + b.ancho / 2.0, 2.0 * radio + 2.0) {
            Some(x) => {
                m.registro.push(Registro { x, y: b.y - banda, radio });
                m.registro.push(Registro { x, y: b.arriba() + banda, radio });
            }
            None => m.avisos.push("sin espacio para registro arriba y abajo".into()),
        }
    }

    if op.tira_color {
        // En la cola (arriba), entre las marcas y el límite imprimible.
        let y = b.arriba() + op.espacio_necesario(rebase) + 1.0;
        let x0 = area.x.max(0.0);
        let x1 = area.derecha().min(pliego.ancho);
        if y + LADO_PARCHE <= area.arriba() && x1 - x0 >= LADO_PARCHE * PARCHES.len() as f64 {
            let cantidad = ((x1 - x0) / LADO_PARCHE).floor() as usize;
            let inicio = x0 + (x1 - x0 - cantidad as f64 * LADO_PARCHE) / 2.0;
            for i in 0..cantidad {
                m.tira_color.push(Parche {
                    rect: Rect::new(inicio + i as f64 * LADO_PARCHE, y, LADO_PARCHE, LADO_PARCHE),
                    cmyk: PARCHES[i % PARCHES.len()],
                });
            }
        } else {
            m.avisos.push("sin espacio en la cola para la tira de color".into());
        }
    }

    if op.corte {
        m.rotulo = lugar_rotulo(&b, pliego, area, &m, op.espacio_necesario(rebase));
        if m.rotulo.is_none() {
            m.avisos.push("sin espacio para el rótulo de la plancha".into());
        }
    }
    m
}

/// Primer lugar libre para el rótulo: en la cola sobre la tira de color, a la
/// izquierda o a la derecha del bloque (vertical) o entre la pinza y el bloque.
fn lugar_rotulo(b: &Rect, pliego: Tamano, area: &Rect, m: &Marcas, espacio: f64) -> Option<Rotulo> {
    let h = ALTO_ROTULO;
    let x0 = area.x.max(0.0) + 1.0;
    let x1 = area.derecha().min(pliego.ancho) - 1.0;
    let y0 = area.y.max(0.0) + 1.0;
    let y1 = area.arriba().min(pliego.alto) - 1.0;
    let minimo = 40.0;
    // En la cola: sobre la tira de color o sobre las marcas.
    let sobre = m.tira_color.iter().map(|p| p.rect.arriba()).fold(b.arriba() + espacio, f64::max) + 1.0;
    if sobre + h <= y1 && x1 - b.x.max(x0) >= minimo {
        let x = b.x.max(x0);
        return Some(Rotulo { x, y: sobre, alto: h, largo: x1 - x, vertical: false });
    }
    // A los lados, leyendo de abajo hacia arriba.
    let izquierda = b.x - espacio - 1.0;
    if izquierda - h >= x0 && b.alto >= minimo {
        return Some(Rotulo { x: izquierda, y: b.y, alto: h, largo: b.alto, vertical: true });
    }
    let derecha = b.derecha() + espacio + 1.0 + h;
    if derecha <= x1 && b.alto >= minimo {
        return Some(Rotulo { x: derecha, y: b.y, alto: h, largo: b.alto, vertical: true });
    }
    // Entre la pinza y el bloque.
    let abajo = b.y - espacio - 1.0 - h;
    if abajo >= y0 && x1 - b.x.max(x0) >= minimo {
        let x = b.x.max(x0);
        return Some(Rotulo { x, y: abajo, alto: h, largo: x1 - x, vertical: false });
    }
    None
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn marcas_de_corte_por_cada_linea() {
        // Dos piezas lado a lado con corte compartido: 3 líneas verticales y 2 horizontales.
        let cortes = [Rect::new(20.0, 20.0, 90.0, 50.0), Rect::new(110.0, 20.0, 90.0, 50.0)];
        let area = Rect::new(5.0, 10.0, 210.0, 90.0);
        let m = generar(&cortes, Tamano::new(220.0, 105.0), &area, 3.0, &OpcionesMarcas::default());
        assert_eq!(m.corte.len(), 3 * 2 + 2 * 2);
        // El registro lateral no cae sobre el corte central (y = 45 es el centro de la pieza).
        assert!(m.registro.iter().any(|r| (r.y - 45.0).abs() < 1e-9));
        // El registro de arriba/abajo esquiva la línea x = 110.
        assert!(m.registro.iter().all(|r| (r.x - 110.0).abs() > 1e-6));
    }

    #[test]
    fn sin_espacio_para_tira() {
        let cortes = [Rect::new(10.0, 10.0, 90.0, 50.0)];
        let area = Rect::new(0.0, 0.0, 110.0, 70.0);
        let m = generar(&cortes, Tamano::new(110.0, 70.0), &area, 3.0, &OpcionesMarcas::default());
        assert!(m.tira_color.is_empty());
        assert!(!m.avisos.is_empty());
    }
}
