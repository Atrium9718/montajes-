//! Verificación independiente de la compaginación de un libro.
//!
//! No usa el esquema con que se armó el montaje: toma los pliegos tal como
//! salen impresos (tiro y retiro, con el volteo), los corta en sus copias y
//! prueba todas las formas de doblarlos. El montaje está bien si alguna deja
//! las páginas de cada firma en orden, todas con la cabeza para el mismo lado
//! y el lomo (el último pliegue) a la izquierda de la página impar (a la
//! derecha si el libro se lee de derecha a izquierda).

use std::collections::{BTreeMap, BTreeSet};

use super::firmas::{Firma, ParametrosLibro, PlanLibro, planificar};
use super::{Cara, Volteo};

type V = (i8, i8);

fn rot_horario(v: V) -> V {
    (v.1, -v.0)
}

/// Hacia dónde apunta la cabeza de una página girada `giro` grados (horario).
fn cabeza(giro: u16) -> V {
    (0..(giro / 90) % 4).fold((0, 1), |h, _| rot_horario(h))
}

/// Una cara de una hoja: la página y sus direcciones «arriba» y «derecha»
/// vistas desde el tiro.
#[derive(Clone, Copy, Debug)]
struct Lado {
    pagina: u32,
    cabeza: V,
    derecha: V,
}

/// Páginas de una cara en coordenadas del tiro (el retiro se voltea).
fn colocar(c: &Cara, dorso: bool, volteo: Volteo) -> Vec<((i64, i64), Lado)> {
    let (w, h) = (c.pliego.ancho, c.pliego.alto);
    c.ubicaciones
        .iter()
        .map(|u| {
            let (mut x, mut y) = (u.corte.x + u.corte.ancho / 2.0, u.corte.y + u.corte.alto / 2.0);
            let mut cab = cabeza(u.giro);
            let mut der = rot_horario(cab);
            if dorso {
                match volteo {
                    Volteo::Lateral => {
                        x = w - x;
                        cab.0 = -cab.0;
                        der.0 = -der.0;
                    }
                    Volteo::Cabeza => {
                        y = h - y;
                        cab.1 = -cab.1;
                        der.1 = -der.1;
                    }
                }
            }
            let clave = ((x * 100.0).round() as i64, (y * 100.0).round() as i64);
            (clave, Lado { pagina: u.pagina as u32 + 1, cabeza: cab, derecha: der })
        })
        .collect()
}

type Matriz = [[i8; 2]; 2];

fn aplicar(m: &Matriz, v: V) -> V {
    (m[0][0] * v.0 + m[0][1] * v.1, m[1][0] * v.0 + m[1][1] * v.1)
}

fn por(a: &Matriz, b: &Matriz) -> Matriz {
    let mut r = [[0; 2]; 2];
    for (i, fila) in r.iter_mut().enumerate() {
        for (j, v) in fila.iter_mut().enumerate() {
            *v = a[i][0] * b[0][j] + a[i][1] * b[1][j];
        }
    }
    r
}

#[derive(Clone, Copy)]
struct Pieza {
    x: i32,
    y: i32,
    z: i32,
    t: Matriz,
    arriba: bool,
    frente: Lado,
    dorso: Lado,
}

/// Órdenes distintos de los pliegues (cuántos a lo ancho y cuántos a lo alto).
fn ordenes(nx: usize, ny: usize) -> Vec<Vec<bool>> {
    if nx == 0 && ny == 0 {
        return vec![vec![]];
    }
    let mut r = Vec::new();
    if nx > 0 {
        for mut resto in ordenes(nx - 1, ny) {
            resto.insert(0, true);
            r.push(resto);
        }
    }
    if ny > 0 {
        for mut resto in ordenes(nx, ny - 1) {
            resto.insert(0, false);
            r.push(resto);
        }
    }
    r
}

/// ¿Alguna forma de doblar esta hoja (bw × bh celdas) da `esperado` en orden?
fn se_puede_doblar(
    bw: usize,
    bh: usize,
    celdas: &BTreeMap<(usize, usize), (Lado, Lado)>,
    esperado: &[u32],
    rtl: bool,
) -> bool {
    if !bw.is_power_of_two() || !bh.is_power_of_two() {
        return false;
    }
    let (nx, ny) = (bw.trailing_zeros() as usize, bh.trailing_zeros() as usize);
    const ID: Matriz = [[1, 0], [0, 1]];
    const MX: Matriz = [[-1, 0], [0, 1]];
    const MY: Matriz = [[1, 0], [0, -1]];
    for orden in ordenes(nx, ny) {
        for modos in 0..(1u32 << (2 * orden.len())) {
            let mut piezas: Vec<Pieza> = celdas
                .iter()
                .map(|(&(x, y), &(frente, dorso))| Pieza {
                    x: x as i32,
                    y: y as i32,
                    z: 0,
                    t: ID,
                    arriba: true,
                    frente,
                    dorso,
                })
                .collect();
            let (mut w, mut h) = (bw as i32, bh as i32);
            let mut lomo: V = (0, 0);
            for (k, &a_lo_ancho) in orden.iter().enumerate() {
                let modo = (modos >> (2 * k)) & 3;
                let (alta, encima) = (modo & 1 == 1, modo & 2 == 2);
                let largo = if a_lo_ancho { w } else { h };
                let m = largo / 2;
                let zmax = piezas.iter().map(|p| p.z).max().unwrap_or(0);
                let zmin = piezas.iter().map(|p| p.z).min().unwrap_or(0);
                for p in &mut piezas {
                    let c = if a_lo_ancho { &mut p.x } else { &mut p.y };
                    let mueve = if alta { *c >= m } else { *c < m };
                    if mueve {
                        *c = largo - 1 - *c;
                        p.t = por(if a_lo_ancho { &MX } else { &MY }, &p.t);
                        p.arriba = !p.arriba;
                        p.z = if encima { zmax + 1 + (zmax - p.z) } else { zmin - 1 - (p.z - zmin) };
                    }
                    if !alta && *c >= m {
                        *c -= m;
                    }
                }
                if a_lo_ancho {
                    w = m;
                } else {
                    h = m;
                }
                // El lomo es el último pliegue: queda del lado por donde se dobló.
                let lado = if alta { 1 } else { -1 };
                lomo = if a_lo_ancho { (lado, 0) } else { (0, lado) };
            }
            piezas.sort_by_key(|p| -p.z);
            let caras: Vec<(u32, V, V)> = piezas
                .iter()
                .flat_map(|p| {
                    let (a, b) = if p.arriba { (p.frente, p.dorso) } else { (p.dorso, p.frente) };
                    [a, b].map(|l| (l.pagina, aplicar(&p.t, l.cabeza), aplicar(&p.t, l.derecha)))
                })
                .collect();
            for desde_abajo in [false, true] {
                let seq: Vec<&(u32, V, V)> =
                    if desde_abajo { caras.iter().rev().collect() } else { caras.iter().collect() };
                if seq.iter().map(|c| c.0).ne(esperado.iter().copied()) {
                    continue;
                }
                let cab = seq[0].1;
                let der = seq[0].2;
                let alternan = seq
                    .iter()
                    .enumerate()
                    .all(|(i, c)| c.1 == cab && c.2 == if i % 2 == 0 { der } else { (-der.0, -der.1) });
                // La página impar tiene el lomo a su izquierda (a su derecha, de derecha a izquierda).
                let lomo_ok = lomo == if rtl { der } else { (-der.0, -der.1) };
                if alternan && lomo_ok {
                    return true;
                }
            }
        }
    }
    false
}

/// Verifica que al doblar cada pliego impreso las firmas queden en orden y al
/// derecho. Devuelve qué falla, si algo falla.
pub fn verificar(p: &ParametrosLibro, plan: &PlanLibro) -> Result<(), String> {
    // Con páginas en blanco (sin ubicación) o con mapa, se verifica el mismo
    // montaje rehecho sin ellas: así cada celda tiene su página del libro.
    let rehecho;
    let plan = if plan.blancas > 0 || !p.mapa.is_empty() {
        let completo = ParametrosLibro { paginas: plan.paginas_libro, mapa: Vec::new(), ..p.clone() };
        rehecho = planificar(&completo).map_err(|e| e.to_string())?;
        &rehecho
    } else {
        plan
    };
    let firmas: Vec<&Firma> = plan.firmas.iter().collect();
    let mut i = 0;
    while i < plan.caras.len() {
        let c = &plan.caras[i];
        let nombre = c.nombre.to_lowercase();
        let (tiro, retiro, volteo) = if nombre.contains("tira y retira") {
            i += 1;
            (c, c, Volteo::Lateral)
        } else {
            i += 2;
            (c, plan.caras.get(i - 1).ok_or_else(|| format!("{} no tiene retiro", c.nombre))?, p.volteo)
        };
        let frente = colocar(tiro, false, volteo);
        let dorso = colocar(retiro, true, volteo);
        let xs: BTreeSet<i64> = frente.iter().map(|(k, _)| k.0).collect();
        let ys: BTreeSet<i64> = frente.iter().map(|(k, _)| k.1).collect();
        let (xs, ys): (Vec<i64>, Vec<i64>) = (xs.into_iter().collect(), ys.into_iter().collect());
        let indice = |k: (i64, i64)| Some((xs.iter().position(|x| *x == k.0)?, ys.iter().position(|y| *y == k.1)?));
        let mut celdas: BTreeMap<(usize, usize), (Lado, Option<Lado>)> = BTreeMap::new();
        for (k, l) in &frente {
            celdas.insert(indice(*k).ok_or("tiro fuera de grilla")?, (*l, None));
        }
        for (k, l) in &dorso {
            let celda = indice(*k).and_then(|ix| celdas.get_mut(&ix));
            match celda {
                Some(c) => c.1 = Some(*l),
                None => return Err(format!("{}: una página del retiro no cae detrás de ninguna del tiro", c.nombre)),
            }
        }
        let celdas: BTreeMap<(usize, usize), (Lado, Lado)> = celdas
            .into_iter()
            .map(|(k, (f, d))| d.map(|d| (k, (f, d))).ok_or_else(|| format!("{}: hay páginas sin retiro", c.nombre)))
            .collect::<Result<_, _>>()?;
        // Se corta el pliego en sus copias: bloques iguales con una firma entera cada uno.
        let (cols, filas) = (xs.len(), ys.len());
        let paginas: BTreeSet<u32> = celdas.values().flat_map(|(f, d)| [f.pagina, d.pagina]).collect();
        let k = firmas
            .iter()
            .find(|f| f.paginas_libro.iter().all(|x| paginas.contains(x)))
            .map(|f| f.paginas as usize)
            .ok_or_else(|| format!("{}: no lleva ninguna firma completa", c.nombre))?;
        // Bloques de bw × bh celdas (las hojas de cada firma); con firmas
        // combinadas puede haber lugares vacíos.
        let celdas_firma = (k / 2).max(1);
        let mut hecho = false;
        'cortes: for bw in (1..=cols).filter(|bw| cols % bw == 0 && celdas_firma % bw == 0) {
            let bh = celdas_firma / bw;
            if bh == 0 || filas % bh != 0 {
                continue;
            }
            let mut bloques = Vec::new();
            for bi in 0..cols / bw {
                for bj in 0..filas / bh {
                    let sub: BTreeMap<(usize, usize), (Lado, Lado)> = celdas
                        .iter()
                        .filter(|((x, y), _)| {
                            (bi * bw..(bi + 1) * bw).contains(x) && (bj * bh..(bj + 1) * bh).contains(y)
                        })
                        .map(|(&(x, y), v)| ((x - bi * bw, y - bj * bh), *v))
                        .collect();
                    if sub.is_empty() {
                        continue;
                    }
                    let pags: BTreeSet<u32> = sub.values().flat_map(|(f, d)| [f.pagina, d.pagina]).collect();
                    let Some(firma) =
                        firmas.iter().find(|f| f.paginas_libro.iter().copied().collect::<BTreeSet<_>>() == pags)
                    else {
                        continue 'cortes;
                    };
                    if sub.len() != celdas_firma {
                        continue 'cortes;
                    }
                    bloques.push((sub, *firma));
                }
            }
            for (sub, firma) in &bloques {
                if !se_puede_doblar(bw, bh, sub, &firma.paginas_libro, p.derecha_a_izquierda) {
                    return Err(format!(
                        "{}: al doblar la firma {} sus páginas no quedan en orden y al derecho",
                        c.nombre, firma.numero
                    ));
                }
            }
            hecho = true;
            break;
        }
        if !hecho {
            return Err(format!("{}: el pliego no se puede cortar en firmas completas", c.nombre));
        }
    }
    Ok(())
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use crate::geometria::Tamano;
    use crate::imposicion::firmas::{Aprovechamiento, Encuadernacion};
    use crate::imposicion::marcas::OpcionesMarcas;
    use crate::imposicion::nup::Orientacion;
    use crate::imposicion::{Margenes, Volteo};

    #[test]
    fn todos_los_montajes_se_doblan_bien() {
        let mut probados = 0;
        for (pw, ph) in [(740.0, 530.0), (480.0, 330.0), (1000.0, 700.0)] {
            for (aw, ah) in [(148.0, 210.0), (216.0, 279.0), (105.0, 148.0)] {
                for firma in [None, Some(4), Some(8), Some(16), Some(32)] {
                    for e in [Encuadernacion::Lomo, Encuadernacion::Caballete] {
                        for o in [Orientacion::Normal, Orientacion::Girada] {
                            for v in [Volteo::Lateral, Volteo::Cabeza] {
                                for a in [Aprovechamiento::Una, Aprovechamiento::Auto, Aprovechamiento::Combinar] {
                                    for rtl in [false, true] {
                                        let p = ParametrosLibro {
                                            pliego: Tamano::new(pw, ph),
                                            margenes: Margenes { pinza: 10.0, cola: 10.0, lateral: 5.0 },
                                            pagina: Tamano::new(aw, ah),
                                            paginas: 46,
                                            encuadernacion: e,
                                            firma,
                                            rebase: 3.0,
                                            fresado: 3.0,
                                            refile: 3.0,
                                            calibre_mm: None,
                                            derecha_a_izquierda: rtl,
                                            marcas: OpcionesMarcas::default(),
                                            aprovechamiento: a,
                                            cuadernillos: vec![],
                                            mapa: vec![],
                                            volteo: v,
                                            orientacion: o,
                                        };
                                        let Ok(plan) = planificar(&p) else { continue };
                                        probados += 1;
                                        if let Err(e) = verificar(&p, &plan) {
                                            panic!(
                                                "{pw}x{ph} {aw}x{ah} {firma:?} {e:?} {o:?} {v:?} {a:?} rtl {rtl}: {e}"
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(probados > 100, "{probados}");
    }

    #[test]
    fn detecta_un_montaje_mal_hecho() {
        let p = ParametrosLibro {
            pliego: Tamano::new(740.0, 530.0),
            margenes: Margenes { pinza: 10.0, cola: 10.0, lateral: 5.0 },
            pagina: Tamano::new(148.0, 210.0),
            paginas: 16,
            encuadernacion: Encuadernacion::Lomo,
            firma: Some(16),
            rebase: 3.0,
            fresado: 3.0,
            refile: 3.0,
            calibre_mm: None,
            derecha_a_izquierda: false,
            marcas: OpcionesMarcas::default(),
            aprovechamiento: Aprovechamiento::Una,
            cuadernillos: vec![],
            mapa: vec![],
            volteo: Volteo::Lateral,
            orientacion: Orientacion::Normal,
        };
        let plan = planificar(&p).unwrap();
        assert!(verificar(&p, &plan).is_ok());
        // Con el retiro volteado de cabeza (como si la máquina volteara distinto) ya no cuadra.
        let otro = ParametrosLibro { volteo: Volteo::Cabeza, ..p.clone() };
        assert!(verificar(&otro, &plan).is_err());
    }
}
