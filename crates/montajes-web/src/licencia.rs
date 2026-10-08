//! Licencia de Macula: el servidor firma (ECDSA P-256) un token con la
//! empresa y la vigencia de su suscripción, y el motor lo exige para trabajar.
//! Sin un token vigente no se calcula ni se genera ningún PDF.

use std::cell::RefCell;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsError;

/// Llave pública de licencias (SEC1 sin comprimir). La privada solo está en el servidor.
const PUBLICA: &str = "043f122f0c7f751de114d6666562f6483a2886b74e5a52fdfc45cbd42e37a2f73e390e55d5e8758f40c85fc972144cf068edd224e03b401a0c8206ab5ed3b6456d";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Licencia {
    /// Empresa.
    pub e: u64,
    /// Nombre de la empresa.
    pub n: String,
    /// Plan.
    pub p: String,
    /// Vigente hasta (segundos Unix).
    pub h: i64,
    /// Emitida (segundos Unix).
    pub i: i64,
}

thread_local! {
    static ACTUAL: RefCell<Option<Licencia>> = const { RefCell::new(None) };
}

fn ahora() -> i64 {
    (js_sys::Date::now() / 1000.0) as i64
}

fn llave() -> Option<VerifyingKey> {
    let bytes: Vec<u8> =
        (0..PUBLICA.len()).step_by(2).filter_map(|i| u8::from_str_radix(&PUBLICA[i..i + 2], 16).ok()).collect();
    VerifyingKey::from_sec1_bytes(&bytes).ok()
}

/// Verifica la firma y la vigencia del token, y lo deja activo.
pub fn activar(token: &str) -> Result<Licencia, String> {
    let (datos, firma) = token.split_once('.').ok_or("licencia mal formada")?;
    let datos = URL_SAFE_NO_PAD.decode(datos).map_err(|_| "licencia mal formada")?;
    let firma = URL_SAFE_NO_PAD.decode(firma).map_err(|_| "licencia mal formada")?;
    let firma = Signature::from_der(&firma).map_err(|_| "firma de licencia inválida")?;
    llave().ok_or("llave de licencias inválida")?.verify(&datos, &firma).map_err(|_| "la licencia no es auténtica")?;
    let l: Licencia = serde_json::from_slice(&datos).map_err(|_| "licencia mal formada")?;
    if l.h <= ahora() {
        return Err("la licencia venció: vuelve a entrar con conexión a internet".into());
    }
    ACTUAL.with(|a| *a.borrow_mut() = Some(l.clone()));
    Ok(l)
}

/// Error si no hay una licencia vigente.
pub fn exigir() -> Result<(), JsError> {
    let vigente = ACTUAL.with(|a| a.borrow().as_ref().is_some_and(|l| l.h > ahora()));
    if vigente {
        Ok(())
    } else {
        Err(JsError::new(
            "La suscripción de Macula no está activa en este equipo. Entra con internet o revisa Mi cuenta.",
        ))
    }
}
