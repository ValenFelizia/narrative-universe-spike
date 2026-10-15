# Spike I1

El nombre del crate `narrative-universe` es un marcador de posición. La licencia todavía no está elegida. Hasta esa decisión, este repositorio no concede permiso de uso.

Este incremento guarda páginas, tipos, entidades, definiciones y afirmaciones en un archivo SQLite. `spike seed` escribe un fixture sintético y termina. `spike inspect` abre ese archivo en otro proceso y vuelve a leerlo.

## Reproduce los checks

Hace falta Rust 1.96.0 y el compilador de C de Visual Studio, porque `rusqlite` con la feature `bundled` compila SQLite. En esta máquina `cl.exe` no estaba en `PATH`. El build usó el Developer PowerShell de Visual Studio 2022 Community 17.14.35, compilador 19.44.35228. No instalé herramientas.

Ejecuta estos comandos en la raíz del crate.

```text
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
```

Para ver la persistencia entre procesos, elige una ruta que no exista y ejecuta los dos comandos en procesos distintos.

```text
cargo run --locked --bin spike -- seed RUTA
cargo run --locked --bin spike -- inspect RUTA
```

Los dos procesos deben salir con código 0 e imprimir el mismo texto. `inspect` no debe crear un archivo si la ruta no existe, y un segundo `seed` sobre la misma ruta debe fallar sin cambiar los bytes.

## Registros

`model` define los valores. Una afirmación es texto, booleano, referencia a entidad o desconocido explícito. `store` comprueba que el sujeto, el destino y la definición existen, y que la definición aplica al sujeto. El archivo `schema.sql` repite la forma del valor y el dueño único con `CHECK` y claves foráneas. `PRAGMA foreign_keys` se activa en cada conexión. No queda guardado en el archivo.

Una página no tiene canon. Una entidad puede no tener tipo ni página. Una definición tiene exactamente un dueño, un tipo o una entidad. No hay unicidad por sujeto y definición, así que una definición singular puede guardar más de una afirmación. El canon de una afirmación es uno de `ACORDADO`, `PROVISIONAL`, `ABIERTO` o `MISTERIO_INTENCIONAL`, y el llamador lo envía en cada alta. `NULL` en un campo opcional es ausencia. Una cadena vacía queda como cadena vacía.

`spike` no decide canon ni relaciones. Llama a la biblioteca y después lee la base para imprimir el informe.

## Fixture

El detalle está en `tests/fixtures/README.md`. El fixture es sintético. La afirmación queda en `PROVISIONAL`. No guardé la membresía de War Archivist como `ACORDADO`, porque G0 pidió un mapeo editorial con fuente precisa y este incremento no lo tiene.

Los recuentos del fixture son 1 página, 2 tipos, 2 entidades, 1 definición y 1 afirmación. La id de la afirmación es `88888888-8888-4888-8888-888888888888`. La consulta desde el sujeto y la consulta hacia el destino devuelven esa misma id. Hay una sola fila en `assertions`.

## Evidencia medida

La corrida es del 2026-10-14, en Windows, con target `x86_64-pc-windows-msvc`.

| Pieza | Versión |
| --- | --- |
| rustc | 1.96.0 (ac68faa20 2026-05-25) |
| cargo | 1.96.0 (30a34c682 2026-05-25) |
| cl.exe | 19.44.35228 |
| rusqlite | 0.40.2 |
| libsqlite3-sys | 0.38.2 |
| SQLite | 3.53.2 |
| uuid | 1.28.0 |
| tempfile | 3.27.0 |

`cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings` y `cargo test --locked --all-targets` terminaron sin errores. La invocación medida de `cargo test --locked --all-targets` tardó 4185 ms con las dependencias ya compiladas. Ese tiempo incluye una compilación del perfil de test de 2,27 s. El binario de `persistence` informó 0,47 s para 24 tests y el de `process_replay` informó 0,29 s para 2 tests. La prueba de bloqueo espera hasta 250 ms dentro de esos 0,47 s. El número de 4185 ms mide el comando entero, no el trabajo de SQLite.

Después corrí `spike seed` y `spike inspect` como dos procesos `cargo run` sobre el mismo archivo nuevo. Los dos salieron con código 0 y escribieron este texto.

```text
universe_id=11111111-1111-4111-8111-111111111111
pages=1
types=2
entities=2
definitions=1
assertions=1
page_body=Texto con acento áéíóú ñ y emoji 📚
subject_label=Narrador sintético
target_label=Casa sintética
owner_type_id=33333333-3333-4333-8333-333333333333
cardinality=singular
assertion_id=88888888-8888-4888-8888-888888888888
from_subject=88888888-8888-4888-8888-888888888888
to_target=88888888-8888-4888-8888-888888888888
canon=PROVISIONAL
value_kind=reference
target_id=66666666-6666-4666-8666-666666666666
scope_note=al inicio
source_status_raw=<absent>
provenance_external_ref=<absent>
provenance_locator=<absent>
provenance_fragment=<absent>
provenance_source_revision=<absent>
archived=false
```

Leí los bytes 60 a 63 del archivo, sin pasar por la biblioteca. `user_version` era 1. El archivo medía 57344 bytes. Python no estaba instalado, así que no hubo una tercera lectura con otro lenguaje.

## Límites

`serde` y `serde_json` no están en este incremento. El plan los reserva para el JSON de I3. `uuid` solo valida ids canónicos que entrega el llamador. I1 no genera ids.

La columna `archived` se guarda y se lee. No hay comando de archivar ni de restaurar. Un alta de afirmación sobre una definición archivada hoy se acepta. I2 debe rechazarla, y el test `archived_flag_roundtrips_without_changing_canon` tendrá que cambiar con esa regla.

No hay comando para cambiar el canon. No hay consulta de posibles incompatibilidades. No hay export ni import.

El tiempo de espera por bloqueo es 250 ms. Si otra conexión tiene una transacción de escritura, el comando devuelve `Busy` y no escribe. No hay reintento ni bloqueo de workspace. La prueba no fija la duración exacta de la espera.

`PRAGMA user_version` entra en la misma transacción que el esquema. En este SQLite, `ROLLBACK` deshace ese pragma. Lo cubre `user_version_rolls_back_with_the_transaction`. No probé un corte de energía.

`create` reserva la ruta con `create_new` y después abre ese archivo sin el flag de creación de SQLite. Si la ruta ya existe, devuelve `PathExists` y no modifica el archivo. Si la inicialización falla después de la reserva, el archivo queda en disco y el error incluye `residual file`. Las claves primarias de `pages`, `types`, `entities`, `definitions` y `assertions` son `NOT NULL`.

La suite posterior a esa corrección tiene 33 tests. `cargo fmt --check` y `cargo clippy --locked --all-targets -- -D warnings` pasaron. `cargo test --locked --all-targets` tardó 2351 ms con las dependencias ya compiladas. `persistence` informó 0,84 s para 27 tests y `process_replay` informó 0,46 s para 3 tests.

Abrir una base ajena o de otra versión falla antes de migrarla. Los tests comparan los bytes del archivo. `inspect` dejó los mismos bytes que `seed` en esta corrida.

Una consulta por sujeto devuelve las filas que existen. Si el sujeto no existe, el resultado es una lista vacía. La lectura por id de un registro ausente devuelve `NotFound`.

`.gitignore` no estaba en la lista de archivos del plan. Está para no versionar `target/`.

Las dependencias directas son `rusqlite`, `uuid` y `tempfile`. `Cargo.lock` también fija las dependencias de compilación de SQLite, entre ellas `cc` y `libsqlite3-sys`.

## Fuera de este incremento

I2, I3, Tauri, interfaz, MCP, migraciones generales y el stack definitivo del producto quedan sin hacer. El siguiente paso es la revisión humana de G1.
