use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("error de PDF: {0}")]
    Pdf(#[from] lopdf::Error),
    #[error("error de E/S: {0}")]
    Io(#[from] std::io::Error),
    #[error("error de JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("no existe en el catálogo: {0}")]
    NoEncontrado(String),
    #[error("ya existe en el catálogo: {0}")]
    Duplicado(String),
    #[error("la pieza no cabe en el pliego: {0}")]
    NoCabe(String),
    #[error("dato inválido: {0}")]
    Invalido(String),
}

pub type Resultado<T> = Result<T, Error>;
