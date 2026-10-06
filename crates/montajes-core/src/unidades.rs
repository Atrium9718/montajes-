/// Puntos PDF por milímetro (1 pt = 1/72 pulgada).
pub const PT_POR_MM: f64 = 72.0 / 25.4;

pub fn mm_a_pt(mm: f64) -> f64 {
    mm * PT_POR_MM
}

pub fn pt_a_mm(pt: f64) -> f64 {
    pt / PT_POR_MM
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn a4_en_puntos() {
        assert!((mm_a_pt(210.0) - 595.276).abs() < 0.001);
        assert!((pt_a_mm(841.89) - 297.0).abs() < 0.001);
    }
}
