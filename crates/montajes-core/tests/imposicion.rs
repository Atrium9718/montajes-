//! Prueba de punta a punta: PDF de tarjetas → montaje → PDF impuesto.

use lopdf::{Document, Object, Stream, dictionary};
use montajes_core::catalogo::VersionPdfx;
use montajes_core::geometria::Tamano;
use montajes_core::imposicion::marcas::OpcionesMarcas;
use montajes_core::imposicion::nup::{self, Orientacion, ParametrosNup};
use montajes_core::imposicion::{Margenes, Volteo};
use montajes_core::pdf::{Fuente, OpcionesSalida, componer};
use montajes_core::unidades::mm_a_pt;

/// Tarjeta de 90×50 mm con 3 mm de rebase: fondo de color hasta el rebase y
/// un cuadro negro en la esquina superior izquierda para ver la orientación.
fn tarjeta(cmyk_fondo: [f64; 4]) -> String {
    let (w, h) = (mm_a_pt(96.0), mm_a_pt(56.0));
    let c = cmyk_fondo;
    format!(
        "{} {} {} {} k 0 0 {w:.3} {h:.3} re f 0 0 0 1 k {:.3} {:.3} {:.3} {:.3} re f",
        c[0],
        c[1],
        c[2],
        c[3],
        mm_a_pt(8.0),
        mm_a_pt(38.0),
        mm_a_pt(10.0),
        mm_a_pt(10.0)
    )
}

pub fn pdf_de_tarjetas() -> Vec<u8> {
    let mut doc = Document::with_version("1.6");
    let arbol = doc.new_object_id();
    let media: Object = vec![0.into(), 0.into(), mm_a_pt(96.0).into(), mm_a_pt(56.0).into()].into();
    let trim: Object =
        vec![mm_a_pt(3.0).into(), mm_a_pt(3.0).into(), mm_a_pt(93.0).into(), mm_a_pt(53.0).into()].into();
    let mut hijos = vec![];
    for fondo in [[0.0, 0.6, 1.0, 0.0], [0.8, 0.2, 0.0, 0.0]] {
        let contenido = doc.add_object(Stream::new(dictionary! {}, tarjeta(fondo).into_bytes()));
        let pagina = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => arbol,
            "MediaBox" => media.clone(),
            "TrimBox" => trim.clone(),
            "BleedBox" => media.clone(),
            "Contents" => contenido,
        });
        hijos.push(pagina.into());
    }
    doc.objects.insert(arbol, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => hijos, "Count" => 2 }));
    let catalogo = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => arbol });
    doc.trailer.set("Root", catalogo);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    bytes
}

#[test]
fn tarjetas_frente_y_dorso() {
    let fuente = Fuente::desde_bytes(&pdf_de_tarjetas()).unwrap();
    assert_eq!(fuente.paginas.len(), 2);
    let t = fuente.paginas[0].tamano_corte();
    assert!((t.ancho - 90.0).abs() < 0.01 && (t.alto - 50.0).abs() < 0.01);
    assert!((fuente.paginas[0].rebase_disponible() - 3.0).abs() < 0.01);

    let p = ParametrosNup {
        pliego: Tamano::new(480.0, 330.0),
        margenes: Margenes { pinza: 10.0, cola: 5.0, lateral: 5.0 },
        pieza: t,
        rebase: 3.0,
        calle: 0.0,
        orientacion: Orientacion::Auto,
        marcas: OpcionesMarcas::default(),
    };
    let d = nup::calcular(&p).unwrap();
    let caras = vec![nup::cara_tiro(&p, &d, 0), nup::cara_retiro(&p, &d, 1, Volteo::Lateral)];
    let opciones = OpcionesSalida {
        titulo: "Tarjetas".into(),
        pdfx: VersionPdfx::X4,
        icc: Some(vec![0; 128]),
        condicion: Some("FOGRA39".into()),
        fecha: Some(1_760_000_000),
    };
    let (mut doc, informe) = componer(fuente, &caras, &opciones).unwrap();
    assert!(informe.pdfx_identificado);
    assert_eq!(informe.pliegos, 2);

    // Se guarda y se vuelve a leer: dos pliegos del tamaño pedido.
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    if let Ok(dir) = std::env::var("MONTAJES_GUARDAR_PRUEBAS") {
        std::fs::write(format!("{dir}/tarjetas-entrada.pdf"), pdf_de_tarjetas()).unwrap();
        std::fs::write(format!("{dir}/tarjetas-impuestas.pdf"), &bytes).unwrap();
    }
    let releido = Fuente::desde_bytes(&bytes).unwrap();
    assert_eq!(releido.paginas.len(), 2);
    let pliego = releido.paginas[0].tamano_corte();
    assert!((pliego.ancho - 480.0).abs() < 0.01 && (pliego.alto - 330.0).abs() < 0.01);

    let doc = Document::load_mem(&bytes).unwrap();
    let catalogo = doc.catalog().unwrap();
    assert!(catalogo.has(b"OutputIntents"));
    assert!(catalogo.has(b"Metadata"));
    // Cada pliego dibuja 27 tarjetas.
    let primera = *doc.get_pages().values().next().unwrap();
    let contenido = String::from_utf8(doc.get_page_content(primera)).unwrap();
    assert_eq!(contenido.matches(" Do ").count(), 27);
}
