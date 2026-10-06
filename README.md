# montajes-

Software de imposición automática para imprenta: volantes, tarjetas, plantas offset, revistas y libros (caballete, al lomo, cosido, tapa dura). Entrega el PDF listo para imprimir (PDF/X).

- Planeación completa: [docs/PLANEACION.md](docs/PLANEACION.md)
- Motor: `crates/montajes-core` (Rust). Línea de comandos: `crates/montajes-cli`.
- App web: `web/` (estática; el motor corre en el navegador vía WebAssembly, `crates/montajes-web`). Se reconstruye con `scripts/construir-web.sh` y se prueba con `python3 -m http.server -d web`.

## Uso rápido

```sh
cargo build --release
alias montajes=target/release/montajes-cli

# 1. Registrar las máquinas del taller (se guardan en ~/.montajes)
montajes maquina agregar --id sm74 --nombre "Heidelberg SM74" --tipo offset \
  --pliego-max 740x530 --pinza 10 --cola 6 --lateral 5 \
  --icc ISOcoated_v2_eci.icc --condicion FOGRA39
montajes maquina agregar --id xerox --nombre "Xerox Versant 180" --tipo digital \
  --pliego-max 480x330 --pinza 4 --cola 4 --lateral 4 --duplex

# 2. Papeles: biblioteca inicial + los propios
montajes papel semilla
montajes papel agregar --id bond75-prov --nombre "Bond 75 g proveedor X" --gramaje 75 --calibre 101

# 3. Revisar un PDF y montarlo
montajes info tarjetas.pdf
montajes nup tarjetas.pdf -s pliego.pdf -m xerox --dorso
montajes nup volante.pdf -s planta.pdf -m sm74 --pliego 500x350 --calle 4 --dorso --volteo cabeza

# 4. Libros y revistas
montajes libro revista.pdf -s revista-pliegos.pdf -m sm74 -e caballete --papel brillante115
montajes libro novela.pdf -s novela-pliegos.pdf -m sm74 -e lomo --papel bond75 --fresado 3
montajes libro manga.pdf -s manga-pliegos.pdf -m sm74 -e cosido --derecha-a-izquierda --firma 16

# 5. Portada con lomo automático
montajes portada plantilla --tripa novela.pdf --papel bond75 --papel-portada brillante300 --solapa 80 -s plantilla.pdf
montajes portada verificar portada-del-cliente.pdf --tripa novela.pdf --papel bond75 --papel-portada brillante300
montajes portada armar tapa-y-contratapa.pdf -s portada.pdf --tripa novela.pdf --papel bond75 -m sm74
montajes portada plantilla --formato 148x210 --paginas 240 --papel bond75 --tapa-dura -s forro.pdf

# 6. Lomo de un libro
montajes lomo --paginas 240 --papel bond75 --portada brillante300 --tolerancia 0.5
```

## Publicación

La app está publicada en **https://montajes.atrioagencia.com** (Hostinger, sitio estático). Para publicar una versión nueva:

```sh
scripts/construir-web.sh
HOSTINGER_TOKEN=... scripts/publicar-hostinger.sh
```

## Desarrollo

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```
