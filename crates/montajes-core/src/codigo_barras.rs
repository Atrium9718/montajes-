//! Código de barras EAN-13 del ISBN para la contracubierta.
//!
//! Medidas nominales GS1 al 100 %: módulo de 0,33 mm, barras de 22,85 mm,
//! guardas 5 módulos más largas, zonas libres de 11 y 7 módulos. Se dibuja
//! en vectores, en negro puro (K 100 %), nunca en cuatricromía.

use serde::Serialize;

use crate::{Error, Resultado};

/// Módulo nominal al 100 % (mm).
pub const MODULO: f64 = 0.33;
const ALTO_BARRA: f64 = 22.85;
const EXTRA_GUARDA: f64 = 5.0 * MODULO;
const ZONA_IZQ: u32 = 11;
const ZONA_DER: u32 = 7;

const L: [&str; 10] =
    ["0001101", "0011001", "0010011", "0111101", "0100011", "0110001", "0101111", "0111011", "0110111", "0001011"];
const G: [&str; 10] =
    ["0100111", "0110011", "0011011", "0100001", "0011101", "0111001", "0000101", "0010001", "0001001", "0010111"];
const R: [&str; 10] =
    ["1110010", "1100110", "1101100", "1000010", "1011100", "1001110", "1010000", "1000100", "1001000", "1110100"];
/// Paridad de los seis dígitos de la izquierda según el primero.
const PARIDAD: [&str; 10] =
    ["LLLLLL", "LLGLGG", "LLGGLG", "LLGGGL", "LGLLGG", "LGGLLG", "LGGGLL", "LGLGLG", "LGLGGL", "LGGLGL"];

/// Una barra, en mm, con el origen abajo a la izquierda del símbolo
/// (incluida la zona libre izquierda).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Barra {
    pub x: f64,
    pub y: f64,
    pub ancho: f64,
    pub alto: f64,
}

/// Un grupo de dígitos legibles bajo las barras.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Digitos {
    pub texto: String,
    /// Centro horizontal del grupo (mm).
    pub x: f64,
    /// Línea base (mm).
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CodigoBarras {
    /// Los 13 dígitos.
    pub ean: String,
    /// ISBN con guiones tal como se escribió (o los 13 dígitos).
    pub isbn: String,
    pub ancho: f64,
    pub alto: f64,
    /// Cuerpo de los dígitos (mm, altura de la letra).
    pub cuerpo: f64,
    pub barras: Vec<Barra>,
    pub digitos: Vec<Digitos>,
}

fn digito_control_13(d: &[u32]) -> u32 {
    let suma: u32 = d.iter().take(12).enumerate().map(|(i, v)| if i % 2 == 0 { *v } else { v * 3 }).sum();
    (10 - suma % 10) % 10
}

/// Normaliza un ISBN-10 o ISBN-13 (con o sin guiones) a 13 dígitos y verifica
/// el dígito de control.
pub fn isbn13(entrada: &str) -> Resultado<String> {
    let limpio: String = entrada.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_uppercase();
    let limpio = limpio.strip_prefix("ISBN").unwrap_or(&limpio).to_string();
    match limpio.len() {
        13 if limpio.chars().all(|c| c.is_ascii_digit()) => {
            let d: Vec<u32> = limpio.chars().map(|c| c.to_digit(10).unwrap_or(0)).collect();
            if !limpio.starts_with("978") && !limpio.starts_with("979") {
                return Err(Error::Invalido("un ISBN-13 empieza por 978 o 979".into()));
            }
            if digito_control_13(&d) != d[12] {
                return Err(Error::Invalido(format!(
                    "el dígito de control del ISBN no cuadra (debería terminar en {})",
                    digito_control_13(&d)
                )));
            }
            Ok(limpio)
        }
        10 => {
            let cuerpo = &limpio[..9];
            if !cuerpo.chars().all(|c| c.is_ascii_digit()) {
                return Err(Error::Invalido("el ISBN-10 solo lleva dígitos (y X al final)".into()));
            }
            let d: Vec<u32> = cuerpo.chars().map(|c| c.to_digit(10).unwrap_or(0)).collect();
            let suma: u32 = d.iter().enumerate().map(|(i, v)| v * (10 - i as u32)).sum();
            let control = (11 - suma % 11) % 11;
            let esperado = if control == 10 { 'X' } else { char::from_digit(control, 10).unwrap_or('0') };
            if !limpio.ends_with(esperado) {
                return Err(Error::Invalido(format!(
                    "el dígito de control del ISBN-10 no cuadra (debería terminar en {esperado})"
                )));
            }
            let mut d13: Vec<u32> = vec![9, 7, 8];
            d13.extend(&d);
            d13.push(0);
            d13[12] = digito_control_13(&d13);
            Ok(d13.iter().map(|v| char::from_digit(*v, 10).unwrap_or('0')).collect())
        }
        _ => Err(Error::Invalido("el ISBN debe tener 10 o 13 dígitos".into())),
    }
}

/// Patrón de 95 módulos del EAN-13 (1 = barra).
fn modulos(ean: &str) -> Vec<bool> {
    let d: Vec<usize> = ean.chars().map(|c| c.to_digit(10).unwrap_or(0) as usize).collect();
    let mut m = String::from("101");
    for (i, &v) in d[1..7].iter().enumerate() {
        m += if PARIDAD[d[0]].as_bytes()[i] == b'L' { L[v] } else { G[v] };
    }
    m += "01010";
    for &v in &d[7..13] {
        m += R[v];
    }
    m += "101";
    m.bytes().map(|b| b == b'1').collect()
}

/// Símbolo EAN-13 del ISBN a una escala (1,0 = 100 %; GS1 admite 0,8 a 2,0).
/// El alto de las barras puede recortarse (`recorte`, 0 a 0,5) si no cabe.
pub fn isbn(entrada: &str, escala: f64, recorte: f64) -> Resultado<CodigoBarras> {
    let ean = isbn13(entrada)?;
    let escala = escala.clamp(0.8, 2.0);
    let x = MODULO * escala;
    let alto_barra = ALTO_BARRA * escala * (1.0 - recorte.clamp(0.0, 0.5));
    let extra = EXTRA_GUARDA * escala;
    let cuerpo = 2.75 * escala; // altura de los dígitos (OCR-B ~ 9 pt al 100 %)
    let m = modulos(&ean);
    let es_guarda = |i: usize| i < 3 || (45..50).contains(&i) || i >= 92;
    let mut barras: Vec<Barra> = Vec::new();
    let mut i = 0;
    while i < m.len() {
        if !m[i] {
            i += 1;
            continue;
        }
        let inicio = i;
        while i < m.len() && m[i] && es_guarda(i) == es_guarda(inicio) {
            i += 1;
        }
        let guarda = es_guarda(inicio);
        // Las barras normales empiezan sobre los dígitos; las guardas bajan entre ellos.
        barras.push(Barra {
            x: (ZONA_IZQ as f64 + inicio as f64) * x,
            y: if guarda { 0.0 } else { extra },
            ancho: (i - inicio) as f64 * x,
            alto: if guarda { alto_barra + extra } else { alto_barra },
        });
    }
    let base = extra - cuerpo - 0.2 * escala;
    let centro = |desde: usize, hasta: usize| (ZONA_IZQ as f64 + (desde + hasta) as f64 / 2.0) * x;
    let digitos = vec![
        Digitos { texto: ean[0..1].into(), x: (ZONA_IZQ as f64 - 4.0) * x, y: base.max(0.0) },
        Digitos { texto: ean[1..7].into(), x: centro(3, 45), y: base.max(0.0) },
        Digitos { texto: ean[7..13].into(), x: centro(50, 92), y: base.max(0.0) },
    ];
    let ancho = (ZONA_IZQ + 95 + ZONA_DER) as f64 * x;
    let isbn = if entrada.chars().filter(|c| c.is_ascii_digit()).count() == 13 && entrada.contains('-') {
        entrada.trim().trim_start_matches("ISBN").trim().to_string()
    } else {
        ean.clone()
    };
    Ok(CodigoBarras { ean, isbn, ancho, alto: alto_barra + extra, cuerpo, barras, digitos })
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn valida_y_convierte_isbn() {
        assert_eq!(isbn13("978-84-376-0494-7").unwrap(), "9788437604947");
        assert_eq!(isbn13("0-306-40615-2").unwrap(), "9780306406157");
        assert!(isbn13("978-84-376-0494-8").is_err());
        assert!(isbn13("123").is_err());
    }

    #[test]
    fn patron_de_95_modulos_y_ancho_nominal() {
        let c = isbn("9788437604947", 1.0, 0.0).unwrap();
        assert_eq!(modulos(&c.ean).len(), 95);
        // 113 módulos con zonas libres: 37,29 mm al 100 %.
        assert!((c.ancho - 37.29).abs() < 0.01, "{}", c.ancho);
        // Las 3 guardas aportan 2 barras cada una.
        assert_eq!(c.barras.iter().filter(|b| b.y == 0.0).count(), 6);
        // Suma de módulos negros: cada dígito tiene 2 barras → 12×2 + 6 guardas = 30 barras.
        assert_eq!(c.barras.len(), 30);
    }
}
