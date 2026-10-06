use serde::{Deserialize, Serialize};

/// Ancho × alto en milímetros.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Tamano {
    pub ancho: f64,
    pub alto: f64,
}

impl Tamano {
    pub const fn new(ancho: f64, alto: f64) -> Self {
        Self { ancho, alto }
    }

    /// Redondea a centésimas de milímetro (los PDF traen ruido de conversión).
    pub fn redondeado(self) -> Self {
        Self::new((self.ancho * 100.0).round() / 100.0, (self.alto * 100.0).round() / 100.0)
    }

    pub fn girado(self) -> Self {
        Self::new(self.alto, self.ancho)
    }

    /// Interpreta "450x320" o "450×320" (mm).
    pub fn parse(texto: &str) -> Option<Self> {
        let (a, b) = texto.split_once(['x', 'X', '×'])?;
        let ancho = a.trim().replace(',', ".").parse().ok()?;
        let alto = b.trim().replace(',', ".").parse().ok()?;
        (ancho > 0.0 && alto > 0.0).then_some(Self::new(ancho, alto))
    }
}

impl std::fmt::Display for Tamano {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let corto = |v: f64| {
            let s = format!("{v:.2}");
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        };
        write!(f, "{}×{} mm", corto(self.ancho), corto(self.alto))
    }
}

/// Rectángulo con origen abajo a la izquierda (convención PDF).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub ancho: f64,
    pub alto: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, ancho: f64, alto: f64) -> Self {
        Self { x, y, ancho, alto }
    }

    pub fn derecha(&self) -> f64 {
        self.x + self.ancho
    }

    pub fn arriba(&self) -> f64 {
        self.y + self.alto
    }

    pub fn interseccion(&self, otro: &Rect) -> Rect {
        let x = self.x.max(otro.x);
        let y = self.y.max(otro.y);
        Rect::new(
            x,
            y,
            (self.derecha().min(otro.derecha()) - x).max(0.0),
            (self.arriba().min(otro.arriba()) - y).max(0.0),
        )
    }

    /// Refleja sobre el eje vertical de un pliego de `ancho` (vista del retiro).
    pub fn reflejar_x(&self, ancho: f64) -> Rect {
        Rect::new(ancho - self.derecha(), self.y, self.ancho, self.alto)
    }

    /// Gira 90° en sentido horario dentro de un pliego de `ancho` (antes del giro).
    pub fn girar_horario(&self, ancho: f64) -> Rect {
        Rect::new(self.y, ancho - self.derecha(), self.alto, self.ancho)
    }

    /// Gira 90° en sentido antihorario dentro de un pliego de `alto` (antes del giro).
    pub fn girar_antihorario(&self, alto: f64) -> Rect {
        Rect::new(alto - self.arriba(), self.x, self.alto, self.ancho)
    }

    /// Expande cada lado por separado: (izquierda, abajo, derecha, arriba).
    pub fn expandir(&self, izq: f64, abajo: f64, der: f64, arriba: f64) -> Self {
        Self::new(self.x - izq, self.y - abajo, self.ancho + izq + der, self.alto + abajo + arriba)
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn giros_inversos() {
        let r = Rect::new(10.0, 20.0, 30.0, 40.0);
        // Pliego virtual 200 × 100: el giro horario y el inverso del antihorario coinciden en tamaño.
        let h = r.girar_horario(200.0);
        assert_eq!(h, Rect::new(20.0, 160.0, 40.0, 30.0));
        let a = r.girar_antihorario(100.0);
        assert_eq!(a, Rect::new(40.0, 10.0, 40.0, 30.0));
        assert_eq!(r.reflejar_x(200.0).reflejar_x(200.0), r);
        assert_eq!(r.interseccion(&Rect::new(0.0, 0.0, 25.0, 100.0)), Rect::new(10.0, 20.0, 15.0, 40.0));
    }

    #[test]
    fn parse_tamano() {
        assert_eq!(Tamano::parse("450x320"), Some(Tamano::new(450.0, 320.0)));
        assert_eq!(Tamano::parse("90,5 × 50"), Some(Tamano::new(90.5, 50.0)));
        assert_eq!(Tamano::parse("90"), None);
        assert_eq!(Tamano::parse("0x5"), None);
        assert_eq!(Tamano::new(89.999997, 50.5).redondeado().to_string(), "90×50.5 mm");
    }
}
