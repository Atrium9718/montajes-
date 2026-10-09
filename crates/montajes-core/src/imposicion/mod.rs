//! Cálculo de imposición: dónde va cada página en cada pliego.
//!
//! El resultado es geometría pura en milímetros (sin PDF), así se puede
//! probar, previsualizar en la interfaz y luego escribir con [`crate::pdf`].

pub mod firmas;
pub mod marcas;
pub mod nup;
pub mod plegado;
pub mod verificar;

use serde::{Deserialize, Serialize};

use crate::catalogo::Maquina;
use crate::geometria::{Rect, Tamano};

/// Zona no imprimible del pliego. La pinza va en el borde inferior.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Margenes {
    pub pinza: f64,
    pub cola: f64,
    pub lateral: f64,
}

impl Margenes {
    pub fn de_maquina(m: &Maquina) -> Self {
        Self { pinza: m.pinza, cola: m.cola, lateral: m.lateral }
    }

    /// Al voltear por la cabeza (work & tumble) la cola del tiro pasa a ser la
    /// pinza del retiro, así que ambos bordes deben respetar el mayor margen.
    pub fn para_volteo(self, volteo: Volteo) -> Self {
        match volteo {
            Volteo::Lateral => self,
            Volteo::Cabeza => {
                let m = self.pinza.max(self.cola);
                Self { pinza: m, cola: m, ..self }
            }
        }
    }

    pub fn area_imprimible(&self, pliego: Tamano) -> Rect {
        Rect::new(self.lateral, self.pinza, pliego.ancho - 2.0 * self.lateral, pliego.alto - self.pinza - self.cola)
    }
}

/// Cómo se voltea el pliego para imprimir el retiro.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Volteo {
    /// Sobre el eje vertical, conservando la pinza (tira y retira / work & turn).
    #[default]
    Lateral,
    /// Sobre el eje horizontal, cambiando la pinza (work & tumble).
    Cabeza,
}

/// Una página colocada en el pliego.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ubicacion {
    /// Índice (desde 0) de la página del PDF de entrada.
    pub pagina: usize,
    /// Rectángulo de corte (formato final) en el pliego.
    pub corte: Rect,
    /// Giro horario adicional en grados: 0, 90, 180 o 270.
    pub giro: u16,
    /// Hasta dónde se deja ver la página (corte + rebase permitido).
    pub recorte: Rect,
    /// Escala del arte (1 = tamaño real). El corte ya viene reducido; la
    /// página se escala para llenarlo.
    #[serde(default = "escala_uno")]
    pub escala: f64,
}

fn escala_uno() -> f64 {
    1.0
}

/// Una cara de un pliego (tiro o retiro).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cara {
    pub nombre: String,
    pub pliego: Tamano,
    pub ubicaciones: Vec<Ubicacion>,
    pub marcas: marcas::Marcas,
    /// Cajas de corte y rebase propias; sin ellas el pliego entero es el formato.
    #[serde(default)]
    pub cajas: Option<Cajas>,
}

/// TrimBox y BleedBox de una página que no es un pliego de máquina (p. ej. una portada).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Cajas {
    pub corte: Rect,
    pub sangrado: Rect,
}
