//! `montajes`: línea de comandos del motor de imposición.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use montajes_core::catalogo::{
    Catalogo, Fibra, Maquina, Papel, PerfilSalida, Plancha, TipoMaquina, VersionPdfx, papeles_de_referencia,
};
use montajes_core::geometria::Tamano;
use montajes_core::imposicion::Cara;
use montajes_core::imposicion::firmas::{self, Encuadernacion, ParametrosLibro};
use montajes_core::imposicion::marcas::OpcionesMarcas;
use montajes_core::imposicion::nup::{self, Orientacion, ParametrosNup};
use montajes_core::imposicion::{Margenes, Volteo};
use montajes_core::libro;
use montajes_core::pdf::{self, Fuente, OpcionesSalida};

#[derive(Parser)]
#[command(
    name = "montajes",
    version,
    about = "Imposición automática para imprenta: entrega el PDF listo para imprimir"
)]
struct Cli {
    /// Carpeta de catálogos (por defecto $MONTAJES_DATOS o ~/.montajes).
    #[arg(long, global = true)]
    datos: Option<PathBuf>,
    #[command(subcommand)]
    comando: Comando,
}

#[derive(Subcommand)]
enum Comando {
    /// Máquinas de impresión (offset y digital) con sus márgenes y salida.
    #[command(subcommand)]
    Maquina(ComandoMaquina),
    /// Papeles con gramaje, calibre y fibra.
    #[command(subcommand)]
    Papel(ComandoPapel),
    /// Muestra tamaño final, rebase y giro de cada página de un PDF.
    Info { pdf: PathBuf },
    /// Calcula el lomo de un libro al lomo (PUR, hot-melt, cosido).
    Lomo(ArgsLomo),
    /// Montaje de piezas repetidas: volantes, tarjetas, etiquetas.
    Nup(ArgsNup),
    /// Libros y revistas por firmas: caballete, al lomo o cosido.
    Libro(ArgsLibro),
}

#[derive(Subcommand)]
enum ComandoMaquina {
    /// Crea o actualiza una máquina.
    Agregar(ArgsMaquina),
    Listar,
    Ver {
        id: String,
    },
    Borrar {
        id: String,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Tipo {
    Offset,
    Digital,
    GranFormato,
}

#[derive(Clone, Copy, ValueEnum)]
enum Pdfx {
    X4,
    X1a,
}

#[derive(Args)]
struct ArgsMaquina {
    /// Identificador corto, p. ej. `gto52` o `xerox-v180`.
    #[arg(long)]
    id: String,
    #[arg(long)]
    nombre: String,
    #[arg(long, value_enum)]
    tipo: Tipo,
    /// Pliego máximo en mm, ancho×alto con la pinza abajo (p. ej. 520x360).
    #[arg(long, value_parser = parse_tamano)]
    pliego_max: Tamano,
    #[arg(long, value_parser = parse_tamano)]
    pliego_min: Option<Tamano>,
    /// Margen de pinza en mm.
    #[arg(long)]
    pinza: f64,
    /// Margen de cola en mm.
    #[arg(long, default_value_t = 5.0)]
    cola: f64,
    /// Margen lateral en mm (cada lado).
    #[arg(long, default_value_t = 5.0)]
    lateral: f64,
    /// Tamaño de plancha en mm (offset).
    #[arg(long, value_parser = parse_tamano)]
    plancha: Option<Tamano>,
    /// Distancia del borde de la plancha a la pinza del pliego (mm).
    #[arg(long, default_value_t = 0.0)]
    plancha_desfase: f64,
    #[arg(long, default_value_t = 4)]
    colores: u8,
    /// Imprime ambas caras en una pasada.
    #[arg(long)]
    duplex: bool,
    #[arg(long, value_enum, default_value = "x4")]
    pdfx: Pdfx,
    /// Perfil ICC CMYK de la condición de impresión.
    #[arg(long)]
    icc: Option<PathBuf>,
    /// Condición de salida, p. ej. FOGRA39, FOGRA51, GRACoL2013.
    #[arg(long)]
    condicion: Option<String>,
    /// El RIP/CTP de esta máquina recibe JDF.
    #[arg(long)]
    jdf: bool,
    #[arg(long, default_value = "")]
    notas: String,
    /// Reemplazar si ya existe.
    #[arg(long)]
    reemplazar: bool,
}

#[derive(Subcommand)]
enum ComandoPapel {
    /// Crea o actualiza un papel.
    Agregar(ArgsPapel),
    Listar,
    Ver {
        id: String,
    },
    Borrar {
        id: String,
    },
    /// Carga una biblioteca inicial de papeles comunes (sin pisar los existentes).
    Semilla,
}

#[derive(Clone, Copy, ValueEnum)]
enum FibraArg {
    Larga,
    Corta,
}

#[derive(Args)]
struct ArgsPapel {
    #[arg(long)]
    id: String,
    #[arg(long)]
    nombre: String,
    /// Gramaje en g/m².
    #[arg(long)]
    gramaje: f64,
    /// Calibre en micras (µm).
    #[arg(long)]
    calibre: f64,
    #[arg(long)]
    estucado: bool,
    #[arg(long, value_enum)]
    fibra: Option<FibraArg>,
    /// Formato de pliego en mm (se puede repetir).
    #[arg(long = "pliego", value_parser = parse_tamano)]
    pliegos: Vec<Tamano>,
    #[arg(long, default_value = "")]
    notas: String,
    #[arg(long)]
    reemplazar: bool,
}

#[derive(Args)]
struct ArgsLomo {
    /// Páginas de la tripa (interior).
    #[arg(long)]
    paginas: u32,
    /// Papel de la tripa (id del catálogo).
    #[arg(long)]
    papel: String,
    /// Papel de la portada (id del catálogo).
    #[arg(long)]
    portada: Option<String>,
    /// Holgura adicional en mm (pegante, cartón, etc.).
    #[arg(long, default_value_t = 0.0)]
    tolerancia: f64,
}

#[derive(Clone, Copy, ValueEnum)]
enum OrientacionArg {
    Auto,
    Normal,
    Girada,
}

#[derive(Clone, Copy, ValueEnum)]
enum VolteoArg {
    /// Tira y retira (work & turn): voltea sobre el lado de la pinza.
    Lateral,
    /// Work & tumble: voltea de cabeza y cambia la pinza.
    Cabeza,
}

#[derive(Args)]
struct ArgsNup {
    /// PDF de entrada.
    entrada: PathBuf,
    /// PDF de salida.
    #[arg(short, long)]
    salida: PathBuf,
    /// Máquina del catálogo.
    #[arg(short, long)]
    maquina: String,
    /// Pliego en mm (por defecto el máximo de la máquina).
    #[arg(long, value_parser = parse_tamano)]
    pliego: Option<Tamano>,
    /// Formato final en mm (por defecto el TrimBox del PDF).
    #[arg(long, value_parser = parse_tamano)]
    formato: Option<Tamano>,
    /// Rebase en mm.
    #[arg(long, default_value_t = 3.0)]
    rebase: f64,
    /// Calle entre piezas en mm; 0 = corte compartido.
    #[arg(long, default_value_t = 0.0)]
    calle: f64,
    #[arg(long, value_enum, default_value = "auto")]
    orientacion: OrientacionArg,
    /// Las páginas vienen en pares frente/dorso.
    #[arg(long)]
    dorso: bool,
    #[arg(long, value_enum, default_value = "lateral")]
    volteo: VolteoArg,
    #[arg(long)]
    sin_marcas: bool,
    #[arg(long)]
    sin_tira_color: bool,
    /// Solo calcular y mostrar el montaje, sin escribir el PDF.
    #[arg(long)]
    simular: bool,
}

#[derive(Clone, Copy, ValueEnum)]
enum EncuadernacionArg {
    /// Grapa: firmas anidadas, con creep.
    Caballete,
    /// PUR o hot-melt: firmas alzadas, lomo fresado.
    Lomo,
    /// Cosido con hilo: firmas alzadas, sin fresado.
    Cosido,
}

#[derive(Args)]
struct ArgsLibro {
    /// PDF de la tripa (interior), página por página.
    entrada: PathBuf,
    /// PDF de salida.
    #[arg(short, long)]
    salida: PathBuf,
    /// Máquina del catálogo.
    #[arg(short, long)]
    maquina: String,
    #[arg(short, long, value_enum)]
    encuadernacion: EncuadernacionArg,
    /// Papel de la tripa (para creep y lomo).
    #[arg(long)]
    papel: Option<String>,
    /// Páginas por firma: 4, 8, 16, 32 o 64 (por defecto la mayor que quepa).
    #[arg(long)]
    firma: Option<u32>,
    /// Pliego en mm (por defecto el máximo de la máquina).
    #[arg(long, value_parser = parse_tamano)]
    pliego: Option<Tamano>,
    /// Formato final en mm (por defecto el TrimBox del PDF).
    #[arg(long, value_parser = parse_tamano)]
    formato: Option<Tamano>,
    #[arg(long, default_value_t = 3.0)]
    rebase: f64,
    /// Fresado del lomo en mm (solo al lomo).
    #[arg(long, default_value_t = 3.0)]
    fresado: f64,
    /// Refile en cabeza, pie y frente en mm.
    #[arg(long, default_value_t = 3.0)]
    refile: f64,
    /// No compensar el creep en caballete.
    #[arg(long)]
    sin_creep: bool,
    /// Lectura de derecha a izquierda.
    #[arg(long)]
    derecha_a_izquierda: bool,
    #[arg(long)]
    sin_marcas: bool,
    #[arg(long)]
    sin_tira_color: bool,
    /// Solo calcular y mostrar el plan, sin escribir el PDF.
    #[arg(long)]
    simular: bool,
}

fn parse_tamano(s: &str) -> Result<Tamano, String> {
    Tamano::parse(s).ok_or_else(|| format!("«{s}» no es un tamaño válido; use ancho×alto en mm, p. ej. 450x320"))
}

fn carpeta_datos(cli: &Cli) -> Result<PathBuf> {
    if let Some(d) = &cli.datos {
        return Ok(d.clone());
    }
    if let Some(d) = std::env::var_os("MONTAJES_DATOS") {
        return Ok(d.into());
    }
    let casa = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .context("no se encontró la carpeta personal; use --datos")?;
    Ok(Path::new(&casa).join(".montajes"))
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let datos = carpeta_datos(&cli)?;
    match cli.comando {
        Comando::Maquina(c) => maquina(&datos, c),
        Comando::Papel(c) => papel(&datos, c),
        Comando::Info { pdf } => info(&pdf),
        Comando::Lomo(a) => lomo(&datos, a),
        Comando::Nup(a) => nup(&datos, a),
        Comando::Libro(a) => libro_cmd(&datos, a),
    }
}

fn maquina(datos: &Path, c: ComandoMaquina) -> Result<()> {
    let mut cat = Catalogo::<Maquina>::abrir(datos)?;
    match c {
        ComandoMaquina::Agregar(a) => {
            let m = Maquina {
                id: a.id,
                nombre: a.nombre,
                tipo: match a.tipo {
                    Tipo::Offset => TipoMaquina::Offset,
                    Tipo::Digital => TipoMaquina::Digital,
                    Tipo::GranFormato => TipoMaquina::GranFormato,
                },
                pliego_max: a.pliego_max,
                pliego_min: a.pliego_min,
                pinza: a.pinza,
                cola: a.cola,
                lateral: a.lateral,
                plancha: a.plancha.map(|tamano| Plancha { tamano, desfase_pinza: a.plancha_desfase }),
                colores: a.colores,
                duplex: a.duplex,
                salida: PerfilSalida {
                    pdfx: match a.pdfx {
                        Pdfx::X4 => VersionPdfx::X4,
                        Pdfx::X1a => VersionPdfx::X1a,
                    },
                    perfil_icc: a.icc.map(|p| p.canonicalize().unwrap_or(p)),
                    condicion: a.condicion,
                    jdf: a.jdf,
                },
                notas: a.notas,
            };
            m.validar()?;
            let id = m.id.clone();
            cat.guardar(m, a.reemplazar)?;
            println!("Máquina «{id}» guardada en {}", datos.display());
        }
        ComandoMaquina::Listar => {
            if cat.todos().is_empty() {
                println!("No hay máquinas. Cree una con `montajes maquina agregar`.");
            }
            for m in cat.todos() {
                println!(
                    "{:<16} {:<28} {:<12} pliego máx {}  pinza {} mm",
                    m.id,
                    m.nombre,
                    format!("{:?}", m.tipo).to_lowercase(),
                    m.pliego_max,
                    m.pinza
                );
            }
        }
        ComandoMaquina::Ver { id } => println!("{}", serde_json::to_string_pretty(cat.obtener(&id)?)?),
        ComandoMaquina::Borrar { id } => {
            cat.borrar(&id)?;
            println!("Máquina «{id}» borrada.");
        }
    }
    Ok(())
}

fn papel(datos: &Path, c: ComandoPapel) -> Result<()> {
    let mut cat = Catalogo::<Papel>::abrir(datos)?;
    match c {
        ComandoPapel::Agregar(a) => {
            let p = Papel {
                id: a.id,
                nombre: a.nombre,
                gramaje: a.gramaje,
                calibre_um: a.calibre,
                estucado: a.estucado,
                fibra: a.fibra.map(|f| match f {
                    FibraArg::Larga => Fibra::Larga,
                    FibraArg::Corta => Fibra::Corta,
                }),
                pliegos: a.pliegos,
                notas: a.notas,
            };
            p.validar()?;
            let id = p.id.clone();
            cat.guardar(p, a.reemplazar)?;
            println!("Papel «{id}» guardado en {}", datos.display());
        }
        ComandoPapel::Listar => {
            if cat.todos().is_empty() {
                println!("No hay papeles. Use `montajes papel semilla` o `montajes papel agregar`.");
            }
            for p in cat.todos() {
                println!("{:<18} {:<40} {:>6} g/m²  {:>5} µm", p.id, p.nombre, p.gramaje, p.calibre_um);
            }
        }
        ComandoPapel::Ver { id } => println!("{}", serde_json::to_string_pretty(cat.obtener(&id)?)?),
        ComandoPapel::Borrar { id } => {
            cat.borrar(&id)?;
            println!("Papel «{id}» borrado.");
        }
        ComandoPapel::Semilla => {
            let mut agregados = 0;
            for p in papeles_de_referencia() {
                if cat.obtener(&p.id).is_err() {
                    cat.guardar(p, false)?;
                    agregados += 1;
                }
            }
            println!("{agregados} papeles de referencia agregados. Ajuste los calibres con la ficha de su proveedor.");
        }
    }
    Ok(())
}

fn info(ruta: &Path) -> Result<()> {
    let f = Fuente::abrir(ruta).with_context(|| format!("no se pudo abrir {}", ruta.display()))?;
    println!("{}: {} páginas", ruta.display(), f.paginas.len());
    for (i, p) in f.paginas.iter().enumerate() {
        let t = p.tamano_corte();
        println!(
            "  pág. {:>3}: formato {:.1}×{:.1} mm{}  rebase {:.1} mm  giro {}°",
            i + 1,
            t.ancho,
            t.alto,
            if p.tiene_trimbox { "" } else { " (sin TrimBox)" },
            p.rebase_disponible(),
            p.giro
        );
    }
    Ok(())
}

fn lomo(datos: &Path, a: ArgsLomo) -> Result<()> {
    let cat = Catalogo::<Papel>::abrir(datos)?;
    let tripa = cat.obtener(&a.papel)?;
    let portada = a.portada.as_deref().map(|id| cat.obtener(id)).transpose()?;
    let c = libro::calcular_lomo(a.paginas, tripa, portada, a.tolerancia)?;
    println!(
        "Tripa: {} páginas = {} hojas de {} ({} µm) → {:.2} mm",
        c.paginas, c.hojas, tripa.nombre, tripa.calibre_um, c.bloque_mm
    );
    if let Some(p) = portada {
        println!("Portada: 2 × {} ({} µm) → {:.2} mm", p.nombre, p.calibre_um, c.portada_mm);
    }
    if c.tolerancia_mm > 0.0 {
        println!("Tolerancia: {:.2} mm", c.tolerancia_mm);
    }
    println!("LOMO: {:.1} mm", c.lomo_mm);
    Ok(())
}

/// Verifica que todas las páginas tengan el mismo formato final y suficiente rebase.
fn revisar_paginas(fuente: &Fuente, formato: Option<Tamano>, rebase: f64) -> Result<(Tamano, Vec<String>)> {
    let formato = formato.unwrap_or_else(|| fuente.paginas[0].tamano_corte());
    let mut avisos = Vec::new();
    for (i, p) in fuente.paginas.iter().enumerate() {
        let t = p.tamano_corte();
        if (t.ancho - formato.ancho).abs() > 0.5 || (t.alto - formato.alto).abs() > 0.5 {
            bail!("la página {} mide {t} y el formato es {formato} (¿falta TrimBox o el giro?)", i + 1);
        }
        if p.rebase_disponible() + 0.05 < rebase {
            avisos.push(format!(
                "página {}: tiene {:.1} mm de rebase y se pidieron {rebase} mm",
                i + 1,
                p.rebase_disponible()
            ));
        }
    }
    Ok((formato, avisos))
}

fn pliego_de(m: &Maquina, pedido: Option<Tamano>) -> Result<Tamano> {
    let pliego = pedido.unwrap_or(m.pliego_max);
    if pliego.ancho > m.pliego_max.ancho + 0.01 || pliego.alto > m.pliego_max.alto + 0.01 {
        bail!("el pliego {pliego} excede el máximo de «{}» ({})", m.nombre, m.pliego_max);
    }
    Ok(pliego)
}

fn opciones_marcas(sin_marcas: bool, sin_tira_color: bool) -> OpcionesMarcas {
    let mut marcas = if sin_marcas { OpcionesMarcas::ninguna() } else { OpcionesMarcas::default() };
    if sin_tira_color {
        marcas.tira_color = false;
    }
    marcas
}

/// Escribe el PDF con la configuración de salida de la máquina e imprime los avisos.
fn escribir_salida(
    m: &Maquina,
    fuente: Fuente,
    caras: &[Cara],
    entrada: &Path,
    salida: &Path,
    mut avisos: Vec<String>,
) -> Result<()> {
    let icc = match &m.salida.perfil_icc {
        Some(ruta) => Some(std::fs::read(ruta).with_context(|| format!("no se pudo leer el ICC {}", ruta.display()))?),
        None => None,
    };
    let titulo = entrada.file_stem().map_or_else(|| "Montaje".into(), |s| s.to_string_lossy().into_owned());
    let opciones = OpcionesSalida { titulo, pdfx: m.salida.pdfx, icc, condicion: m.salida.condicion.clone() };
    let informe = pdf::escribir(fuente, caras, &opciones, salida)?;
    avisos.extend(informe.avisos);
    if m.salida.jdf {
        avisos.push("esta máquina pide JDF; la exportación JDF llega en la fase 4".into());
    }
    for aviso in &avisos {
        println!("⚠ {aviso}");
    }
    println!("✓ {} {}", salida.display(), if informe.pdfx_identificado { "(con OutputIntent)" } else { "" });
    Ok(())
}

fn nup(datos: &Path, a: ArgsNup) -> Result<()> {
    let m = Catalogo::<Maquina>::abrir(datos)?.obtener(&a.maquina)?.clone();
    let fuente = Fuente::abrir(&a.entrada).with_context(|| format!("no se pudo abrir {}", a.entrada.display()))?;
    if a.dorso && fuente.paginas.len() % 2 != 0 {
        bail!("con --dorso el PDF debe tener páginas en pares frente/dorso ({} páginas)", fuente.paginas.len());
    }
    let (pieza, avisos) = revisar_paginas(&fuente, a.formato, a.rebase)?;
    let pliego = pliego_de(&m, a.pliego)?;
    let volteo = match a.volteo {
        VolteoArg::Lateral => Volteo::Lateral,
        VolteoArg::Cabeza => Volteo::Cabeza,
    };
    let mut margenes = Margenes::de_maquina(&m);
    if a.dorso && !m.duplex {
        margenes = margenes.para_volteo(volteo);
    }
    let parametros = ParametrosNup {
        pliego,
        margenes,
        pieza,
        rebase: a.rebase,
        calle: a.calle,
        orientacion: match a.orientacion {
            OrientacionArg::Auto => Orientacion::Auto,
            OrientacionArg::Normal => Orientacion::Normal,
            OrientacionArg::Girada => Orientacion::Girada,
        },
        marcas: opciones_marcas(a.sin_marcas, a.sin_tira_color),
    };
    let d = nup::calcular(&parametros)?;

    let mut caras = Vec::new();
    if a.dorso {
        for par in 0..fuente.paginas.len() / 2 {
            let mut tiro = nup::cara_tiro(&parametros, &d, 2 * par);
            tiro.nombre = format!("Diseño {} tiro", par + 1);
            let mut retiro = nup::cara_retiro(&parametros, &d, 2 * par + 1, volteo);
            retiro.nombre = format!("Diseño {} retiro", par + 1);
            caras.extend([tiro, retiro]);
        }
    } else {
        for i in 0..fuente.paginas.len() {
            let mut tiro = nup::cara_tiro(&parametros, &d, i);
            tiro.nombre = format!("Diseño {}", i + 1);
            caras.push(tiro);
        }
    }

    println!("Máquina: {} — pliego {pliego}, pinza {} mm", m.nombre, margenes.pinza);
    println!(
        "Pieza {pieza} + rebase {} mm, calle {} mm → {} × {} = {} piezas por pliego{}",
        a.rebase,
        a.calle,
        d.columnas,
        d.filas,
        d.piezas(),
        if d.girada { " (girada 90°)" } else { "" }
    );
    println!("Aprovechamiento del pliego: {:.1} %", d.aprovechamiento);
    println!("Pliegos en el PDF: {}", caras.len());

    if a.simular {
        for aviso in &avisos {
            println!("⚠ {aviso}");
        }
        return Ok(());
    }
    escribir_salida(&m, fuente, &caras, &a.entrada, &a.salida, avisos)
}

fn libro_cmd(datos: &Path, a: ArgsLibro) -> Result<()> {
    let m = Catalogo::<Maquina>::abrir(datos)?.obtener(&a.maquina)?.clone();
    let papel = a.papel.as_deref().map(|id| Catalogo::<Papel>::abrir(datos)?.obtener(id).cloned()).transpose()?;
    let fuente = Fuente::abrir(&a.entrada).with_context(|| format!("no se pudo abrir {}", a.entrada.display()))?;
    let (pagina, mut avisos) = revisar_paginas(&fuente, a.formato, a.rebase)?;
    let encuadernacion = match a.encuadernacion {
        EncuadernacionArg::Caballete => Encuadernacion::Caballete,
        EncuadernacionArg::Lomo => Encuadernacion::Lomo,
        EncuadernacionArg::Cosido => Encuadernacion::Cosido,
    };
    let parametros = ParametrosLibro {
        pliego: pliego_de(&m, a.pliego)?,
        margenes: Margenes::de_maquina(&m),
        pagina,
        paginas: fuente.paginas.len() as u32,
        encuadernacion,
        firma: a.firma,
        rebase: a.rebase,
        fresado: a.fresado,
        refile: a.refile,
        calibre_mm: if a.sin_creep { None } else { papel.as_ref().map(Papel::calibre_mm) },
        derecha_a_izquierda: a.derecha_a_izquierda,
        marcas: opciones_marcas(a.sin_marcas, a.sin_tira_color),
    };
    let plan = firmas::planificar(&parametros)?;
    avisos.extend(plan.avisos.iter().cloned());

    println!("Máquina: {} — pliego {}", m.nombre, parametros.pliego);
    println!(
        "Libro {pagina}, {} páginas ({} + {} en blanco), {}",
        plan.paginas_libro,
        parametros.paginas,
        plan.blancas,
        match encuadernacion {
            Encuadernacion::Caballete => "a caballete (firmas anidadas)",
            Encuadernacion::Lomo => "al lomo (firmas alzadas, lomo fresado)",
            Encuadernacion::Cosido => "cosido (firmas alzadas)",
        }
    );
    let mut resumen: Vec<(u32, bool, u32)> = Vec::new();
    for f in &plan.firmas {
        match resumen.last_mut() {
            Some((n, g, cantidad)) if *n == f.paginas && *g == f.girada => *cantidad += 1,
            _ => resumen.push((f.paginas, f.girada, 1)),
        }
    }
    for (n, girada, cantidad) in resumen {
        println!("  {cantidad} × firma de {n} pp{}", if girada { " (girada 90°)" } else { "" });
    }
    for f in &plan.firmas {
        let (a, b) = (f.paginas_libro.iter().min().unwrap(), f.paginas_libro.iter().max().unwrap());
        if encuadernacion == Encuadernacion::Caballete {
            let mitad = f.paginas_libro.len() / 2;
            println!(
                "  Firma {}: págs. {}–{} y {}–{}",
                f.numero,
                f.paginas_libro[0],
                f.paginas_libro[mitad - 1],
                f.paginas_libro[mitad],
                f.paginas_libro[f.paginas_libro.len() - 1]
            );
        } else {
            println!("  Firma {}: págs. {a}–{b}", f.numero);
        }
    }
    if encuadernacion == Encuadernacion::Caballete {
        println!("Creep máximo (hoja central): {:.2} mm", plan.creep_max);
    } else if let Some(papel) = &papel {
        let c = libro::calcular_lomo(plan.paginas_libro, papel, None, 0.0)?;
        println!("Lomo del bloque: {:.1} mm (sin portada)", c.lomo_mm);
    }
    println!("Pliegos en el PDF: {} ({} firmas × tiro y retiro)", plan.caras.len(), plan.firmas.len());

    if a.simular {
        for aviso in &avisos {
            println!("⚠ {aviso}");
        }
        return Ok(());
    }
    escribir_salida(&m, fuente, &plan.caras, &a.entrada, &a.salida, avisos)
}
