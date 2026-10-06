//! Núcleo de Montajes: modelo de datos, catálogos, cálculo de imposición y
//! escritura del PDF de salida.
//!
//! Todas las medidas del modelo están en milímetros; la conversión a puntos
//! PDF se hace solo al escribir el archivo (ver [`unidades`]).

pub mod catalogo;
pub mod error;
pub mod geometria;
pub mod imposicion;
pub mod libro;
pub mod pdf;
pub mod portada;
pub mod unidades;

pub use error::{Error, Resultado};
