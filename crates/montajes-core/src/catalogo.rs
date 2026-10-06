//! Catálogos editables por el usuario: máquinas y papeles.
//!
//! Cada catálogo se guarda como un archivo JSON legible dentro de la carpeta
//! de datos (por defecto `~/.montajes`), para poder respaldarlo, versionarlo
//! o copiarlo a otro equipo del taller.

use std::fs;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::geometria::Tamano;
use crate::{Error, Resultado};

/// Un elemento de catálogo identificado por un `id` corto (p. ej. `gto52`).
pub trait ElementoCatalogo: Serialize + DeserializeOwned + Clone {
    const ARCHIVO: &'static str;
    fn id(&self) -> &str;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TipoMaquina {
    Offset,
    Digital,
    GranFormato,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VersionPdfx {
    #[serde(rename = "PDF/X-4")]
    X4,
    #[serde(rename = "PDF/X-1a")]
    X1a,
}

/// Cómo espera recibir los archivos el RIP/CTP de esta máquina.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PerfilSalida {
    pub pdfx: VersionPdfx,
    /// Perfil ICC de la condición de impresión (OutputIntent).
    #[serde(default)]
    pub perfil_icc: Option<PathBuf>,
    /// Identificador de la condición, p. ej. `FOGRA39`, `FOGRA51`, `GRACoL2013`.
    #[serde(default)]
    pub condicion: Option<String>,
    /// Generar JDF junto al PDF (flujos Prinergy, Apogee, Prinect…).
    #[serde(default)]
    pub jdf: bool,
}

impl Default for PerfilSalida {
    fn default() -> Self {
        Self { pdfx: VersionPdfx::X4, perfil_icc: None, condicion: None, jdf: false }
    }
}

/// Plancha offset: tamaño y distancia desde el borde de la plancha al borde
/// de pinza del pliego.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Plancha {
    pub tamano: Tamano,
    pub desfase_pinza: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Maquina {
    pub id: String,
    pub nombre: String,
    pub tipo: TipoMaquina,
    pub pliego_max: Tamano,
    #[serde(default)]
    pub pliego_min: Option<Tamano>,
    /// Margen no imprimible del borde de pinza (mm). En la salida la pinza
    /// siempre queda en el borde inferior del pliego.
    pub pinza: f64,
    /// Margen no imprimible del borde opuesto a la pinza (mm).
    pub cola: f64,
    /// Margen no imprimible de cada lado (mm).
    pub lateral: f64,
    #[serde(default)]
    pub plancha: Option<Plancha>,
    /// Número de cuerpos o colores por pasada.
    #[serde(default = "cuatro")]
    pub colores: u8,
    /// Imprime ambas caras en una pasada (perfecting o dúplex digital).
    #[serde(default)]
    pub duplex: bool,
    #[serde(default)]
    pub salida: PerfilSalida,
    #[serde(default)]
    pub notas: String,
}

fn cuatro() -> u8 {
    4
}

impl ElementoCatalogo for Maquina {
    const ARCHIVO: &'static str = "maquinas.json";
    fn id(&self) -> &str {
        &self.id
    }
}

impl Maquina {
    pub fn validar(&self) -> Resultado<()> {
        validar_id(&self.id)?;
        if self.pinza < 0.0 || self.cola < 0.0 || self.lateral < 0.0 {
            return Err(Error::Invalido("los márgenes no pueden ser negativos".into()));
        }
        if self.pinza + self.cola >= self.pliego_max.alto || 2.0 * self.lateral >= self.pliego_max.ancho {
            return Err(Error::Invalido("los márgenes ocupan todo el pliego".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fibra {
    /// Fibra paralela al lado largo del pliego.
    Larga,
    /// Fibra paralela al lado corto del pliego.
    Corta,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Papel {
    pub id: String,
    pub nombre: String,
    /// Gramaje en g/m².
    pub gramaje: f64,
    /// Calibre (espesor) en micras.
    pub calibre_um: f64,
    #[serde(default)]
    pub estucado: bool,
    #[serde(default)]
    pub fibra: Option<Fibra>,
    /// Formatos de pliego en que se compra (mm).
    #[serde(default)]
    pub pliegos: Vec<Tamano>,
    #[serde(default)]
    pub notas: String,
}

impl ElementoCatalogo for Papel {
    const ARCHIVO: &'static str = "papeles.json";
    fn id(&self) -> &str {
        &self.id
    }
}

impl Papel {
    pub fn calibre_mm(&self) -> f64 {
        self.calibre_um / 1000.0
    }

    /// Volumen específico (cm³/g): cuánto "abulta" el papel.
    pub fn volumen(&self) -> f64 {
        self.calibre_um / self.gramaje
    }

    pub fn validar(&self) -> Resultado<()> {
        validar_id(&self.id)?;
        if self.gramaje <= 0.0 || self.calibre_um <= 0.0 {
            return Err(Error::Invalido("gramaje y calibre deben ser positivos".into()));
        }
        Ok(())
    }
}

fn validar_id(id: &str) -> Resultado<()> {
    let ok = !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok { Ok(()) } else { Err(Error::Invalido(format!("id «{id}»: use solo letras, números, - y _"))) }
}

/// Lista de elementos persistida en `<carpeta>/<ARCHIVO>`.
pub struct Catalogo<T: ElementoCatalogo> {
    ruta: PathBuf,
    elementos: Vec<T>,
}

impl<T: ElementoCatalogo> Catalogo<T> {
    pub fn abrir(carpeta: &Path) -> Resultado<Self> {
        let ruta = carpeta.join(T::ARCHIVO);
        let elementos = if ruta.exists() { serde_json::from_str(&fs::read_to_string(&ruta)?)? } else { Vec::new() };
        Ok(Self { ruta, elementos })
    }

    pub fn todos(&self) -> &[T] {
        &self.elementos
    }

    pub fn obtener(&self, id: &str) -> Resultado<&T> {
        self.elementos.iter().find(|e| e.id() == id).ok_or_else(|| Error::NoEncontrado(id.to_string()))
    }

    /// Agrega un elemento nuevo; con `reemplazar` actualiza uno existente.
    pub fn guardar(&mut self, elemento: T, reemplazar: bool) -> Resultado<()> {
        match self.elementos.iter().position(|e| e.id() == elemento.id()) {
            Some(i) if reemplazar => self.elementos[i] = elemento,
            Some(_) => return Err(Error::Duplicado(elemento.id().to_string())),
            None => self.elementos.push(elemento),
        }
        self.escribir()
    }

    pub fn borrar(&mut self, id: &str) -> Resultado<T> {
        let i = self.elementos.iter().position(|e| e.id() == id).ok_or_else(|| Error::NoEncontrado(id.to_string()))?;
        let elemento = self.elementos.remove(i);
        self.escribir()?;
        Ok(elemento)
    }

    fn escribir(&self) -> Resultado<()> {
        if let Some(carpeta) = self.ruta.parent() {
            fs::create_dir_all(carpeta)?;
        }
        // Escritura atómica: un corte de luz no deja el catálogo a medias.
        let temporal = self.ruta.with_extension("json.tmp");
        fs::write(&temporal, serde_json::to_string_pretty(&self.elementos)?)?;
        fs::rename(temporal, &self.ruta)?;
        Ok(())
    }
}

/// Biblioteca inicial de papeles comunes con calibres típicos de mercado.
/// Son valores de referencia: conviene reemplazarlos por los de la ficha
/// técnica del proveedor.
pub fn papeles_de_referencia() -> Vec<Papel> {
    let p = |id: &str, nombre: &str, gramaje: f64, calibre_um: f64, estucado: bool| Papel {
        id: id.into(),
        nombre: nombre.into(),
        gramaje,
        calibre_um,
        estucado,
        fibra: None,
        pliegos: vec![Tamano::new(700.0, 1000.0)],
        notas: "Calibre típico de referencia; verificar con la ficha del proveedor.".into(),
    };
    vec![
        p("bond60", "Bond 60 g", 60.0, 78.0, false),
        p("bond75", "Bond 75 g", 75.0, 98.0, false),
        p("bond90", "Bond 90 g", 90.0, 117.0, false),
        p("bond115", "Bond 115 g", 115.0, 145.0, false),
        p("libro-ahuesado70", "Libro ahuesado (book cream) 70 g", 70.0, 112.0, false),
        p("libro-ahuesado80", "Libro ahuesado (book cream) 80 g", 80.0, 128.0, false),
        p("periodico48", "Periódico 48,8 g", 48.8, 70.0, false),
        p("brillante90", "Estucado brillante (propalcote) 90 g", 90.0, 75.0, true),
        p("brillante115", "Estucado brillante (propalcote) 115 g", 115.0, 95.0, true),
        p("brillante150", "Estucado brillante (propalcote) 150 g", 150.0, 125.0, true),
        p("brillante200", "Estucado brillante (propalcote) 200 g", 200.0, 175.0, true),
        p("brillante250", "Estucado brillante (propalcote) 250 g", 250.0, 230.0, true),
        p("brillante300", "Estucado brillante (propalcote) 300 g", 300.0, 285.0, true),
        p("mate115", "Estucado mate 115 g", 115.0, 105.0, true),
        p("mate150", "Estucado mate 150 g", 150.0, 140.0, true),
        p("mate300", "Estucado mate 300 g", 300.0, 310.0, true),
        p("opalina120", "Opalina 120 g", 120.0, 140.0, false),
        p("opalina180", "Opalina 180 g", 180.0, 210.0, false),
        p("esmaltada-c1s12", "Cartulina esmaltada C1S 12 pt", 240.0, 305.0, true),
        p("kraft125", "Kraft 125 g", 125.0, 165.0, false),
    ]
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn carpeta_temporal(nombre: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("montajes-prueba-{nombre}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn maquina(id: &str) -> Maquina {
        Maquina {
            id: id.into(),
            nombre: "GTO 52".into(),
            tipo: TipoMaquina::Offset,
            pliego_max: Tamano::new(520.0, 360.0),
            pliego_min: None,
            pinza: 10.0,
            cola: 5.0,
            lateral: 5.0,
            plancha: None,
            colores: 1,
            duplex: false,
            salida: PerfilSalida::default(),
            notas: String::new(),
        }
    }

    #[test]
    fn guardar_leer_y_borrar() {
        let dir = carpeta_temporal("catalogo");
        let mut cat = Catalogo::<Maquina>::abrir(&dir).unwrap();
        cat.guardar(maquina("gto52"), false).unwrap();
        assert!(matches!(cat.guardar(maquina("gto52"), false), Err(Error::Duplicado(_))));

        let releido = Catalogo::<Maquina>::abrir(&dir).unwrap();
        assert_eq!(releido.obtener("gto52").unwrap().pliego_max, Tamano::new(520.0, 360.0));

        let mut cat = releido;
        cat.borrar("gto52").unwrap();
        assert!(Catalogo::<Maquina>::abrir(&dir).unwrap().todos().is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn valida_ids_y_margenes() {
        assert!(maquina("con espacio").validar().is_err());
        let mut m = maquina("ok");
        m.pinza = 400.0;
        assert!(m.validar().is_err());
        for papel in papeles_de_referencia() {
            papel.validar().unwrap();
        }
    }
}
