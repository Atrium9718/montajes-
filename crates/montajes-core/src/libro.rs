//! Cálculos propios de libros y revistas: lomo y desplazamiento (creep).

use crate::catalogo::Papel;
use crate::{Error, Resultado};

/// Grosor del lomo para encuadernación al lomo (PUR, hot-melt, cosido).
///
/// `lomo = hojas_tripa × calibre_tripa + 2 × calibre_portada + tolerancia`,
/// donde `hojas_tripa = páginas / 2` (cada hoja tiene dos caras).
#[derive(Debug, Clone, PartialEq)]
pub struct CalculoLomo {
    pub paginas: u32,
    pub hojas: u32,
    pub bloque_mm: f64,
    pub portada_mm: f64,
    pub tolerancia_mm: f64,
    pub lomo_mm: f64,
}

pub fn calcular_lomo(
    paginas: u32,
    tripa: &Papel,
    portada: Option<&Papel>,
    tolerancia_mm: f64,
) -> Resultado<CalculoLomo> {
    if paginas == 0 || !paginas.is_multiple_of(2) {
        return Err(Error::Invalido(format!("{paginas} páginas: debe ser un número par mayor que cero")));
    }
    let hojas = paginas / 2;
    let bloque_mm = f64::from(hojas) * tripa.calibre_mm();
    let portada_mm = portada.map_or(0.0, |p| 2.0 * p.calibre_mm());
    Ok(CalculoLomo {
        paginas,
        hojas,
        bloque_mm,
        portada_mm,
        tolerancia_mm,
        lomo_mm: bloque_mm + portada_mm + tolerancia_mm,
    })
}

/// Desplazamiento hacia el lomo de cada hoja de una revista a caballete.
///
/// Las hojas se anidan una dentro de otra; cada hoja interior sobresale por
/// el corte frontal un calibre más que la anterior. Se devuelve un valor por
/// hoja, de la exterior (0 mm) a la central (máximo).
pub fn desplazamiento_caballete(paginas: u32, papel: &Papel) -> Resultado<Vec<f64>> {
    if paginas == 0 || !paginas.is_multiple_of(4) {
        return Err(Error::Invalido(format!("{paginas} páginas: a caballete debe ser múltiplo de 4")));
    }
    let hojas = paginas / 4;
    Ok((0..hojas).map(|i| f64::from(i) * papel.calibre_mm()).collect())
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn papel(calibre_um: f64) -> Papel {
        Papel {
            id: "p".into(),
            nombre: "p".into(),
            gramaje: 75.0,
            calibre_um,
            estucado: false,
            fibra: None,
            pliegos: vec![],
            notas: String::new(),
        }
    }

    #[test]
    fn lomo_de_240_paginas_en_bond_75() {
        let c = calcular_lomo(240, &papel(100.0), Some(&papel(300.0)), 0.5).unwrap();
        assert_eq!(c.hojas, 120);
        assert!((c.bloque_mm - 12.0).abs() < 1e-9);
        assert!((c.portada_mm - 0.6).abs() < 1e-9);
        assert!((c.lomo_mm - 13.1).abs() < 1e-9);
    }

    #[test]
    fn lomo_rechaza_paginas_impares() {
        assert!(calcular_lomo(241, &papel(100.0), None, 0.0).is_err());
    }

    #[test]
    fn creep_de_revista_de_32_paginas() {
        let d = desplazamiento_caballete(32, &papel(100.0)).unwrap();
        assert_eq!(d.len(), 8);
        assert_eq!(d[0], 0.0);
        assert!((d[7] - 0.7).abs() < 1e-9);
        assert!(desplazamiento_caballete(30, &papel(100.0)).is_err());
    }
}
