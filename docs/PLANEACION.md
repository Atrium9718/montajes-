# Montajes: planeación del software

> Software de imposición automática: recibe los PDF del cliente y entrega el **PDF listo para imprimir** (planta, pliego o montaje) con los estándares de la industria gráfica.

---

## 1. Visión

Un diseñador o un operario de preprensa sube un PDF, elige el producto (volante, tarjeta, revista, libro…) y la máquina. El sistema hace lo siguiente:

1. Revisa el archivo (preflight) y corrige lo que se pueda de forma segura.
2. Calcula la mejor imposición: número de piezas por pliego, pliegos/firmas, lomo, desplazamiento (creep), sentido de fibra y desperdicio.
3. Genera el **PDF de salida** (PDF/X-4 o PDF/X-1a) con las marcas de corte, registro, plegado, alzado y tiras de control.
4. Genera además un reporte de producción y, si se requiere, un **JDF** para el CTP/RIP.

Todo debe funcionar en modo "un clic" con valores por defecto inteligentes, sin dejar de permitir el control total a un experto.

### Principios
- **Vectorial siempre**: nunca rasterizar las páginas del cliente. Se colocan como *Form XObjects*, de modo que la salida conserva la calidad original y pesa poco.
- **Determinista**: la misma entrada y el mismo preset producen el mismo PDF, byte a byte salvo la fecha.
- **Explicable**: cada decisión automática se muestra ("Elegí 16 pp porque el pliego 70×100 admite 8 caras por lado con fibra larga").
- **Seguro**: el preflight bloquea los errores graves antes de que lleguen a la plancha.

---

## 2. Productos soportados

### 2.1 Piezas sueltas (step & repeat / n-up)
| Producto | Funciones clave |
|---|---|
| Volantes, flyers | n-up automático, rotación óptima, frente/dorso (tiro y retiro) |
| Tarjetas de presentación | ganging (varias tarjetas distintas en un pliego), cortes compartidos o con calle, numeración |
| Etiquetas, stickers | step & repeat con troquel (capa de troquel / *CutContour* como color directo) |
| Afiches, postales, invitaciones | tamaños mixtos, plegados (díptico, tríptico, acordeón, ventana) |
| Talonarios, facturas | numeración consecutiva y datos variables (VDP), *cut & stack* |

**Modos de montaje**
- **Step & repeat**: la misma pieza repetida.
- **Gang run**: distintos trabajos en un mismo pliego (bin packing con guillotina).
- **Cut & stack**: se apila y se corta, y el orden queda consecutivo al cortar (numerados, talonarios).
- **Dutch cut / montaje mixto**: piezas en orientaciones mezcladas para aprovechar mejor el pliego.

### 2.2 Revistas y libros (imposición por firmas)

**Encuadernaciones**
| Tipo | Nombre local | Particularidades del montaje |
|---|---|---|
| Saddle stitch | **Caballete** (grapa) | Firmas anidadas (una dentro de otra), creep obligatorio, páginas múltiplo de 4 |
| Perfect binding | **Al lomo / Hot-melt / PUR / fresado** | Firmas alzadas (una tras otra), margen de fresado, cálculo de lomo, marcas de alzado (escalera) |
| Sewn | **Cosido con hilo** | Firmas alzadas, sin fresado, margen de costura |
| Case bound | **Tapa dura / cartoné** | Bloque cosido o PUR + tapa con cartón, *hinge* (bisagra), vuelta de 15–18 mm, guardas |
| Wire-O / espiral | **Anillado** | Hojas sueltas o pliegos cortados, margen de perforación |
| Lay-flat, japonesa, suizo | Encuadernación suiza o expuesta | Lomo independiente, fijación de tapa solo en la contratapa |

**Variaciones del libro que se deben manejar**
- Tamaños finales libres y estándar (A4, A5, carta, media carta, 17×24, 14×21, 21×28…).
- Orientación vertical, horizontal (apaisado) y cuadrado.
- Lectura de izquierda a derecha o de derecha a izquierda (árabe, hebreo, manga).
- Firmas de 4, 8, 12, 16, 24, 32 y 64 páginas, y firmas mixtas para completar (por ejemplo 16+16+8+4).
- Páginas en blanco automáticas para completar las firmas, con aviso y una opción de dónde ubicarlas.
- Tripa a 1 tinta y portada a 4 tintas, o pliegos de color intercalados (*mixed-ink signatures*).
- Insertos y encartes, láminas en papel distinto.
- **Portada**: tapa, lomo y contratapa en un solo pliego. Solapas (izquierda/derecha), sobrecubierta, retiros de portada impresos, rebases y canales.
- Laminado, reserva UV, hot stamping (foil), troquel: capas separadas en tintas directas.

### 2.3 Plantas para litografía (offset)
- Esquemas de impresión:
  - **Tiro y retiro** (*sheetwise*): plancha distinta para cada cara.
  - **Tira y retira con la misma plancha** (*work & turn*): voltea sobre el lado largo y conserva la pinza.
  - ***Work & tumble***: voltea sobre el lado corto y cambia la pinza.
  - ***Perfecting*** (máquinas que imprimen las dos caras en una pasada).
- Perfil de máquina: tamaño máximo y mínimo de pliego, **pinza** (gripper), margen de cola, área imprimible, tamaño de plancha y offset de plancha a pliego, número de cuerpos.
- Tiras de control de color (FOGRA / Ugra / GRACoL), cuñas de densidad, marcas de registro, marcas de pinza y escuadra (*side lay*), marcas de plegado, marcas de corte, cotas de colación y cruces de centro.
- Slug o info de trabajo: cliente, OT, firma, cara, tinta, fecha.
- Salida separada por tinta o compuesta, según el RIP.

---

## 3. Motor de cálculo (el corazón)

### 3.1 Optimización de n-up
- Probar las orientaciones (0°/90°), con y sin calle, y con cortes compartidos cuando no hay rebase.
- Restricciones: área imprimible (sin pinza ni cola), rebase, calle mínima y sentido de fibra requerido.
- Objetivo: maximizar las piezas por pliego, minimizar los cortes de guillotina y el desperdicio, y respetar la fibra.
- Para gang run: *2D bin packing* con restricción de guillotina (heurísticas MaxRects/Skyline más búsqueda local).

### 3.2 Firmas y plegado
- Catálogo de esquemas de plegado según el estándar **JDF Fold Catalog** (F4-1, F8-7, F16-6, F32-9…), con su diagrama.
- Elegir automáticamente la firma más grande que quepa en el pliego y en la plegadora.
- Generar la posición de cada página (número, rotación de 0°/180° "cabeza con cabeza" o "pie con pie") por cada cara.
- Hoja de doblez (*dummy*) visual para verificar: el usuario "pliega" la firma en pantalla en 3D.

### 3.3 Fórmulas clave
- **Lomo** (perfect binding):
  `lomo = (páginas_tripa / 2) × calibre_papel + 2 × calibre_portada + tolerancia_cola`
  Calibre a partir de µm, PPI o g/m² × volumen (bulk) del catálogo de papeles.
- **Creep / desplazamiento (caballete)**:
  `desplazamiento_hoja_i = calibre × (i − 1)` desde la hoja exterior hacia la interior. Se aplica hacia el lomo (*push-in*) o se escala (*scale*), según el preset.
- **Fresado**: margen extra en el lomo de las firmas (por lo general 3 mm) para PUR o hot-melt.
- **Desperdicio y arranque**: hojas de arranque por cuerpo y por tinta para el reporte de papel.

### 3.4 Preflight (estándar GWG / PDF/X)
Verificaciones, cada una con nivel error/advertencia/info:
- Conformidad PDF/X-1a, X-3 o X-4 (ISO 15930), *OutputIntent* presente.
- Fuentes incrustadas, imágenes con resolución efectiva ≥ 300 ppi (≥ 1200 en *line art*).
- Espacios de color: RGB sin perfil, tintas directas no declaradas, TAC (cobertura total de tinta) por encima del límite del perfil (ej. 300% FOGRA39, 320% coated, 260% newsprint).
- Rebase (bleed) suficiente; TrimBox y BleedBox correctos.
- Textos negros pequeños en 4 colores (rich black) que deberían ir en sobreimpresión de K 100%.
- Líneas muy finas (< 0,1 pt), sobreimpresión de blanco, transparencias en PDF/X-1a.
- Tamaño de página distinto al producto declarado.

**Correcciones automáticas seguras**: agregar el rebase por espejo o extensión cuando falta (con aviso), convertir RGB a CMYK con el perfil del trabajo (LittleCMS), activar el overprint de negro y normalizar las cajas (Trim/Bleed).

### 3.5 Salida
- PDF/X-4 por defecto (transparencia vivo, mejor para RIPs modernos, conforme a GWG 2022) y PDF/X-1a para flujos antiguos.
- Perfiles de salida: FOGRA39/FOGRA51/FOGRA52, GRACoL 2013, SWOP, PSO Uncoated v3, ISO Coated v2.
- **JDF 1.7** (CIP4) con el layout, los esquemas de plegado y la información de producción, para Prinergy, Apogee, Prinect y otros.
- Opcional: **PPF (CIP3)** de cobertura de tinta para preajustar los tinteros de la máquina.
- Reporte PDF o HTML: miniaturas de pliegos, hoja de plegado, cálculo de papel, lomo y tiempos.

---

## 4. Automatización

- **Presets por producto** ("Libro 14×21 PUR, tripa bond 75 g, portada propalcote 300 g con solapas de 8 cm").
- **Asistente "un clic"**: detecta el tamaño, cuenta las páginas, sugiere el producto y el preset.
- **Hot folders**: soltar un PDF en una carpeta produce el PDF impuesto en la carpeta de salida.
- **API REST + CLI** para integrarse con MIS/ERP, tiendas web-to-print y sistemas de cotización.
- **Cotizador integrado**: con el preset y la tirada, calcula pliegos, planchas, papel y desperdicio.
- Cola de trabajos con estados (recibido → preflight → impuesto → aprobado → enviado a CTP).
- Aprobación del cliente con un enlace de *soft proof* (visor con el plegado y las páginas enfrentadas).

---

## 5. Arquitectura técnica

```
┌───────────────────────────────────────────────────────────┐
│ UI  (Tauri + React + TypeScript)   ·   Web (misma UI)       │
│ Asistente · Editor de pliego · Vista 3D de plegado · Cola   │
└──────────────▲──────────────────────────────▲─────────────┘
               │ IPC / WASM                    │ REST
┌──────────────┴──────────────────────────────┴─────────────┐
│ Núcleo "montajes-core" (Rust)                              │
│  ├─ modelo: Job, Producto, Componente, Firma, Pliego, Cara │
│  ├─ imposición: n-up, gang, firmas, creep, lomo            │
│  ├─ marcas: corte, registro, plegado, alzado, tiras color  │
│  ├─ preflight + correcciones                               │
│  ├─ escritor PDF (Form XObjects, PDF/X) · JDF/PPF          │
│  └─ color: LittleCMS (lcms2) + perfiles ICC                │
├────────────────────────────────────────────────────────────┤
│ Render de previsualización: PDFium                         │
│ Servicio de automatización: hot folders · API · CLI        │
└────────────────────────────────────────────────────────────┘
```

**Por qué esta pila**
- **Rust** para el núcleo: rendimiento con PDFs de cientos de páginas, seguridad de memoria, y compila tanto a binario nativo (escritorio y servidor) como a **WASM** (web), con el mismo motor en todas partes.
- Librerías PDF: `lopdf` para leer y manipular, `pdf-writer` para escribir la salida y `pdfium-render` para las miniaturas (PDFium tiene licencia BSD; evitamos MuPDF por su licencia AGPL).
- **Tauri** en lugar de Electron: un instalador liviano (~10 MB) en Windows, macOS y Linux.
- **React + TypeScript** en la UI y **Three.js** para la simulación de plegado en 3D.
- **SQLite** local para el catálogo de papeles, máquinas, presets e historial; PostgreSQL en modo servidor.

### 5.1 Modelo de datos (simplificado)
```
Trabajo
 ├─ cliente, OT, tirada, fecha
 ├─ Producto (volante | tarjeta | revista | libro | …)
 │   └─ Componentes (portada, tripa, insertos, sobrecubierta)
 │        ├─ archivo PDF + rango de páginas
 │        ├─ papel (gramaje, calibre, fibra, estucado)
 │        ├─ tintas (CMYK, directas, barniz)
 │        └─ acabado (encuadernación, laminado, troquel)
 ├─ Máquina (pliego máx/mín, pinza, plancha, cuerpos)
 └─ Imposición resultante
      └─ Pliegos → Caras (tiro/retiro) → Ubicaciones
           (página, posición, rotación, escala, creep, marcas)
```

### 5.2 Calidad
- Pruebas unitarias de cada fórmula (lomo, creep, n-up).
- Pruebas *golden*: los PDF de salida se comparan contra referencias.
- Validación con el **Ghent Output Suite** y los parches de prueba de GWG.
- Verificación de PDF/X con veraPDF o una herramienta equivalente en CI.

---

## 6. Diseño de interfaz

**Referencia visual**: [dormi, Sleep App for Students (Behance)](https://www.behance.net/gallery/232445089/dormi-Sleep-App-for-Students), a partir de las capturas que compartió el usuario.

### 6.1 Lo que tomamos de dormi
| Elemento de dormi | Cómo se traduce a Montajes |
|---|---|
| Fondo blanco hueso, mucho aire, tarjetas grandes muy redondeadas | Lienzo claro; el pliego es la tarjeta protagonista y los paneles flotan en tarjetas de 24–28 px de radio |
| **Orbes de degradado difuminado** (azul → amarillo → naranja) | Estado "calculando/listo" del montaje, portada de cada trabajo y barra de progreso de la cola. Es la firma visual de la marca |
| Tarjetas negras con texto blanco grande | Tarjeta de resultado ("27 piezas por pliego · 85 % aprovechamiento") y acciones primarias |
| Barra de navegación negra en forma de píldora, con la pestaña activa en una píldora blanca | Navegación principal: Inicio · Trabajos · Montaje · Catálogos · Ajustes |
| Números enormes (6:30, 72 %) | Métricas clave: piezas por pliego, lomo en mm, aprovechamiento %, pliegos totales |
| Resaltado lavanda detrás de frases clave | Explicación de cada decisión automática ("elegí **girar 90°** porque caben 2 más") |
| Formas tipo estrella/flor/cuadro redondeado | Íconos de tipo de producto (volante, tarjeta, revista, libro, planta) |
| Gráficas de barras apiladas azul/amarillo/naranja | Reporte de consumo de papel, planchas y desperdicio por trabajo |
| Botones en píldora negra ("Continue") y casillas con check naranja | Asistente de 3 pasos y lista de preflight con "corregir" |

### 6.2 Tokens de diseño
| Token | Valor | Uso |
|---|---|---|
| `--fondo` | `#F4F3EF` | Fondo general (blanco hueso) |
| `--superficie` | `#FFFFFF` | Tarjetas |
| `--tinta` | `#111111` | Texto, tarjetas oscuras, barra de navegación |
| `--azul` | `#4C9EF3` | Color principal, "Trustworthy blue" de dormi |
| `--naranja` | `#FF6B2C` | Acción, advertencias suaves, check activo |
| `--lima` | `#E4EA5B` | Destacados, estado "listo para imprimir" |
| `--lavanda` | `#8B8CF0` | Resaltado de texto, selección |
| `--gris` | `#8A8A86` | Texto secundario |
| Degradado de marca | `radial(#4C9EF3) → #E4EA5B → #FF6B2C`, con desenfoque de 40–60 px | Orbes |

- Tema oscuro: `--fondo #0E0E10`, tarjetas `#1A1A1D`; los orbes se mantienen.
- **Tipografía**: sans grotesca compacta para la interfaz y los números (*Inter Tight* o *Manrope*, con cifras tabulares en las medidas) y serif editorial para títulos de sección y textos explicativos (*Instrument Serif* o *Newsreader*), como la "g" del sistema tipográfico de dormi.
- Radios: 28 px en tarjetas, 999 px en botones y navegación, 12 px en campos.
- Movimiento: los orbes "respiran" mientras calcula; el pliego se arma pieza a pieza en 300 ms.

### 6.3 Pantallas principales
1. **Inicio**: saludo y "¿Qué vas a imprimir hoy?", tarjetas de producto con su forma, y trabajos recientes con su orbe.
2. **Asistente** (3 pasos): subir el PDF → elegir producto, máquina y papel → resultado en una tarjeta negra con números grandes.
3. **Preflight**: lista tipo checklist (como la de hábitos de dormi), con un botón "corregir" en cada hallazgo.
4. **Editor de pliego**: lienzo grande con zoom, capas (páginas, marcas, tira de color) y un panel con el cálculo explicado y resaltado en lavanda.
5. **Vista de libro**: páginas enfrentadas, lomo calculado en vivo con número gigante, y simulación 3D del plegado y alzado.
6. **Portada**: tapa, lomo, contratapa y solapas con guías, que se ajustan solas al cambiar páginas o papel.
7. **Catálogos**: tarjetas de máquinas (offset, digital, gran formato) y papeles; crear, duplicar y editar.
8. **Exportar**: PDF/X, JDF y reporte; enviar a hot folder o CTP.

## 7. Hoja de ruta

| Fase | Alcance | Entregable |
|---|---|---|
| **0. Fundaciones** (2–3 sem) | Repo, CI, núcleo Rust, lectura/escritura de PDF con Form XObjects, modelo de datos | CLI que coloca 1 página en un pliego con marcas de corte |
| **1. Piezas sueltas** (4–6 sem) | n-up, tiro/retiro, rebase, marcas, PDF/X-4, presets de volante y tarjeta | Volantes y tarjetas listos para imprimir |
| **2. Revistas: caballete** (4 sem) | Firmas anidadas, creep, páginas en blanco, portada separada | Revista engrapada impuesta |
| **3. Libros al lomo** (6 sem) | Firmas alzadas, fresado, marcas de escalera, cálculo de lomo, portada con solapas, tapa dura | Libro PUR/cosido/cartoné completo |
| **4. Plantas offset** (4–6 sem) | Perfiles de máquina, work & turn y tumble, tiras de color, JDF, PPF | Plantas para CTP |
| **5. Preflight pro** (4 sem) | Reglas GWG, correcciones automáticas, gestión de color | Reporte de preflight y corrección |
| **6. App y UX** (en paralelo desde la fase 1) | Tauri + React, asistente, editor, vista 3D | App de escritorio instalable |
| **7. Automatización** (4 sem) | Hot folders, API REST, cola, cotizador, gang run | Flujo sin intervención |

**MVP sugerido = fases 0–3**: volantes, tarjetas, caballete y libro al lomo, con salida PDF/X-4. Eso cubre la mayor parte del trabajo diario de una imprenta comercial.

---

## 8. Estándares de referencia
- **ISO 15930** (PDF/X-1a, X-3, X-4) e **ISO 32000** (PDF).
- **Ghent Workgroup (GWG) 2022** specs para preflight.
- **ISO 12647-2** (offset, PSO) y **G7/GRACoL** (EE. UU.).
- **CIP4 JDF 1.7 / XJDF 2.x** e **ICS** de imposición; **CIP3 PPF** para tinteros.
- **ICC v4/v2** y perfiles FOGRA.
- Convenciones de marcas de impresión y de plegado (JDF Fold Catalog).

---

## 9. Decisiones tomadas

| Tema | Decisión |
|---|---|
| Plataforma | **Web primero** (cambio de plan, para publicarla en montajes.atrioagencia.com): el motor Rust compilado a WebAssembly corre en el navegador, así que los PDF no salen del computador y basta un hosting estático. La app de escritorio (Tauri) reutilizará la misma interfaz y el mismo motor. El motor también se usa por CLI. |
| Máquinas | Hay varias de litografía y digitales. El sistema tiene un **catálogo de máquinas editable**: pliego máx/mín, pinza, cola, laterales, plancha, colores, dúplex y perfil de salida. Se guarda y se reutiliza. |
| Papeles | Todos los papeles y calibres. Hay un **catálogo de papeles editable** (gramaje, calibre, estucado, fibra, formatos) con una biblioteca inicial de referencia que se ajusta con la ficha de cada proveedor. |
| RIP/CTP | Cada máquina define su salida: versión PDF/X, perfil ICC y condición (FOGRA39, GRACoL…), y si necesita JDF. |
| Negocio | Primero para uso del taller, con la puerta abierta a venderlo. Por eso: licencias de dependencias permisivas (sin AGPL), datos en archivos portables, interfaz preparada para varios idiomas y, más adelante, multiusuario y licenciamiento. |

## 10. Estado

- [x] **Fase 0, fundaciones**: workspace Rust, catálogos de máquinas y papeles, lectura de PDF (cajas, giro y rebase), escritura con Form XObjects, OutputIntent y XMP PDF/X-4, y CLI.
- [x] **Fase 1, primera parte**: n-up con orientación automática, corte compartido o calle, tiro/retiro (lateral y de cabeza), marcas de corte y registro, tira de color, y cálculo de lomo y creep.
- [ ] Fase 1, pendiente: gang run (diseños distintos en un pliego), cut & stack, numeración, sentido de fibra en la decisión.
- [x] **Fase 2 y 3, núcleo de libros**: esquemas de plegado de 4, 8, 16, 32 y 64 pp calculados simulando el doblez (no con tablas), y reparto automático en firmas (la mayor que quepa, el resto en firmas menores). Incluye:
  - Caballete (firmas anidadas con creep por hoja), al lomo (firmas alzadas, fresado y marcas de alzado en escalera) y cosido.
  - Páginas en blanco automáticas, firma girada 90° cuando así cabe, y lectura de derecha a izquierda.
  - Marcas de plegado.
  - Comando `montajes libro`.
- [x] **Portada**:
  - Medidas calculadas a partir de las páginas y el papel: rústica con o sin solapas, y tapa dura (cartón, escuadra, vuelta, bisagra).
  - Plantilla PDF para el diseñador con guías en una capa que no imprime.
  - Armado desde páginas sueltas, o verificación de una portada completa.
  - Comando `montajes portada`.
- [x] **App web** (`web/`): inicio, piezas, libros, portadas y catálogos con el diseño dormi. Tiene vista previa del pliego en vivo, catálogos guardados en el navegador (con exportar e importar) y perfiles ICC por máquina.
- [x] **Preflight (fase 5, primera parte)**: fuentes no incrustadas, color RGB o Lab, tintas directas, resolución efectiva de imágenes (siguiendo la CTM), line art, cobertura total de tinta en rellenos CMYK, líneas finas, transparencias (error en PDF/X-1a), TrimBox, rebase y anotaciones. Disponible con `montajes preflight` y en la web.
- [x] **Correcciones automáticas** (vectoriales, sin rasterizar): sobreimprimir el negro 100 %, quitar la sobreimpresión de blancos, engrosar las líneas a 0,25 pt y generar el rebase en espejo. El preflight marca qué hallazgos corrige cada una.
- [x] **Buscador de máquinas**: un catálogo de referencia con 35 modelos de offset y digital (Heidelberg, Komori, Koenig & Bauer, RMGT/Ryobi, Shinohara, manroland, Xerox, Konica Minolta, Canon, Ricoh, HP Indigo, Fujifilm) que llena el formulario, y búsqueda en internet con Claude (búsqueda web) para los modelos que no estén en el catálogo; usa la clave de API del usuario guardada en su navegador.
- [x] **Combinado (gang run)**: varios diseños o clientes en un mismo pliego. Las posiciones se reparten para cubrir todas las cantidades con el menor número de pliegos, y cada diseño va en un bloque seguido. Acepta varios PDF (se unen) y frente/dorso. Comando `montajes combinar` y modo «Combinar varios» en la web.
- [x] **Varias firmas por pliego**: repetir la firma o montarla en tira y retira (work & turn: tiro y retiro lado a lado, un solo juego de planchas) cuando cabe dos veces. Informa juegos de planchas y pliegos por ejemplar.
- [x] **Nube del taller**: los catálogos se sincronizan con el servidor (`web/api/catalogo.php`, PHP). Usan un código secreto del taller y se guardan fuera de `public_html` con respaldos de las últimas 30 versiones. La sincronización es entre equipos, incluidos los borrados.
- [x] **Cotizador con mácula**: cada pliego distinto del trabajo (montaje, firma o combinado) suma su mácula de arranque por pasada y su porcentaje de tiraje. Con eso calcula pliegos de compra, papel según el formato de compra, planchas (la tira y retira ahorra), impresión offset por millar y arranque o digital por clic, acabados por ejemplar, millar, pliego o fijo, utilidad e impuesto. Costos y mácula se configuran por máquina y precio por papel.
- [x] **Páginas del libro y carátula**: en la web se marca qué es cada página del PDF: tripa; portada, contraportada, lomo y solapas (tiro); 2.ª y 3.ª de forros y solapas interiores (retiro); carátula exterior o interior completa; o no usar. La tripa se impone en cuadernillos automáticos o a mano (`--cuadernillos 16,16,8`). La carátula se arma con tiro y retiro reflejado y se puede descargar sola o montada en el pliego de la máquina.
- [x] **Diagramación de libros** (`web/diagramacion.js`, `web/composicion.js`): del manuscrito (Word .docx, Markdown o texto) al interior compuesto con Typst dentro del navegador (typst.ts). Detecta capítulos, partes, secciones, notas al pie, citas, listas, tablas e imágenes. Hace limpieza editorial (rayas de diálogo, comillas españolas, puntos suspensivos). Aplica reglas de libro: aperturas en impar con caída, blancas sin cornisa ni folio, cornisas par/impar, folio al pie o en la cabeza, viudas y huérfanas, silabeo en español, preliminares (portadilla, portada, página legal con ISBN, dedicatoria), índice y colofón. **Cuadra en cuadernillos**: ajusta interlineado y cuerpo dentro de límites (±6 % y ±2,5 %) para cerrar en un múltiplo exacto y completa con blancas antes del colofón. El PDF sale con TrimBox y rebase (`pdf::fijar_cajas`) y pasa directo a Libros y Portadas. Acepta fuentes propias (.otf/.ttf).
- [x] **Macula como servicio (cobro)**: el software se llama Macula y se vende por suscripción a empresas. Cuentas de empresa (NIT) o independiente (cédula) con verificación por correo y usuarios con roles (dueño, administrador, operador); una sola prueba gratis por NIT/cédula, correo y teléfono. Planes por empresa con límite de usuarios, mensual o anual. Cobro con **Wompi**: pago único por Web Checkout (tarjeta, PSE, Nequi, Bancolombia) y renovación automática con tarjeta tokenizada; webhook firmado. **Sin pago no se mueve nada**: `puerta.php` solo entrega la app a cuentas al día y el motor WebAssembly exige una licencia firmada (ECDSA P-256) por el servidor, vigente hasta 3 días sin conexión. Días de gracia, avisos por correo, panel del administrador (empresas, pagos, precios, llaves de la pasarela, días de cortesía, aliados y descuentos). Datos en SQLite fuera de `public_html`.
- [x] **Guardas aparte**: en Libros se marcan las páginas de la guarda delantera y trasera (una extendida o dos sueltas). Se arman a doble formato con el pliegue al centro y se montan aparte de la tripa y de la carátula, combinadas en el mismo pliego si caben (`generar_guardas`).
- [x] **Pliegos de lectura y volteo**: si el PDF trae páginas dobles (dos páginas seguidas por hoja) se detectan y se separan en páginas sencillas sin rasterizar (`pdf::separar_dobles`, recortando las cajas y usando la página vecina como rebase del lomo). Cada máquina guarda su volteo del retiro (de lado o de cabeza) y en Libros se puede cambiar por trabajo; de cabeza, el retiro de cada firma queda girado 180° e iguala pinza y cola. También se usa en la carátula montada.
- [x] **Tamaños de papel**: en Libros y Piezas se elige el pliego (carta, oficio, tabloide, tabloide extra 33×48, SRA3, 1/4, 1/3, 1/2 y pliego completo, los del papel del catálogo y tamaños propios guardados), siempre dentro del máximo y mínimo de la máquina y orientado como ella. La tabla «¿En qué papel sale mejor?» compara páginas o piezas por pliego, pliegos, m² de papel y aprovechamiento, y recomienda el mejor (por pliegos en digital, por área en offset).
- [x] **Papel horizontal o vertical**: el motor prueba el pliego en las dos orientaciones que entran en la máquina (pinza en el lado largo o en el corto) y se queda con la que da más piezas por pliego o menos pliegos por ejemplar; también se puede forzar horizontal o vertical. La comparación de papeles muestra la orientación elegida.
- [x] **Orientación de las páginas**: en Libros se eligen las páginas derechas, giradas 90° o la orientación que más rinde (más firmas por pliego); al lado se muestra cuántos pliegos salen de cada forma. En Piezas ya existía (normal, girada o automática).
- [x] **Escala del arte**: en Piezas y Libros se reduce (o amplía) el arte en %, con el tamaño resultante en cm; la app busca la menor reducción (hasta 70 %) que mete más piezas o gasta menos pliegos y la ofrece con un botón. El motor escala cada página en su posición, sin rasterizar (`Ubicacion.escala`).
- [x] **Hojas sueltas anidadas (corte y anidado)**: además de las firmas plegadas, en Libros se puede armar con hojas de 4 pp distintas en el mismo pliego (`Aprovechamiento::Combinar`): se imprime, se corta, cada hoja se dobla y se anida. Es el armado automático en digital a caballete. La compaginación de caballete se verificó geométricamente (espalda con espalda, parejas de lomo juntas en el pliegue y cabeza con cabeza) en 2.520 combinaciones de máquina, formato, páginas, volteo, orientación y aprovechamiento.
- [x] **Doble corte y rebase solo de fondo**: en Piezas el corte entre piezas puede ser doble (calle = 2 × rebase, cada pieza con su propio rebase; es el predeterminado) o sencillo. El rebase que falta se genera por defecto estirando la orilla de la página (0,3 mm), así sigue el fondo sin repetir textos ni logos; el espejo queda como opción.
- [x] **Compaginación verificada** (`imposicion::verificar`): cada libro se comprueba doblando los pliegos tal como salen impresos (tiro, retiro con su volteo, copias cortadas) y probando todas las formas de doblar: cada firma debe quedar en orden, al derecho y con el lomo en el último pliegue. La app lo muestra en cada montaje. Verificado además por fuera con 95 424 combinaciones. La propuesta de carátula al subir el PDF ya no saca de la tripa las páginas que difieren unos milímetros.
- [x] **Plegables de 2, 3 o 4 cuerpos** (Piezas): díptico; tríptico envolvente, acordeón o ventana; cuadríptico acordeón, envolvente o ventana doble. La app calcula el ancho de cada cuerpo (con compensación para los que se meten adentro) o usa los que se escriban, y el motor marca los pliegues punteados fuera del bloque (`nup::marcar_pliegues`, reflejados en el interior). Si el PDF viene por cuerpos, se unen en exterior e interior abiertos (`pdf::unir_cuerpos`, rebase solo en los bordes de afuera).
- [x] **Rótulo de plancha y tintas**: cada pliego lleva un rótulo con letra de trazos (`letras.rs`, sin fuentes, apto PDF/X): el nombre de cada tinta escrito en esa tinta (CYAN, MAGENTA, AMARILLO, NEGRO), el archivo y el lado (TIRO 1, RETIRO 1…, `pdf::lados_de`), en la cola sobre la tira de color o al lado del bloque (`marcas::Rotulo`). Tintas por trabajo (4×4, 4×1, 1×4, 1×1): a una tinta el lado se convierte a gris (`gris.rs`: colores, degradados tipo 2/3, imágenes Flate y JPEG RGB/CMYK, formas y patrones) y la tira de control sale solo en negro.
- [x] **Pinza por trabajo**: en Piezas y Libros se elige si la pinza muerde el lado largo (papel horizontal) o el corto (vertical) cuando el papel entra de las dos formas, y se cambia el tamaño de la pinza solo para ese trabajo (vale para esa máquina). La vista previa muestra el pliego siempre horizontal: con la pinza en el lado corto se gira 90° y la pinza queda a la izquierda.
- [x] **Rebase con fondo extendido** (predeterminado): el motor saca de cada página su capa de fondo (`fondo::forma_fondo`: imágenes grandes, rellenos grandes y sencillos, sombreados; sin textos, trazos, letras en curvas, logos, QR ni rellenos blancos; sin el recorte de la mesa de trabajo) y arma el rebase con ella (`pdf::colocar_extendido`): reflejada en cada franja, tal cual en el anillo de rebase (si la foto es más grande que la página se ve su continuación real) y la página encima. La vista previa dibuja la misma capa (`capa_fondo` en WASM).
- [x] **Rebase automático por borde** (predeterminado en Piezas y Libros): la app mira con pdf.js cada borde de cada página (a 2 px/mm) y elige: el rebase que trae el PDF si es continuación limpia de la orilla; si no, espejo (sigue degradados, fotos y texturas); y color plano del fondo si junto al corte hay textos o QR que el espejo repetiría. El motor lo arma por lados (`pdf::colocar_por_lados`): franjas en espejo o estiradas debajo, la página con su rebase original donde se usa, y el color encima. También se puede forzar un modo (original, espejo, estirar, color). La vista previa dibuja lo mismo que el PDF.
- [x] **Vista previa con el contenido real** de cada página (pdf.js), colocada con la misma matriz que usa el motor.
- [ ] Fase 3, pendiente: firmas de 12/24 pp, varias firmas iguales por pliego (work & turn de firmas), insertos.
