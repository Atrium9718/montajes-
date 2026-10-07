//! Cotizador: papel con mácula, planchas, impresión y acabados.
//!
//! Un trabajo se cotiza como una lista de *tirajes*: cada pliego distinto
//! que se imprime (un montaje de piezas, cada firma de un libro, un
//! combinado). La mácula (pliegos de arranque y desperdicio de tiraje) se
//! suma a los pliegos netos antes de calcular papel e impresión.

use serde::{Deserialize, Serialize};

use crate::geometria::Tamano;
use crate::{Error, Resultado};

/// Costos y mácula de una máquina, en la moneda del taller.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CostosMaquina {
    /// Máquina digital: se cobra por clic (cara impresa) y no lleva planchas.
    #[serde(default)]
    pub digital: bool,
    /// Imprime ambas caras en una pasada.
    #[serde(default)]
    pub duplex: bool,
    #[serde(default)]
    pub costo_plancha: f64,
    /// Puesta a punto de cada pasada (lavado, registro, color).
    #[serde(default)]
    pub costo_arranque: f64,
    /// Por cada mil pliegos impresos en una pasada.
    #[serde(default)]
    pub costo_millar: f64,
    /// Valor mínimo de impresión por tiraje.
    #[serde(default)]
    pub costo_minimo: f64,
    /// Digital: por cara impresa.
    #[serde(default)]
    pub costo_clic: f64,
    /// Mácula de arranque: pliegos que se pierden al poner a punto cada pasada.
    #[serde(default)]
    pub macula_arranque: u32,
    /// Mácula de tiraje: porcentaje de pliegos que se pierden durante la impresión.
    #[serde(default)]
    pub macula_porcentaje: f64,
}

/// Papel: precio por pliego de compra y cuántos pliegos de máquina salen de él.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CostoPapel {
    pub nombre: String,
    pub precio_pliego_compra: f64,
    pub salen_por_pliego: u32,
}

/// Un pliego distinto que se imprime.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tiraje {
    pub nombre: String,
    /// Pliegos buenos que se necesitan (sin mácula).
    pub pliegos_netos: u32,
    pub colores_tiro: u8,
    /// 0 = sin retiro.
    pub colores_retiro: u8,
    /// Tiro y retiro con la misma plancha (work & turn).
    #[serde(default)]
    pub tira_retira: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unidad {
    /// Por cada ejemplar terminado.
    Ejemplar,
    /// Por cada mil ejemplares.
    Millar,
    /// Por cada pliego de máquina (corte, barniz…).
    Pliego,
    /// Valor fijo del trabajo (diseño, troquel…).
    Fijo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Acabado {
    pub concepto: String,
    pub unidad: Unidad,
    pub valor: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParametrosCotizacion {
    /// Ejemplares terminados que pide el cliente.
    pub cantidad: u32,
    pub tirajes: Vec<Tiraje>,
    pub maquina: CostosMaquina,
    #[serde(default)]
    pub papel: Option<CostoPapel>,
    #[serde(default)]
    pub acabados: Vec<Acabado>,
    #[serde(default)]
    pub utilidad_porcentaje: f64,
    #[serde(default)]
    pub impuesto_porcentaje: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Linea {
    pub concepto: String,
    pub detalle: String,
    pub valor: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DetalleTiraje {
    pub nombre: String,
    pub pliegos_netos: u32,
    pub macula: u32,
    pub pliegos_maquina: u32,
    pub pasadas: u32,
    pub planchas: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cotizacion {
    pub tirajes: Vec<DetalleTiraje>,
    pub pliegos_netos: u32,
    pub macula: u32,
    pub pliegos_maquina: u32,
    pub pliegos_compra: u32,
    pub planchas: u32,
    pub impresiones: u64,
    pub lineas: Vec<Linea>,
    pub subtotal: f64,
    pub utilidad: f64,
    pub impuesto: f64,
    pub total: f64,
    pub unitario: f64,
}

/// Cuántos pliegos de máquina salen de un pliego de compra (mejor orientación).
pub fn salen_de(compra: Tamano, pliego: Tamano) -> u32 {
    let caben = |a: f64, b: f64| ((a + 1e-6) / b).floor().max(0.0) as u32;
    let normal = caben(compra.ancho, pliego.ancho) * caben(compra.alto, pliego.alto);
    let girado = caben(compra.ancho, pliego.alto) * caben(compra.alto, pliego.ancho);
    normal.max(girado)
}

/// Pliegos netos para `cantidad` ejemplares cuando cada pliego da `por_pliego`.
pub fn pliegos_para(cantidad: u32, por_pliego: u32) -> u32 {
    cantidad.div_ceil(por_pliego.max(1))
}

pub fn cotizar(p: &ParametrosCotizacion) -> Resultado<Cotizacion> {
    if p.tirajes.is_empty() {
        return Err(Error::Invalido("no hay nada que imprimir".into()));
    }
    if p.cantidad == 0 {
        return Err(Error::Invalido("la cantidad debe ser mayor que cero".into()));
    }
    let m = &p.maquina;
    let mut tirajes = Vec::with_capacity(p.tirajes.len());
    let mut lineas = Vec::new();
    let (mut costo_planchas, mut costo_impresion, mut impresiones) = (0.0, 0.0, 0_u64);
    for t in &p.tirajes {
        let dos_caras = t.colores_retiro > 0;
        let pasadas = if dos_caras && !m.duplex { 2 } else { 1 };
        let macula =
            m.macula_arranque * pasadas + (f64::from(t.pliegos_netos) * m.macula_porcentaje / 100.0).ceil() as u32;
        let pliegos_maquina = t.pliegos_netos + macula;
        let planchas = if m.digital {
            0
        } else {
            u32::from(t.colores_tiro) + if dos_caras && !t.tira_retira { u32::from(t.colores_retiro) } else { 0 }
        };
        costo_planchas += f64::from(planchas) * m.costo_plancha;
        if m.digital {
            let caras = if dos_caras { 2 } else { 1 };
            let clics = u64::from(pliegos_maquina) * caras;
            impresiones += clics;
            costo_impresion += (clics as f64 * m.costo_clic).max(m.costo_minimo);
        } else {
            impresiones += u64::from(pliegos_maquina) * u64::from(pasadas);
            let por_pasada = m.costo_arranque + f64::from(pliegos_maquina) / 1000.0 * m.costo_millar;
            costo_impresion += (por_pasada * f64::from(pasadas)).max(m.costo_minimo);
        }
        tirajes.push(DetalleTiraje {
            nombre: t.nombre.clone(),
            pliegos_netos: t.pliegos_netos,
            macula,
            pliegos_maquina,
            pasadas,
            planchas,
        });
    }
    let pliegos_netos: u32 = tirajes.iter().map(|t| t.pliegos_netos).sum();
    let macula: u32 = tirajes.iter().map(|t| t.macula).sum();
    let pliegos_maquina = pliegos_netos + macula;
    let planchas: u32 = tirajes.iter().map(|t| t.planchas).sum();

    let mut pliegos_compra = 0;
    if let Some(papel) = &p.papel {
        if papel.salen_por_pliego == 0 {
            return Err(Error::NoCabe("el pliego de máquina no sale del formato de compra del papel".into()));
        }
        pliegos_compra = pliegos_maquina.div_ceil(papel.salen_por_pliego);
        lineas.push(Linea {
            concepto: "Papel".into(),
            detalle: format!(
                "{} · {pliegos_compra} pliegos de compra ({pliegos_maquina} de máquina, {} por pliego; incluye {macula} de mácula)",
                papel.nombre, papel.salen_por_pliego
            ),
            valor: f64::from(pliegos_compra) * papel.precio_pliego_compra,
        });
    }
    if planchas > 0 {
        lineas.push(Linea {
            concepto: "Planchas".into(),
            detalle: format!("{planchas} planchas"),
            valor: costo_planchas,
        });
    }
    lineas.push(Linea {
        concepto: "Impresión".into(),
        detalle: if m.digital {
            format!("{impresiones} clics")
        } else {
            format!("{impresiones} impresiones en {} pasadas", tirajes.iter().map(|t| t.pasadas).sum::<u32>())
        },
        valor: costo_impresion,
    });
    for a in &p.acabados {
        let (veces, unidad) = match a.unidad {
            Unidad::Ejemplar => (f64::from(p.cantidad), format!("{} ejemplares", p.cantidad)),
            Unidad::Millar => {
                (f64::from(p.cantidad) / 1000.0, format!("{:.1} millares", f64::from(p.cantidad) / 1000.0))
            }
            Unidad::Pliego => (f64::from(pliegos_maquina), format!("{pliegos_maquina} pliegos")),
            Unidad::Fijo => (1.0, "valor fijo".into()),
        };
        lineas.push(Linea { concepto: a.concepto.clone(), detalle: unidad, valor: veces * a.valor });
    }
    let subtotal: f64 = lineas.iter().map(|l| l.valor).sum();
    let utilidad = subtotal * p.utilidad_porcentaje / 100.0;
    let impuesto = (subtotal + utilidad) * p.impuesto_porcentaje / 100.0;
    let total = subtotal + utilidad + impuesto;
    Ok(Cotizacion {
        tirajes,
        pliegos_netos,
        macula,
        pliegos_maquina,
        pliegos_compra,
        planchas,
        impresiones,
        lineas,
        subtotal,
        utilidad,
        impuesto,
        total,
        unitario: total / f64::from(p.cantidad),
    })
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn offset() -> CostosMaquina {
        CostosMaquina {
            digital: false,
            duplex: false,
            costo_plancha: 30_000.0,
            costo_arranque: 50_000.0,
            costo_millar: 40_000.0,
            costo_minimo: 0.0,
            costo_clic: 0.0,
            macula_arranque: 100,
            macula_porcentaje: 3.0,
        }
    }

    #[test]
    fn papel_que_sale_del_formato_de_compra() {
        assert_eq!(salen_de(Tamano::new(1000.0, 700.0), Tamano::new(500.0, 350.0)), 4);
        assert_eq!(salen_de(Tamano::new(1000.0, 700.0), Tamano::new(740.0, 530.0)), 1);
        assert_eq!(salen_de(Tamano::new(1000.0, 700.0), Tamano::new(330.0, 488.0)), 4);
        assert_eq!(salen_de(Tamano::new(500.0, 350.0), Tamano::new(740.0, 530.0)), 0);
        assert_eq!(pliegos_para(1000, 27), 38);
    }

    #[test]
    fn volantes_4x4_en_offset_con_macula() {
        let p = ParametrosCotizacion {
            cantidad: 5000,
            tirajes: vec![Tiraje {
                nombre: "Volante".into(),
                pliegos_netos: 1250,
                colores_tiro: 4,
                colores_retiro: 4,
                tira_retira: false,
            }],
            maquina: offset(),
            papel: Some(CostoPapel {
                nombre: "Propalcote 150".into(),
                precio_pliego_compra: 1_200.0,
                salen_por_pliego: 2,
            }),
            acabados: vec![Acabado { concepto: "Corte".into(), unidad: Unidad::Millar, valor: 5_000.0 }],
            utilidad_porcentaje: 30.0,
            impuesto_porcentaje: 0.0,
        };
        let c = cotizar(&p).unwrap();
        // Mácula: 100 por pasada × 2 + 3 % de 1250 (37,5 → 38) = 238.
        assert_eq!(c.macula, 238);
        assert_eq!(c.pliegos_maquina, 1488);
        assert_eq!(c.pliegos_compra, 744);
        assert_eq!(c.planchas, 8);
        let papel = 744.0 * 1_200.0;
        let planchas = 8.0 * 30_000.0;
        let impresion = 2.0 * (50_000.0 + 1.488 * 40_000.0);
        let corte = 5.0 * 5_000.0;
        let subtotal = papel + planchas + impresion + corte;
        assert!((c.subtotal - subtotal).abs() < 1e-6);
        assert!((c.total - subtotal * 1.3).abs() < 1e-6);
        assert!((c.unitario - c.total / 5000.0).abs() < 1e-9);
    }

    #[test]
    fn tira_y_retira_ahorra_planchas_y_digital_cobra_clics() {
        let mut p = ParametrosCotizacion {
            cantidad: 1000,
            tirajes: vec![Tiraje {
                nombre: "Firma 1".into(),
                pliegos_netos: 500,
                colores_tiro: 4,
                colores_retiro: 4,
                tira_retira: true,
            }],
            maquina: offset(),
            papel: None,
            acabados: vec![],
            utilidad_porcentaje: 0.0,
            impuesto_porcentaje: 19.0,
        };
        assert_eq!(cotizar(&p).unwrap().planchas, 4);
        p.maquina = CostosMaquina {
            digital: true,
            duplex: true,
            costo_clic: 300.0,
            macula_arranque: 5,
            macula_porcentaje: 1.0,
            ..offset()
        };
        let c = cotizar(&p).unwrap();
        // Dúplex: una pasada; mácula 5 + 1 % de 500 = 10; 510 pliegos × 2 caras.
        assert_eq!((c.planchas, c.macula, c.impresiones), (0, 10, 1020));
        assert!((c.total - 1020.0 * 300.0 * 1.19).abs() < 1e-6);
    }

    #[test]
    fn papel_que_no_sale_del_formato() {
        let p = ParametrosCotizacion {
            cantidad: 10,
            tirajes: vec![Tiraje {
                nombre: "x".into(),
                pliegos_netos: 1,
                colores_tiro: 1,
                colores_retiro: 0,
                tira_retira: false,
            }],
            maquina: offset(),
            papel: Some(CostoPapel { nombre: "p".into(), precio_pliego_compra: 1.0, salen_por_pliego: 0 }),
            acabados: vec![],
            utilidad_porcentaje: 0.0,
            impuesto_porcentaje: 0.0,
        };
        assert!(cotizar(&p).is_err());
    }
}
