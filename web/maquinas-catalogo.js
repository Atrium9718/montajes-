// Catálogo de referencia de máquinas de impresión, a partir de fichas técnicas
// publicadas por los fabricantes y distribuidores. Medidas en mm.
//
// - pliego: pliego máximo [ancho, alto], con la pinza en el lado «ancho».
// - minimo: pliego mínimo.
// - impresion: área máxima de impresión; de ahí salen la cola y los laterales.
// - pinza: margen de pinza (offset) o pérdida de imagen en el borde de entrada (digital).
//
// Son valores de referencia: cada máquina y cada configuración puede variar,
// por eso al agregarla se abre el formulario para revisarla antes de guardar.

export const CATALOGO_MAQUINAS = [
  // ── Offset pliego · Heidelberg ──
  { modelo: "Heidelberg Speedmaster SM 52", tipo: "offset", pliego: [520, 370], minimo: [145, 105], impresion: [510, 360], pinza: 7, colores: 4, alias: "sm52 sm 52 speedmaster 52" },
  { modelo: "Heidelberg Printmaster GTO 52", tipo: "offset", pliego: [520, 360], minimo: [145, 105], impresion: [505, 340], pinza: 10, colores: 1, alias: "gto gto52 printmaster 52" },
  { modelo: "Heidelberg Printmaster QM 46", tipo: "offset", pliego: [460, 340], minimo: [140, 100], impresion: [450, 320], pinza: 10, colores: 2, alias: "qm46 qm 46 quickmaster" },
  { modelo: "Heidelberg Speedmaster SM 74", tipo: "offset", pliego: [740, 530], minimo: [280, 210], impresion: [740, 510], pinza: 10, colores: 4, alias: "sm74 sm 74 speedmaster 74", verificado: true },
  { modelo: "Heidelberg Speedmaster SX 74", tipo: "offset", pliego: [740, 530], minimo: [280, 210], impresion: [740, 510], pinza: 10, colores: 4, alias: "sx74 sx 74" },
  { modelo: "Heidelberg Speedmaster XL 75", tipo: "offset", pliego: [750, 605], minimo: [350, 210], impresion: [750, 595], pinza: 10, colores: 5, alias: "xl75 xl 75" },
  { modelo: "Heidelberg Speedmaster SM 102 / CD 102", tipo: "offset", pliego: [1020, 720], minimo: [420, 280], impresion: [1020, 710], pinza: 10, colores: 4, alias: "sm102 cd102 sm 102 cd 102 speedmaster 102" },
  { modelo: "Heidelberg Speedmaster XL 106", tipo: "offset", pliego: [1060, 750], minimo: [480, 340], impresion: [1050, 740], pinza: 10, colores: 5, alias: "xl106 xl 106" },
  // ── Offset pliego · Komori ──
  { modelo: "Komori Lithrone G29 / S29", tipo: "offset", pliego: [750, 530], minimo: [279, 200], impresion: [740, 520], pinza: 10, colores: 4, alias: "lithrone 29 g29 s29 l29" },
  { modelo: "Komori Enthrone 29", tipo: "offset", pliego: [750, 530], minimo: [297, 210], impresion: [740, 515], pinza: 10, colores: 4, alias: "enthrone e29" },
  { modelo: "Komori Lithrone G37", tipo: "offset", pliego: [940, 640], minimo: [360, 250], impresion: [930, 630], pinza: 10, colores: 4, alias: "lithrone 37 g37" },
  { modelo: "Komori Lithrone G40 / S40", tipo: "offset", pliego: [1030, 720], minimo: [460, 360], impresion: [1020, 710], pinza: 10, colores: 4, alias: "lithrone 40 g40 s40 l40" },
  { modelo: "Komori Lithrone GX40 advance", tipo: "offset", pliego: [1050, 750], minimo: [460, 360], impresion: [1040, 740], pinza: 10, colores: 5, alias: "gx40 lithrone gx40", verificado: true },
  // ── Offset pliego · Koenig & Bauer (KBA) ──
  { modelo: "Koenig & Bauer Rapida 75", tipo: "offset", pliego: [750, 530], minimo: [330, 210], impresion: [740, 515], pinza: 10, colores: 4, alias: "kba rapida 75 r75" },
  { modelo: "Koenig & Bauer Rapida 105", tipo: "offset", pliego: [1050, 720], minimo: [500, 350], impresion: [1040, 710], pinza: 10, colores: 4, alias: "kba rapida 105 r105" },
  { modelo: "Koenig & Bauer Rapida 106", tipo: "offset", pliego: [1060, 750], minimo: [510, 350], impresion: [1050, 740], pinza: 10, colores: 5, alias: "kba rapida 106 r106" },
  // ── Offset pliego · RMGT (Ryobi) / Shinohara / manroland ──
  { modelo: "Ryobi 520 / 522 / 524", tipo: "offset", pliego: [520, 365], minimo: [148, 100], impresion: [510, 355], pinza: 9, colores: 2, alias: "ryobi 520 522 524 rmgt 5" },
  { modelo: "RMGT 7 (Ryobi 750)", tipo: "offset", pliego: [750, 520], minimo: [300, 210], impresion: [740, 510], pinza: 9, colores: 4, alias: "ryobi 750 rmgt 7 rmgt7" },
  { modelo: "RMGT 9 (Ryobi 920)", tipo: "offset", pliego: [940, 650], minimo: [360, 260], impresion: [930, 640], pinza: 9, colores: 4, alias: "ryobi 920 925 rmgt 9" },
  { modelo: "Shinohara 52", tipo: "offset", pliego: [520, 365], minimo: [148, 100], impresion: [510, 355], pinza: 9, colores: 2, alias: "shinohara 52 52iv" },
  { modelo: "Shinohara 66", tipo: "offset", pliego: [660, 480], minimo: [230, 160], impresion: [650, 470], pinza: 9, colores: 4, alias: "shinohara 66 66iv" },
  { modelo: "manroland ROLAND 700", tipo: "offset", pliego: [1040, 740], minimo: [400, 280], impresion: [1020, 710], pinza: 10, colores: 5, alias: "roland 700 r700 manroland" },
  // ── Digital ──
  { modelo: "Xerox Versant 180 / 280 / 4100", tipo: "digital", pliego: [330, 488], minimo: [98, 146], impresion: [326, 480], pinza: 4, colores: 4, duplex: true, alias: "versant 180 280 3100 4100" },
  { modelo: "Xerox Iridesse", tipo: "digital", pliego: [330, 488], minimo: [98, 146], impresion: [326, 480], pinza: 4, colores: 6, duplex: true, alias: "iridesse" },
  { modelo: "Xerox PrimeLink C9070", tipo: "digital", pliego: [330, 488], minimo: [100, 148], impresion: [323, 480], pinza: 4, colores: 4, duplex: true, alias: "primelink c9070 c9065" },
  { modelo: "Fujifilm Revoria Press PC1120", tipo: "digital", pliego: [330, 488], minimo: [98, 146], impresion: [326, 480], pinza: 4, colores: 6, duplex: true, alias: "revoria pc1120 fujifilm" },
  { modelo: "Konica Minolta AccurioPress C4080 / C4070", tipo: "digital", pliego: [330, 488], minimo: [100, 148], impresion: [323, 480], pinza: 4, cola: 4.5, lateral: 3, colores: 4, duplex: true, alias: "accuriopress c4080 c4070 c4065 konica", verificado: true },
  { modelo: "Konica Minolta AccurioPress C6100 / C6085", tipo: "digital", pliego: [330, 488], minimo: [100, 148], impresion: [323, 480], pinza: 4, cola: 4.5, lateral: 3, colores: 4, duplex: true, alias: "accuriopress c6100 c6085 konica" },
  { modelo: "Konica Minolta AccurioPress C14000 / C12000", tipo: "digital", pliego: [330, 488], minimo: [100, 148], impresion: [323, 480], pinza: 4, cola: 4.5, lateral: 3, colores: 4, duplex: true, alias: "accuriopress c14000 c12000 konica" },
  { modelo: "Canon imagePRESS V900 / V1000", tipo: "digital", pliego: [330, 488], minimo: [100, 148], impresion: [323, 480], pinza: 4, colores: 4, duplex: true, alias: "imagepress v900 v1000 canon" },
  { modelo: "Canon imagePRESS C10010VP / C9010VP", tipo: "digital", pliego: [330, 488], minimo: [100, 148], impresion: [323, 480], pinza: 4, colores: 4, duplex: true, alias: "imagepress c10010vp c9010vp c910 canon" },
  { modelo: "Ricoh Pro C7200 / C9200", tipo: "digital", pliego: [330, 488], minimo: [100, 148], impresion: [323, 480], pinza: 4, colores: 5, duplex: true, alias: "ricoh pro c7200 c9200 c7210 c9210" },
  { modelo: "Ricoh Pro C5300 / C5310", tipo: "digital", pliego: [330, 488], minimo: [100, 148], impresion: [323, 480], pinza: 4, colores: 4, duplex: true, alias: "ricoh pro c5300 c5310" },
  { modelo: "HP Indigo 7K / 7900", tipo: "digital", pliego: [330, 482], minimo: [210, 148], impresion: [317, 464], pinza: 9, colores: 7, duplex: true, alias: "indigo 7900 7k 7800 hp", verificado: true },
  { modelo: "HP Indigo 12000 / 15K", tipo: "digital", pliego: [750, 530], minimo: [279, 210], impresion: [740, 510], pinza: 10, colores: 7, duplex: true, alias: "indigo 12000 15k 100k b2 hp" },
];

/** Convierte una ficha del catálogo en los datos del formulario de máquina. */
export function aMaquina(f) {
  const [ancho, alto] = f.pliego;
  const [anchoImp, altoImp] = f.impresion;
  const pinza = f.pinza;
  // Lo que queda fuera del área de impresión se reparte en cola y laterales.
  const cola = f.cola ?? Math.max(Math.round((alto - altoImp - pinza) * 10) / 10, f.tipo === "offset" ? 3 : 2);
  const lateral = f.lateral ?? Math.max(Math.round(((ancho - anchoImp) / 2) * 10) / 10, f.tipo === "offset" ? 2 : 2);
  return {
    nombre: f.modelo,
    tipo: f.tipo,
    pliego_max: { ancho, alto },
    pliego_min: f.minimo ? { ancho: f.minimo[0], alto: f.minimo[1] } : null,
    pinza,
    cola,
    lateral,
    colores: f.colores ?? 4,
    duplex: !!f.duplex,
    condicion: "FOGRA39",
    notas: `Ficha de referencia: pliego ${ancho}×${alto}, área de impresión ${anchoImp}×${altoImp} mm. Verifique con su máquina.`,
  };
}

const normalizar = (t) => t.normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase().replace(/[^a-z0-9]+/g, " ").trim();

/** Busca por palabras sueltas en modelo y alias («sm 74», «indigo», «komori 40»). */
export function buscarMaquinas(consulta, limite = 8) {
  const palabras = normalizar(consulta).split(" ").filter(Boolean);
  if (!palabras.length) return [];
  return CATALOGO_MAQUINAS
    .map((f) => {
      const texto = normalizar(`${f.modelo} ${f.alias || ""}`);
      const compacto = texto.replace(/ /g, "");
      let puntos = 0;
      for (const p of palabras) {
        if (texto.split(" ").includes(p)) puntos += 3;
        else if (texto.includes(p) || compacto.includes(p)) puntos += 2;
        else return null;
      }
      return { f, puntos };
    })
    .filter(Boolean)
    .sort((a, b) => b.puntos - a.puntos)
    .slice(0, limite)
    .map((r) => r.f);
}
