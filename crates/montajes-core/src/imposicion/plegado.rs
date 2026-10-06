//! Esquemas de plegado calculados simulando el doblez del pliego.
//!
//! En lugar de una tabla fija por cada esquema, se dobla un pliego virtual
//! (cuadrícula de celdas) tal como lo hace la plegadora y se numeran las
//! hojas del cuadernillo resultante. Así cualquier secuencia de pliegues en
//! cruz produce una imposición correcta por construcción.

use serde::{Deserialize, Serialize};

use crate::{Error, Resultado};

/// Dirección del pliegue sobre el pliego plano, visto desde el tiro.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Pliegue {
    /// Línea vertical: la mitad izquierda se dobla sobre la derecha.
    Vertical,
    /// Línea horizontal: la mitad superior se dobla sobre la inferior.
    Horizontal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Lado {
    Tiro,
    Retiro,
}

/// Dónde va una página del cuadernillo en el pliego plano.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Posicion {
    /// Página local de la firma, desde 1.
    pub pagina: u32,
    pub lado: Lado,
    /// Columna y fila vistas desde ese lado (0 = izquierda / abajo).
    pub columna: u32,
    pub fila: u32,
    /// La página va girada 180° (cabeza con cabeza).
    pub invertida: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Esquema {
    pub paginas: u32,
    pub columnas: u32,
    pub filas: u32,
    pub pliegues: Vec<Pliegue>,
    pub posiciones: Vec<Posicion>,
}

impl Esquema {
    /// Esquemas en cruz habituales: 4, 8, 16, 32 y 64 páginas.
    pub fn estandar(paginas: u32) -> Resultado<Self> {
        use Pliegue::{Horizontal as H, Vertical as V};
        let pliegues: &[Pliegue] = match paginas {
            4 => &[V],
            8 => &[H, V],
            16 => &[V, H, V],
            32 => &[H, V, H, V],
            64 => &[V, H, V, H, V],
            _ => return Err(Error::Invalido(format!("firma de {paginas} páginas: use 4, 8, 16, 32 o 64"))),
        };
        Self::plegar(pliegues)
    }

    /// Calcula el esquema para una secuencia de pliegues. El último pliegue
    /// debe ser vertical: es el lomo.
    pub fn plegar(pliegues: &[Pliegue]) -> Resultado<Self> {
        if pliegues.last() != Some(&Pliegue::Vertical) {
            return Err(Error::Invalido("el último pliegue debe ser vertical (es el lomo)".into()));
        }
        let verticales = pliegues.iter().filter(|p| **p == Pliegue::Vertical).count() as u32;
        let columnas = 1 << verticales;
        let filas = 1 << (pliegues.len() as u32 - verticales);

        // Cada celda física del pliego, con su posición actual tras los pliegues.
        #[derive(Clone, Copy)]
        struct Pieza {
            columna: u32,
            fila: u32,
            x: u32,
            y: u32,
            z: u32,
            espejo_x: bool,
            espejo_y: bool,
            tiro_arriba: bool,
        }
        let mut piezas: Vec<Pieza> = (0..filas)
            .flat_map(|fila| {
                (0..columnas).map(move |columna| Pieza {
                    columna,
                    fila,
                    x: columna,
                    y: fila,
                    z: 0,
                    espejo_x: false,
                    espejo_y: false,
                    tiro_arriba: true,
                })
            })
            .collect();
        let (mut ancho, mut alto, mut capas) = (columnas, filas, 1);
        for pliegue in pliegues {
            for p in &mut piezas {
                match pliegue {
                    Pliegue::Vertical => {
                        let mitad = ancho / 2;
                        if p.x < mitad {
                            // Mitad izquierda: se voltea encima, con las capas invertidas.
                            p.x = ancho - 1 - p.x - mitad;
                            p.z = 2 * capas - 1 - p.z;
                            p.espejo_x = !p.espejo_x;
                            p.tiro_arriba = !p.tiro_arriba;
                        } else {
                            p.x -= mitad;
                        }
                    }
                    Pliegue::Horizontal => {
                        let mitad = alto / 2;
                        if p.y >= mitad {
                            p.y = alto - 1 - p.y;
                            p.z = 2 * capas - 1 - p.z;
                            p.espejo_y = !p.espejo_y;
                            p.tiro_arriba = !p.tiro_arriba;
                        }
                    }
                }
            }
            match pliegue {
                Pliegue::Vertical => ancho /= 2,
                Pliegue::Horizontal => alto /= 2,
            }
            capas *= 2;
        }

        // La capa de arriba es la primera hoja; su cara superior es la página 1.
        piezas.sort_by_key(|p| std::cmp::Reverse(p.z));
        let mut posiciones = Vec::with_capacity(2 * piezas.len());
        for (hoja, p) in piezas.iter().enumerate() {
            // Con el tiro arriba la orientación depende del espejo horizontal;
            // con el retiro arriba, del vertical (el retiro se lee reflejado en x).
            let invertida = if p.tiro_arriba { p.espejo_x } else { p.espejo_y };
            let (arriba, abajo) = if p.tiro_arriba { (Lado::Tiro, Lado::Retiro) } else { (Lado::Retiro, Lado::Tiro) };
            for (pagina, lado) in [(2 * hoja as u32 + 1, arriba), (2 * hoja as u32 + 2, abajo)] {
                // El retiro se ve volteado de lado: la columna se refleja.
                let columna = if lado == Lado::Tiro { p.columna } else { columnas - 1 - p.columna };
                posiciones.push(Posicion { pagina, lado, columna, fila: p.fila, invertida });
            }
        }
        posiciones.sort_by_key(|p| p.pagina);
        Ok(Self { paginas: 2 * piezas.len() as u32, columnas, filas, pliegues: pliegues.to_vec(), posiciones })
    }

    pub fn posicion(&self, pagina: u32) -> &Posicion {
        &self.posiciones[pagina as usize - 1]
    }

    /// Páginas de un lado como cuadrícula de texto, fila superior primero
    /// (`↓` marca las invertidas). Útil para reportes y pruebas.
    pub fn diagrama(&self, lado: Lado) -> Vec<String> {
        (0..self.filas)
            .rev()
            .map(|fila| {
                (0..self.columnas)
                    .map(|columna| {
                        let p = self
                            .posiciones
                            .iter()
                            .find(|p| p.lado == lado && p.fila == fila && p.columna == columna)
                            .expect("cada celda tiene una página");
                        format!("{}{}", p.pagina, if p.invertida { "↓" } else { "" })
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect()
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn cuatro_paginas() {
        let e = Esquema::estandar(4).unwrap();
        // Tiro: interior 2 | 3. Retiro: contraportada 4 | portada 1.
        assert_eq!(e.diagrama(Lado::Tiro), ["2 3"]);
        assert_eq!(e.diagrama(Lado::Retiro), ["4 1"]);
    }

    #[test]
    fn ocho_paginas_cabeza_con_cabeza() {
        let e = Esquema::estandar(8).unwrap();
        assert_eq!(e.diagrama(Lado::Retiro), ["5↓ 4↓", "8 1"]);
        assert_eq!(e.diagrama(Lado::Tiro), ["3↓ 6↓", "2 7"]);
    }

    #[test]
    fn todos_los_esquemas_son_completos_y_coherentes() {
        for n in [4, 8, 16, 32, 64] {
            let e = Esquema::estandar(n).unwrap();
            assert_eq!(e.paginas, n);
            assert_eq!(e.columnas * e.filas * 2, n);
            let mut celdas: Vec<_> = e.posiciones.iter().map(|p| (p.lado == Lado::Tiro, p.columna, p.fila)).collect();
            celdas.sort();
            celdas.dedup();
            assert_eq!(celdas.len() as u32, n, "cada celda una sola vez en {n} pp");
            // La página 1 y la última forman el pliego exterior: mismo lado,
            // misma fila, columnas vecinas y la misma orientación.
            let (primera, ultima) = (e.posicion(1), e.posicion(n));
            assert_eq!(primera.lado, ultima.lado);
            assert_eq!(primera.fila, ultima.fila);
            assert_eq!(primera.columna.abs_diff(ultima.columna), 1);
            assert_eq!(primera.invertida, ultima.invertida);
            // Cada hoja tiene su frente y su vuelta en la misma celda física.
            for hoja in 0..n / 2 {
                let (a, b) = (e.posicion(2 * hoja + 1), e.posicion(2 * hoja + 2));
                assert_ne!(a.lado, b.lado);
                assert_eq!(a.columna, e.columnas - 1 - b.columna);
                assert_eq!(a.fila, b.fila);
            }
        }
    }

    #[test]
    fn el_lomo_debe_ser_el_ultimo_pliegue() {
        assert!(Esquema::plegar(&[Pliegue::Vertical, Pliegue::Horizontal]).is_err());
    }
}
